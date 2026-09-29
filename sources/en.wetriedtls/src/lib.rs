#![no_std]
use buny::{
	Chapter, ContentBlock, ContentRating, FilterValue, Novel, NovelPageResult, NovelStatus, Result,
	Source,
	alloc::{String, Vec, string::ToString, vec},
	helpers::uri::{QueryParameters, encode_uri_component},
	imports::{
		html::Html,
		net::Request,
		std::{parse_date, send_partial_result},
	},
	prelude::*,
};
use serde_json::Value;

mod content;
mod traits;

const BASE_URL: &str = "https://wetriedtls.com";
const API_URL: &str = "https://api.wetriedtls.com";

// The API rejects perPage above 200 on /query; 30 keeps list pages light.
const NOVEL_PAGE_SIZE: i32 = 30;
// /chapters/{id} has no cap. Paid chapters come from /chapters/{id}/paid, which
// silently caps perPage at 1000 (the most any series has is ~50).
const CHAPTER_PAGE_SIZE: i32 = 500;
const PAID_PAGE_SIZE: i32 = 1000;

// /query orderBy values, in the order of the sort filter options. An unknown
// value is rejected with a 422. "latest" is also accepted but orders the same
// as "updated_at".
const SORT_VALUES: [&str; 5] = ["total_views", "updated_at", "created_at", "rating", "title"];

struct WeTriedTLS;

impl WeTriedTLS {
	// Errors come back as JSON: `{"errors": [{"message": ...}]}` for a rejected
	// param (422), `{"message": ...}` for a missing series or chapter (a 500 with
	// "Factory has thrown an error") or an unknown route (404).
	fn get_json(url: &str) -> Result<Value> {
		let json: Value = Request::get(url)?
			.header("Accept", "application/json")
			.header("Referer", &format!("{BASE_URL}/"))
			.json_owned()?;
		if let Some(msg) = json["errors"][0]["message"].as_str() {
			bail!("{msg}");
		}
		if let Some(msg) = json["message"].as_str()
			&& json.get("data").is_none()
		{
			if msg == "Factory has thrown an error" {
				bail!("Not found on We Tried TLS");
			}
			bail!("{msg}");
		}
		Ok(json)
	}

	fn novel_list(qs: &mut QueryParameters, page: i32) -> Result<NovelPageResult> {
		// `adult` makes no difference today, but the site sends it and it keeps
		// any adult series from being gated out later.
		qs.set("adult", Some("true"));
		qs.set("page", Some(&page.max(1).to_string()));
		qs.set("perPage", Some(&NOVEL_PAGE_SIZE.to_string()));

		let json = Self::get_json(&format!("{API_URL}/query?{qs}"))?;
		let items = json["data"]
			.as_array()
			.ok_or(error!("Invalid novel list"))?;
		let current = json["meta"]["current_page"].as_i64().unwrap_or(1);
		let last = json["meta"]["last_page"].as_i64().unwrap_or(1);

		Ok(NovelPageResult {
			entries: items.iter().filter_map(parse_novel_item).collect(),
			has_next_page: current < last,
		})
	}

	fn listing(id: &str, page: i32) -> Result<NovelPageResult> {
		let order_by = match id {
			"popular" => "total_views",
			"latest" => "updated_at",
			"newest" => "created_at",
			"top_rated" => "rating",
			_ => bail!("Unknown listing: {id}"),
		};
		let mut qs = QueryParameters::new();
		qs.push("query_string", Some(""));
		qs.push("orderBy", Some(order_by));
		qs.push("order", Some("desc"));
		Self::novel_list(&mut qs, page)
	}

	fn chapter_page(series_id: i64, novel_key: &str, page: i32) -> Result<(Vec<Chapter>, bool)> {
		let json = Self::get_json(&format!(
			"{API_URL}/chapters/{series_id}?page={page}&perPage={CHAPTER_PAGE_SIZE}&order=asc"
		))?;
		let items = json["data"]
			.as_array()
			.ok_or(error!("Invalid chapter list"))?;
		let last = json["meta"]["last_page"].as_i64().unwrap_or(1);
		let chapters = items
			.iter()
			.filter_map(|ch| parse_chapter(ch, novel_key, false))
			.collect();
		Ok((chapters, (page as i64) < last))
	}

	// Paid (early access) chapters are the newest ones and are not part of the
	// free list, so they are listed as locked after the last free page.
	fn paid_chapters(series_id: i64, novel_key: &str) -> Result<Vec<Chapter>> {
		let json = Self::get_json(&format!(
			"{API_URL}/chapters/{series_id}/paid?page=1&perPage={PAID_PAGE_SIZE}&order=asc"
		))?;
		let items = json["data"]
			.as_array()
			.ok_or(error!("Invalid paid chapter list"))?;
		Ok(items
			.iter()
			.filter_map(|ch| parse_chapter(ch, novel_key, true))
			.collect())
	}
}

impl Source for WeTriedTLS {
	fn new() -> Self {
		Self
	}

	fn get_search_novel_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<NovelPageResult> {
		let mut qs = QueryParameters::new();
		qs.push(
			"query_string",
			Some(query.as_deref().map(str::trim).unwrap_or("")),
		);
		qs.push("orderBy", Some(SORT_VALUES[0]));
		qs.push("order", Some("desc"));

		for filter in filters {
			match filter {
				FilterValue::Sort {
					index, ascending, ..
				} => {
					if let Some(value) = SORT_VALUES.get(index as usize) {
						qs.set("orderBy", Some(value));
						qs.set("order", Some(if ascending { "asc" } else { "desc" }));
					}
				}
				FilterValue::Select { id, value } if id == "status" && value != "All" => {
					qs.push("status", Some(&value));
				}
				// Tags go as one JSON array param, `tags_ids=[58,21]`, and match
				// series that have all of them. The comma must be percent-encoded:
				// a literal one makes the API reject every element as not a number.
				FilterValue::MultiSelect { id, included, .. }
					if id == "tags" && !included.is_empty() =>
				{
					qs.push("tags_ids", Some(&format!("[{}]", included.join(","))));
				}
				_ => {}
			}
		}

		Self::novel_list(&mut qs, page)
	}

	fn get_novel_update(
		&self,
		mut novel: Novel,
		needs_details: bool,
		needs_chapters: bool,
		page: i32,
	) -> Result<Novel> {
		// Chapters are listed by the numeric series id, which only the series
		// endpoint returns, so it is fetched either way.
		let json = Self::get_json(&format!("{API_URL}/series/{}", novel.key))?;
		let series_id = json["id"].as_i64().ok_or(error!("Invalid series"))?;

		if needs_details {
			if let Some(title) = json["title"].as_str() {
				novel.title = title.trim().to_string();
			}
			novel.cover = json["thumbnail"].as_str().and_then(|c| cover_url(c, 640));
			novel.url = Some(format!("{BASE_URL}/series/{}", novel.key));
			novel.description = json["description"].as_str().map(content::plain_text);
			novel.authors = json["author"]
				.as_str()
				.map(str::trim)
				.filter(|a| !a.is_empty())
				.map(|a| vec![a.to_string()]);
			novel.tags = Some(tag_names(&json["tags"]));
			novel.status = parse_status(json["status"].as_str());
			novel.content_rating = content_rating(&json["tags"]);

			if needs_chapters {
				send_partial_result(&novel);
			}
		}

		if needs_chapters {
			let page = page.max(1);
			let (mut chapters, has_more) = Self::chapter_page(series_id, &novel.key, page)?;
			if !has_more {
				// A failed paid list shouldn't hide the free chapters.
				chapters.extend(Self::paid_chapters(series_id, &novel.key).unwrap_or_default());
				// Paid chapters can sit between free ones (a series can free a
				// later chapter first), so keep the page in reading order.
				chapters.sort_by(|a, b| {
					a.chapter_number
						.partial_cmp(&b.chapter_number)
						.unwrap_or(core::cmp::Ordering::Equal)
				});
			}
			novel.chapters = Some(chapters);
			novel.has_more_chapters = Some(has_more);
		}

		Ok(novel)
	}

	fn get_chapter_content_list(
		&self,
		novel: Novel,
		chapter: Chapter,
	) -> Result<Vec<ContentBlock>> {
		let json = Self::get_json(&format!("{API_URL}/chapter/{}/{}", novel.key, chapter.key))?;

		if json["paywall"].as_bool().unwrap_or(false) {
			match json["chapter"]["price"].as_i64() {
				Some(price) if price > 0 => bail!(
					"This chapter is premium on We Tried TLS ({price} coins). It can only be unlocked on the website."
				),
				_ => bail!(
					"This chapter is premium on We Tried TLS. It can only be unlocked on the website."
				),
			}
		}

		let body = json["chapter"]["chapter_content"]
			.as_str()
			.filter(|b| !b.trim().is_empty())
			.ok_or(error!("Chapter has no text"))?;
		let headings = [
			json["chapter"]["chapter_name"].as_str().unwrap_or(""),
			json["chapter"]["chapter_title"].as_str().unwrap_or(""),
			json["chapter"]["series"]["title"].as_str().unwrap_or(""),
		];
		let blocks = content::chapter_blocks(body, &headings);
		if blocks.is_empty() {
			bail!("Chapter has no text");
		}
		Ok(blocks)
	}
}

fn parse_novel_item(item: &Value) -> Option<Novel> {
	let key = item["series_slug"].as_str().filter(|s| !s.is_empty())?;
	// Every series is a novel today; the CMS also supports comics.
	if item["series_type"].as_str().is_some_and(|t| t != "Novel") {
		return None;
	}
	Some(Novel {
		key: key.to_string(),
		title: item["title"].as_str().unwrap_or("").trim().to_string(),
		cover: item["thumbnail"].as_str().and_then(|c| cover_url(c, 384)),
		description: item["description"].as_str().map(content::plain_text),
		url: Some(format!("{BASE_URL}/series/{key}")),
		tags: Some(tag_names(&item["tags"])),
		status: parse_status(item["status"].as_str()),
		content_rating: content_rating(&item["tags"]),
		..Default::default()
	})
}

// Covers are full-size uploads (median ~2 MB, some ~5 MB). The site's own
// Next.js image optimizer serves them resized, as AVIF, at the widths in its
// srcset (384 and 640 among them); other widths are rejected. It only proxies
// the media host, so anything else is used as is.
fn cover_url(thumbnail: &str, width: u32) -> Option<String> {
	let thumbnail = thumbnail.trim();
	if thumbnail.is_empty() {
		return None;
	}
	if !thumbnail.starts_with("https://media.reaperscans.net/") {
		return Some(thumbnail.to_string());
	}
	Some(format!(
		"{BASE_URL}/_next/image?url={}&w={width}&q=75",
		encode_uri_component(thumbnail)
	))
}

fn tag_names(tags: &Value) -> Vec<String> {
	tags.as_array()
		.into_iter()
		.flatten()
		.filter_map(|t| t["name"].as_str())
		.map(str::trim)
		.filter(|n| !n.is_empty())
		.map(str::to_string)
		.collect()
}

// There's no explicit flag; the site tags 19+ series "Adult" and a few more "Mature".
fn content_rating(tags: &Value) -> ContentRating {
	let has = |name: &str| {
		tags.as_array()
			.is_some_and(|t| t.iter().any(|t| t["name"].as_str() == Some(name)))
	};
	if has("Adult") {
		ContentRating::NSFW
	} else if has("Mature") {
		ContentRating::Suggestive
	} else {
		ContentRating::Safe
	}
}

// The API's status enum: Ongoing, Completed, Hiatus, Dropped, Canceled.
fn parse_status(status: Option<&str>) -> NovelStatus {
	match status {
		Some("Ongoing") => NovelStatus::Ongoing,
		Some("Completed") => NovelStatus::Completed,
		Some("Hiatus") => NovelStatus::Hiatus,
		Some("Dropped" | "Canceled") => NovelStatus::Cancelled,
		_ => NovelStatus::Unknown,
	}
}

fn parse_chapter(ch: &Value, novel_key: &str, locked: bool) -> Option<Chapter> {
	let key = ch["chapter_slug"].as_str().filter(|s| !s.is_empty())?;
	Some(Chapter {
		title: chapter_title(
			ch["chapter_name"].as_str().unwrap_or(""),
			ch["chapter_title"].as_str(),
		),
		// "12.0", or "140.5" for an extra such as "Author's Q&A (1)".
		chapter_number: ch["index"].as_str().and_then(|i| i.trim().parse().ok()),
		date_uploaded: ch["created_at"].as_str().and_then(parse_timestamp),
		url: Some(format!("{BASE_URL}/series/{novel_key}/{key}")),
		locked,
		key: key.to_string(),
		..Default::default()
	})
}

// "Chapter 12" (also misspelt "Chatper", "Chaoter", "Chaper") followed by an
// optional remainder such as "- END", "(IF)" or ": Name". Returns the remainder.
fn strip_chapter_prefix(s: &str) -> Option<&str> {
	let word_end = s
		.find(|c: char| !c.is_ascii_alphabetic())
		.unwrap_or(s.len());
	let word = &s[..word_end];
	if !["chapter", "chatper", "chaoter", "chaper"]
		.iter()
		.any(|w| word.eq_ignore_ascii_case(w))
	{
		return None;
	}
	let rest = s[word_end..].trim_start();
	let after = rest.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
	if after.len() == rest.len() {
		return None;
	}
	Some(
		after.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '-' | ':' | '–' | '.')),
	)
}

// The app shows the chapter number separately, so the title drops "Chapter N".
// Extras ("Author's Q&A (1)", "Prologue") keep their name, since it is the only
// thing that tells them apart from a numbered chapter.
fn chapter_title(name: &str, title: Option<&str>) -> Option<String> {
	let name = name.trim();
	let title = title.map(str::trim).filter(|t| !t.is_empty());
	match strip_chapter_prefix(name) {
		Some(rest) => {
			// Some titles repeat the number: "Chapter 526: Seo Hweol's Memories (3)".
			let title = title
				.map(|t| strip_chapter_prefix(t).unwrap_or(t).trim())
				.filter(|t| !t.is_empty());
			let rest = rest.trim();
			title
				.or((!rest.is_empty()).then_some(rest))
				.map(str::to_string)
		}
		None => match title {
			Some(t) if name.is_empty() || t.contains(name) => Some(t.to_string()),
			Some(t) => Some(format!("{name}: {t}")),
			None => (!name.is_empty()).then(|| name.to_string()),
		},
	}
}

// Parses a timestamp such as "2024-01-11T20:32:05.271Z" (always UTC).
fn parse_timestamp(value: &str) -> Option<i64> {
	let value = value.get(..19)?.replace('T', " ");
	parse_date(value, "yyyy-MM-dd HH:mm:ss")
}

// Decodes the entities the site's editor emits. Anything else goes to the
// host's full decoder; buny-test-runner's only knows a handful (not &nbsp;),
// so the common ones are handled here to behave the same in tests and the app.
fn unescape(text: &str) -> String {
	if !text.contains('&') {
		return text.to_string();
	}
	let mut out = String::with_capacity(text.len());
	let mut rest = text;
	while let Some(amp) = rest.find('&') {
		out.push_str(&rest[..amp]);
		rest = &rest[amp..];
		let entity = rest[1..]
			.find(';')
			.filter(|&end| end <= 10)
			.map(|end| &rest[1..=end]);
		let decoded = entity.and_then(|name| match name {
			"nbsp" => Some('\u{a0}'),
			"amp" => Some('&'),
			"lt" => Some('<'),
			"gt" => Some('>'),
			"quot" => Some('"'),
			"apos" => Some('\''),
			"lsquo" => Some('‘'),
			"rsquo" => Some('’'),
			"ldquo" => Some('“'),
			"rdquo" => Some('”'),
			"hellip" => Some('…'),
			"mdash" => Some('—'),
			"ndash" => Some('–'),
			_ => {
				let num = name.strip_prefix('#')?;
				let code = match num.strip_prefix(['x', 'X']) {
					Some(hex) => u32::from_str_radix(hex, 16).ok()?,
					None => num.parse().ok()?,
				};
				char::from_u32(code)
			}
		});
		match (entity, decoded) {
			(Some(name), Some(c)) => {
				out.push(c);
				rest = &rest[name.len() + 2..];
			}
			(Some(name), None) if name.bytes().all(|b| b.is_ascii_alphanumeric()) => {
				return Html::unescape(text).unwrap_or_else(|| text.to_string());
			}
			_ => {
				out.push('&');
				rest = &rest[1..];
			}
		}
	}
	out.push_str(rest);
	out
}

register_source!(WeTriedTLS, ListingProvider, Home, DeepLinkHandler);

#[cfg(test)]
mod tests;
