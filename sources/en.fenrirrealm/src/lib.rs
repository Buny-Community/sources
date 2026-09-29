#![no_std]
use buny::{
	Chapter, ContentBlock, ContentRating, FilterValue, Novel, NovelPageResult, NovelStatus, Result,
	Source,
	alloc::{String, Vec, string::ToString},
	helpers::uri::QueryParameters,
	imports::{
		html::Html,
		net::{Request, TimeUnit, set_rate_limit},
		std::{parse_date, send_partial_result},
	},
	prelude::*,
};
use serde_json::Value;

mod content;
mod traits;

const BASE_URL: &str = "https://fenrirealm.com";
const API_URL: &str = "https://fenrirealm.com/api/new/v2";

// /series rejects per_page above 100 ("The per page field must not be greater than 100.").
const NOVEL_PAGE_SIZE: i32 = 24;

// /series sort values, in the order of the sort filter options. An unknown value
// is rejected ("The selected sort is invalid."). There is no sort direction.
// "free" and "premium" order by the number of free and paid chapters.
const SORT_VALUES: [&str; 8] = [
	"popular", "trending", "updated", "latest", "oldest", "title", "free", "premium",
];

struct FenrirRealm;

impl FenrirRealm {
	// Errors come back as `{"message": ...}`: a rejected param (422), an unknown
	// series or chapter (404, "An error occurred"), or the reader rate limit (429,
	// "Too Many Attempts." with `rateLimit.retryAfterSeconds`).
	fn get_json(url: &str) -> Result<Value> {
		let json: Value = Request::get(url)?
			.header("Accept", "application/json")
			.header("Referer", &format!("{BASE_URL}/"))
			.json_owned()?;
		if let Some(msg) = json["message"].as_str()
			&& json.get("data").is_none()
		{
			if let Some(secs) = json["rateLimit"]["retryAfterSeconds"].as_i64() {
				let unit = if secs == 1 { "second" } else { "seconds" };
				bail!(
					"Fenrir Realm is limiting how fast chapters load. Try again in {secs} {unit}."
				);
			}
			if msg == "An error occurred" {
				bail!("Not found on Fenrir Realm");
			}
			bail!("{msg}");
		}
		Ok(json)
	}

	fn novel_list(qs: &mut QueryParameters, page: i32) -> Result<NovelPageResult> {
		qs.set("page", Some(&page.max(1).to_string()));
		qs.set("per_page", Some(&NOVEL_PAGE_SIZE.to_string()));

		let json = Self::get_json(&format!("{API_URL}/series?{qs}"))?;
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
		if !["popular", "trending", "updated", "latest"].contains(&id) {
			bail!("Unknown listing: {id}");
		}
		let mut qs = QueryParameters::new();
		qs.push("sort", Some(id));
		Self::novel_list(&mut qs, page)
	}

	// Every chapter comes in one reply (~550 bytes each; 900 chapters is 500 KB),
	// in reading order.
	fn chapters(novel_key: &str) -> Result<Vec<Chapter>> {
		let json = Self::get_json(&format!("{API_URL}/series/{novel_key}/chapters"))?;
		let items = json.as_array().ok_or(error!("Invalid chapter list"))?;
		Ok(parse_chapters(items, novel_key))
	}
}

impl Source for FenrirRealm {
	fn new() -> Self {
		// Chapter reads are limited per visitor: 32 in a row, then "Too Many
		// Attempts." for ~40 seconds. The host queues requests past this limit,
		// so downloading many chapters waits instead of failing. It counts every
		// request, which browsing stays well under.
		set_rate_limit(30, 60, TimeUnit::Seconds);
		Self
	}

	fn get_search_novel_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<NovelPageResult> {
		let mut qs = QueryParameters::new();
		if let Some(query) = query.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
			qs.push("search", Some(query));
		}
		let mut sort = SORT_VALUES[0];

		for filter in filters {
			match filter {
				FilterValue::Sort { index, .. } => {
					if let Some(value) = SORT_VALUES.get(index as usize) {
						sort = value;
					}
				}
				FilterValue::Select { id, value } => match id.as_str() {
					"type" if value != "any" => qs.push("type", Some(&value)),
					// Only sent with two or more genres or tags, as the site does.
					"genre_condition" | "tag_condition" => qs.push(&id, Some(&value)),
					_ => {}
				},
				// Multi-value params repeat with a `[]` suffix (`genres[]=1&genres[]=7`);
				// a comma-joined list matches nothing.
				FilterValue::MultiSelect {
					id,
					included,
					excluded,
				} => {
					let (include, exclude) = match id.as_str() {
						"status" => ("statuses[]", "exclude_status[]"),
						"genres" => ("genres[]", "exclude_genres[]"),
						"tags" => ("tags[]", "exclude_tags[]"),
						_ => continue,
					};
					for value in &included {
						qs.push(include, Some(value));
					}
					for value in &excluded {
						qs.push(exclude, Some(value));
					}
				}
				_ => {}
			}
		}
		qs.push("sort", Some(sort));

		Self::novel_list(&mut qs, page)
	}

	fn get_novel_update(
		&self,
		mut novel: Novel,
		needs_details: bool,
		needs_chapters: bool,
		_page: i32,
	) -> Result<Novel> {
		if needs_details {
			let json = Self::get_json(&format!("{API_URL}/series/{}", novel.key))?;
			if let Some(title) = json["title"].as_str() {
				novel.title = title.trim().to_string();
			}
			novel.cover = json["cover"].as_str().and_then(|c| cover_url(c, 600, 800));
			novel.url = Some(format!("{BASE_URL}/series/{}", novel.key));
			novel.description = json["description"]
				.as_str()
				.map(content::plain_text)
				.filter(|d| !d.is_empty());
			novel.tags = Some(tag_names(&json));
			novel.status = parse_status(json["status"].as_str());
			novel.content_rating = content_rating(&json);

			if needs_chapters {
				send_partial_result(&novel);
			}
		}

		if needs_chapters {
			novel.chapters = Some(Self::chapters(&novel.key)?);
			novel.has_more_chapters = Some(false);
		}

		Ok(novel)
	}

	fn get_chapter_content_list(
		&self,
		_novel: Novel,
		chapter: Chapter,
	) -> Result<Vec<ContentBlock>> {
		let json = Self::get_json(&format!("{API_URL}/chapters/{}", chapter.key))?;

		// A paid chapter comes back with the same fields, but `content` is only the
		// `excerpt` (the heading and first line) in `content_format` "text". The
		// site's reader also knows a "locked" format and a "..." body.
		let format = json["content_format"].as_str().unwrap_or("json");
		let body = json["content"].as_str().unwrap_or("").trim();
		let price = json["locked"]["price"].as_i64().unwrap_or(0);
		if format == "locked" || body == "..." || (price > 0 && format == "text") {
			if price > 0 {
				bail!(
					"This chapter is premium on Fenrir Realm ({price} runes). It can only be unlocked on the website."
				);
			}
			bail!(
				"This chapter is premium on Fenrir Realm. It can only be unlocked on the website."
			);
		}
		if body.is_empty() {
			bail!("Chapter has no text");
		}

		let headings = [
			json["name"].as_str().unwrap_or(""),
			json["title"].as_str().unwrap_or(""),
		];
		let blocks = match format {
			"html" => content::html_blocks(body, &headings),
			"text" => content::text_blocks(body, &headings),
			// TipTap documents, stored as a JSON string. Older chapters only.
			_ => {
				let doc: Value =
					serde_json::from_str(body).map_err(|_| error!("Invalid chapter content"))?;
				content::doc_blocks(&doc, &headings)
			}
		};
		if blocks.is_empty() {
			bail!("Chapter has no text");
		}
		Ok(blocks)
	}
}

fn parse_novel_item(item: &Value) -> Option<Novel> {
	let key = item["slug"].as_str().filter(|s| !s.is_empty())?;
	// One series is a webtoon: its chapters are images, which a novel reader
	// can't show.
	if item["type"].as_str() == Some("webtoon") {
		return None;
	}
	Some(Novel {
		key: key.to_string(),
		title: item["title"].as_str().unwrap_or("").trim().to_string(),
		cover: item["cover"].as_str().and_then(|c| cover_url(c, 300, 400)),
		description: item["description"]
			.as_str()
			.map(content::plain_text)
			.filter(|d| !d.is_empty()),
		url: Some(format!("{BASE_URL}/series/{key}")),
		tags: Some(tag_names(item)),
		status: parse_status(item["status"].as_str()),
		content_rating: content_rating(item),
		..Default::default()
	})
}

// Covers are uploads of up to ~1 MB (1024x1536 PNGs). The site serves them
// resized with `?width=&height=`, at 300x400 in lists and 600x800 on the
// series page; those are the sizes used here.
fn cover_url(cover: &str, width: u32, height: u32) -> Option<String> {
	let cover = cover.trim().trim_start_matches('/');
	if cover.is_empty() {
		return None;
	}
	if cover.starts_with("http") {
		return Some(cover.to_string());
	}
	Some(format!("{BASE_URL}/{cover}?width={width}&height={height}"))
}

// Genres (35, broad) first, then tags (~770, specific).
fn tag_names(series: &Value) -> Vec<String> {
	["genres", "tags"]
		.iter()
		.flat_map(|key| series[key].as_array().into_iter().flatten())
		.filter_map(|t| t["name"].as_str())
		.map(str::trim)
		.filter(|n| !n.is_empty())
		.map(str::to_string)
		.collect()
}

// `content_rating` is "r18" on a couple of series and null on the rest, so the
// adult genres decide too.
fn content_rating(series: &Value) -> ContentRating {
	let has = |name: &str| {
		series["genres"]
			.as_array()
			.is_some_and(|g| g.iter().any(|g| g["name"].as_str() == Some(name)))
	};
	if series["content_rating"].as_str() == Some("r18") || has("Adult") || has("Smut") {
		ContentRating::NSFW
	} else if has("Mature") || has("Ecchi") {
		ContentRating::Suggestive
	} else {
		ContentRating::Safe
	}
}

// The API's statuses: Ongoing, Completed, Hiatus, On Hold, Dropped.
fn parse_status(status: Option<&str>) -> NovelStatus {
	match status {
		Some("Ongoing") => NovelStatus::Ongoing,
		Some("Completed") => NovelStatus::Completed,
		Some("Hiatus" | "On Hold") => NovelStatus::Hiatus,
		Some("Dropped") => NovelStatus::Cancelled,
		_ => NovelStatus::Unknown,
	}
}

// The number a chapter would show: `number`, plus `part` as a decimal
// (number 17, part 1 is "17.1"). `name` is a label built from these, but not
// always the same way ("Chapter 21.5" for number 21, part 1), so it isn't parsed.
fn site_number(ch: &Value) -> Option<f32> {
	let number = ch["number"].as_f64()?;
	let part = ch["part"].as_f64().unwrap_or(0.0);
	let decimal = if part <= 0.0 {
		0.0
	} else if part < 10.0 {
		part / 10.0
	} else {
		part / 100.0
	};
	Some((number + decimal) as f32)
}

fn parse_chapters(items: &[Value], novel_key: &str) -> Vec<Chapter> {
	// Volumes ("group") are listed interleaved, and `index` restarts in each, so
	// reading order is (volume, index). A stable sort keeps the list order for
	// the rest.
	let volume = |ch: &Value| ch["group"]["index"].as_f64().map(|v| v as f32);
	let mut items: Vec<&Value> = items.iter().collect();
	items.sort_by_key(|ch| {
		(
			ch["group"]["index"].as_i64().unwrap_or(0),
			ch["index"].as_i64().unwrap_or(0),
		)
	});

	// Reader sorts on (volume, chapter_number) and merges chapters with the same
	// pair, so the numbers must rise. The site's do in almost every series (per
	// volume, where a series has them), but typos repeat a number (two
	// "Chapter 820"s) or jump back. Those series are numbered by position instead.
	let all_grouped = items.iter().all(|ch| volume(ch).is_some());
	let key = |ch: &Value| (if all_grouped { volume(ch) } else { None }, site_number(ch));
	let rising = items.iter().all(|ch| site_number(ch).is_some())
		&& items.windows(2).all(|w| {
			let (a, b) = (key(w[0]), key(w[1]));
			match (a.0, b.0) {
				(Some(va), Some(vb)) if va != vb => va < vb,
				_ => a.1 < b.1,
			}
		});

	items
		.into_iter()
		.enumerate()
		.filter_map(|(i, ch)| {
			let id = ch["id"].as_i64()?;
			let (volume_number, chapter_number) = if rising {
				key(ch)
			} else {
				(None, Some((i + 1) as f32))
			};
			let slug = ch["slug"].as_str().unwrap_or("");
			Some(Chapter {
				key: id.to_string(),
				title: chapter_title(
					ch["name"].as_str().unwrap_or(""),
					ch["title"].as_str(),
					if rising { site_number(ch) } else { None },
				),
				chapter_number,
				volume_number,
				date_uploaded: ch["created_at"].as_str().and_then(parse_timestamp),
				url: (!slug.is_empty()).then(|| format!("{BASE_URL}/series/{novel_key}/{slug}")),
				// Free chapters have price 0; paid ones stay priced until bought.
				locked: ch["locked"]["price"].as_i64().unwrap_or(0) > 0,
				..Default::default()
			})
		})
		.collect()
}

// Splits "Chapter 12", "Chapter 12.5", "Ch. 12", "12." or "12:" off the start
// of `s`. Returns the number and the rest, without its leading separator.
fn split_label(s: &str) -> Option<(f32, &str)> {
	let s = s.trim_start();
	let word_end = s
		.find(|c: char| !c.is_ascii_alphabetic() && c != '.')
		.unwrap_or(s.len());
	let word = &s[..word_end];
	let rest = if word.is_empty() {
		s
	} else if ["chapter", "chapter.", "ch.", "ch", "chatper", "chaper"]
		.iter()
		.any(|w| word.eq_ignore_ascii_case(w))
	{
		// "Chapter . 5", "Chapter: 5"
		s[word_end..].trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '.' | ':'))
	} else {
		return None;
	};
	let digits_end = rest
		.find(|c: char| !c.is_ascii_digit() && c != '.')
		.unwrap_or(rest.len());
	let digits = rest[..digits_end].trim_end_matches('.');
	if digits.is_empty() {
		return None;
	}
	let number: f32 = digits.parse().ok()?;
	let after = &rest[digits_end..];
	// A bare number must be followed by a separator ("12. Name", "12: Name"),
	// so a title that starts with a number ("100,000 Spirit Stones") stays.
	if word.is_empty()
		&& !rest[digits.len()..].starts_with(['.', ':'])
		&& !after.trim_start().starts_with(['-', '–', '—', ':'])
	{
		return None;
	}
	Some((
		number,
		after.trim_start_matches(|c: char| {
			c.is_whitespace() || matches!(c, '-' | ':' | '–' | '—' | '.')
		}),
	))
}

// The app shows the chapter number separately, so a "Chapter N" label is
// dropped from the title when N is the number sent. `name` is "Chapter N" or
// "Chapter N - Title"; `title` is the title alone, but sometimes repeats the
// label ("Chapter 765. Reversal.") or is just "Chapter 17".
fn chapter_title(name: &str, title: Option<&str>, number: Option<f32>) -> Option<String> {
	let strip = |s: &str| -> String {
		let s = s.trim();
		match (split_label(s), number) {
			(Some((n, rest)), Some(sent)) if n == sent => rest.trim().to_string(),
			_ => s.to_string(),
		}
	};
	let from_title = title.map(strip).filter(|t| !t.is_empty());
	let from_name = strip(name);
	let title = from_title.or((!from_name.is_empty()).then_some(from_name))?;
	// "Chapter 01" when the number is 1.
	match (split_label(&title), number) {
		(Some((n, rest)), Some(sent)) if n == sent && rest.trim().is_empty() => None,
		_ => Some(title),
	}
}

// Parses a timestamp such as "2025-01-17T01:11:56.000000Z" (always UTC).
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

register_source!(FenrirRealm, ListingProvider, Home, DeepLinkHandler);

#[cfg(test)]
mod tests;
