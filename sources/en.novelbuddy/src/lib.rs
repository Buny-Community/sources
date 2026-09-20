#![no_std]
use buny::{
	Chapter, ContentBlock, ContentRating, FilterValue, Listing, ListingProvider, Novel,
	NovelPageResult, NovelStatus, Result, Source,
	alloc::{String, Vec, string::ToString},
	helpers::uri::QueryParameters,
	imports::{
		html::Html,
		net::Request,
		std::{parse_date, send_partial_result},
	},
	prelude::*,
};
use core::cell::RefCell;
use serde_json::Value;

const BASE_URL: &str = "https://novelbuddy.me";
const API_URL: &str = "https://api.novelbuddy.me";

const DATE_FORMAT: &str = "yyyy-MM-dd HH:mm:ss";

// Sort values accepted by the /titles/search API, in the order of the sort filter options.
// The API also advertises "added_date", but it returns a 500, so no option maps to it.
const SORT_VALUES: [&str; 11] = [
	"",
	"latest",
	"popular",
	"rating",
	"views",
	"bookmarks",
	"chapters",
	"newest",
	"views_today",
	"views_7days",
	"views_30days",
];

struct NovelBuddy {
	// The Next.js build id is required for the /_next/data JSON routes and changes
	// on every site deploy, so it is fetched lazily and refreshed when a request fails.
	build_id: RefCell<Option<String>>,
}

impl NovelBuddy {
	fn request(url: &str) -> Result<Request> {
		Ok(Request::get(url)?
			.header("Accept", "application/json, text/plain, */*")
			.header("Origin", BASE_URL)
			.header("Referer", &format!("{BASE_URL}/")))
	}

	fn get_json(url: &str) -> Result<Value> {
		Self::request(url)?.json_owned()
	}

	fn build_id(&self) -> Result<String> {
		if let Some(id) = self.build_id.borrow().as_ref() {
			return Ok(id.clone());
		}
		let json = Self::get_json(&format!("{BASE_URL}/api/version"))?;
		let id = json["buildId"]
			.as_str()
			.filter(|s| !s.is_empty())
			.ok_or(error!("Missing buildId"))?
			.to_string();
		*self.build_id.borrow_mut() = Some(id.clone());
		Ok(id)
	}

	fn next_data_url(&self, path: &str, page: i32, slug: Option<&str>) -> Result<String> {
		let path = path.trim_start_matches('/');
		let mut qs = QueryParameters::new();
		if page > 0 {
			qs.push("page", Some(&page.to_string()));
		}
		if let Some(slug) = slug {
			qs.push("slug", Some(slug));
		}
		if let Some(kind) = path.strip_prefix("top/") {
			qs.push("type", Some(kind));
		}
		let mut url = format!("{}/_next/data/{}/{}.json", BASE_URL, self.build_id()?, path);
		let qs = qs.to_string();
		if !qs.is_empty() {
			url.push('?');
			url.push_str(&qs);
		}
		Ok(url)
	}

	// Fetches the Next.js page props JSON for a site path, retrying once with a
	// fresh build id when the cached one has gone stale.
	//
	// A stale build id 404s with an empty `{}` body, so a missing "pageProps" is the
	// staleness signal. A path that does not exist still returns "pageProps", carrying
	// an "httpError" instead, and must not trigger a retry.
	fn next_data(&self, path: &str, page: i32, slug: Option<&str>) -> Result<Value> {
		let url = self.next_data_url(path, page, slug)?;
		if let Ok(json) = Self::get_json(&url)
			&& json.get("pageProps").is_some()
		{
			return check_page_error(json);
		}

		*self.build_id.borrow_mut() = None;
		let url = self.next_data_url(path, page, slug)?;
		let json = Self::get_json(&url)?;
		if json.get("pageProps").is_none() {
			bail!("Invalid page data");
		}
		check_page_error(json)
	}

	fn search(qs: &QueryParameters) -> Result<NovelPageResult> {
		let json = Self::get_json(&format!("{API_URL}/titles/search?{qs}"))?;
		// A rejected query still parses as JSON, so an unchecked read would silently
		// yield an empty result list instead of an error.
		if !json["success"].as_bool().unwrap_or(false) {
			bail!(
				"{}",
				json["message"].as_str().unwrap_or("Search request failed")
			);
		}
		let data = &json["data"];
		Ok(NovelPageResult {
			entries: parse_novel_items(data["items"].as_array()),
			has_next_page: data["pagination"]["has_next"].as_bool().unwrap_or(false),
		})
	}
}

impl Source for NovelBuddy {
	fn new() -> Self {
		Self {
			build_id: RefCell::new(None),
		}
	}

	fn get_search_novel_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<NovelPageResult> {
		let mut qs = QueryParameters::new();
		qs.push("page", Some(&page.max(1).to_string()));
		qs.push("status", Some("all"));
		if let Some(q) = query.as_deref().map(str::trim)
			&& !q.is_empty()
		{
			qs.push("q", Some(q));
		}

		for filter in filters {
			match filter {
				FilterValue::Sort { index, .. } => {
					if let Some(value) = SORT_VALUES.get(index as usize)
						&& !value.is_empty()
					{
						qs.push("sort", Some(value));
					}
				}
				FilterValue::Select { id, value } => {
					if id == "status" && value != "all" {
						qs.set("status", Some(&value));
					}
				}
				// The API rejects repeated genre params, so each must be a single
				// comma-joined value.
				FilterValue::MultiSelect {
					id,
					included,
					excluded,
				} if id == "genres" => {
					if !included.is_empty() {
						qs.push("genres", Some(&included.join(",")));
					}
					if !excluded.is_empty() {
						qs.push("excludeGenres", Some(&excluded.join(",")));
					}
				}
				_ => {}
			}
		}

		Self::search(&qs)
	}

	fn get_novel_update(
		&self,
		mut novel: Novel,
		needs_details: bool,
		needs_chapters: bool,
		_page: i32,
	) -> Result<Novel> {
		let json = self.next_data(&novel.key, 0, Some(&novel.key))?;
		let page_props = &json["pageProps"];
		let manga = page_props
			.get("initialManga")
			.filter(|m| m.is_object())
			.ok_or(error!("Missing novel payload"))?;

		if needs_details {
			if let Some(name) = manga["name"].as_str() {
				novel.title = name.to_string();
			}
			novel.cover = manga["cover"].as_str().map(str::to_string);
			novel.url = Some(format!("{BASE_URL}/{}", novel.key));
			novel.description = manga["summary"]
				.as_str()
				.map(|summary| html_paragraphs(summary).join("\n\n"));
			novel.authors = manga["authors"].as_array().map(|authors| {
				authors
					.iter()
					.filter_map(|author| author["name"].as_str())
					.map(str::to_string)
					.collect()
			});

			// Genres are the broad categories shown in the filter; "tags" are the finer
			// descriptors (Magic, System, Weak to Strong). Both are useful, and the
			// payload lists some of them twice in different casing.
			let mut tags: Vec<String> = Vec::new();
			for entry in ["genres", "tags"]
				.iter()
				.flat_map(|key| manga[*key].as_array().into_iter().flatten())
			{
				let name = entry["name"].as_str().unwrap_or("").trim();
				if !name.is_empty() && !tags.iter().any(|t| t.eq_ignore_ascii_case(name)) {
					tags.push(name.to_string());
				}
			}

			novel.status = match manga["status"]
				.as_str()
				.unwrap_or("")
				.to_lowercase()
				.as_str()
			{
				"ongoing" => NovelStatus::Ongoing,
				"completed" => NovelStatus::Completed,
				"on-hold" | "hiatus" => NovelStatus::Hiatus,
				"canceled" | "cancelled" | "dropped" => NovelStatus::Cancelled,
				_ => NovelStatus::Unknown,
			};
			let has_tag = |name: &str| tags.iter().any(|t| t.eq_ignore_ascii_case(name));
			novel.content_rating = if manga["isAdult"].as_bool().unwrap_or(false)
				|| ["Adult", "Mature", "Smut"].iter().any(|t| has_tag(t))
			{
				ContentRating::NSFW
			} else if has_tag("Ecchi") {
				ContentRating::Suggestive
			} else {
				ContentRating::Safe
			};
			novel.tags = Some(tags);

			if needs_chapters {
				send_partial_result(&novel);
			}
		}

		if needs_chapters {
			let id = page_props["mangaHsid"]
				.as_str()
				.or_else(|| manga["id"].as_str())
				.filter(|s| !s.is_empty())
				.ok_or(error!("Missing novel id"))?;
			let mut url = format!("{API_URL}/titles/{id}/chapters");
			if let Some(cv) = manga["cv"].as_i64() {
				url.push_str(&format!("?cv={cv}"));
			}

			let json = Self::get_json(&url)?;
			let raw = json["data"]["chapters"]
				.as_array()
				.ok_or(error!("Invalid chapters array"))?;

			// The API lists newest first; the reader expects the first chapter first.
			let chapters: Vec<Chapter> = raw
				.iter()
				.rev()
				.filter_map(|ch| {
					let path = ch["url"].as_str()?.trim_start_matches('/');
					if path.is_empty() {
						return None;
					}
					let title = ch["name"]
						.as_str()
						.and_then(Html::unescape)
						.or_else(|| ch["name"].as_str().map(str::to_string));
					Some(Chapter {
						key: path.to_string(),
						title,
						chapter_number: ch["number"].as_f64().map(|n| n as f32),
						date_uploaded: ch["updated_at"].as_str().and_then(parse_timestamp),
						url: Some(format!("{BASE_URL}/{path}")),
						..Default::default()
					})
				})
				.collect();

			novel.chapters = Some(chapters);
			novel.has_more_chapters = Some(false);
		}

		Ok(novel)
	}

	fn get_chapter_content_list(
		&self,
		_novel: Novel,
		chapter: Chapter,
	) -> Result<Vec<ContentBlock>> {
		let json = self.next_data(&chapter.key, 0, None)?;
		let content = json["pageProps"]["initialChapter"]["content"]
			.as_str()
			.ok_or(error!("Missing chapter payload"))?;

		Ok(html_paragraphs(content)
			.into_iter()
			.map(|text| {
				if text == "***" {
					ContentBlock::Divider
				} else {
					ContentBlock::paragraph(text, None)
				}
			})
			.collect())
	}
}

impl ListingProvider for NovelBuddy {
	fn get_novel_list(&self, listing: Listing, page: i32) -> Result<NovelPageResult> {
		let page = page.max(1);

		// Completed novels have no site page of their own; use the search API.
		if listing.id == "completed" {
			let mut qs = QueryParameters::new();
			qs.push("status", Some("completed"));
			qs.push("page", Some(&page.to_string()));
			qs.push("limit", Some("24"));
			return Self::search(&qs);
		}

		// The site redirects /trending to /trending/manga.
		let path = match listing.id.as_str() {
			"trending" => "trending/manga",
			id => id,
		};
		let json = self.next_data(path, page, None)?;
		let page_props = &json["pageProps"];

		let items = ["items", "initialItems", "todayItems"]
			.iter()
			.find_map(|key| page_props[*key].as_array())
			.or_else(|| page_props["initialData"]["items"].as_array());

		// A listing can surface the same novel more than once (a title updated twice
		// within the window), and the repeats are not necessarily adjacent.
		let mut seen: Vec<String> = Vec::new();
		let mut entries = parse_novel_items(items);
		entries.retain(|novel| {
			if seen.contains(&novel.key) {
				return false;
			}
			seen.push(novel.key.clone());
			true
		});

		// /trending serves a fixed set with no pagination block, so it stays on one page.
		let has_next_page = ["pagination", "initialPagination"]
			.iter()
			.any(|key| page_props[*key]["has_next"].as_bool().unwrap_or(false));

		Ok(NovelPageResult {
			entries,
			has_next_page,
		})
	}
}

// A path the site does not serve still answers with page props, carrying an
// "httpError" object instead of the page's data. Surface its message rather than
// letting the caller report a generic missing-payload error.
fn check_page_error(json: Value) -> Result<Value> {
	let error = &json["pageProps"]["httpError"];
	if error.is_object() {
		bail!("{}", error["message"].as_str().unwrap_or("Page not found"));
	}
	Ok(json)
}

// Listing and search entries share the same shape; some listings wrap the
// novel inside a "title" or "manga" object.
fn parse_novel_items(items: Option<&Vec<Value>>) -> Vec<Novel> {
	items
		.into_iter()
		.flatten()
		.filter_map(|item| {
			let item = ["title", "manga"]
				.iter()
				.find_map(|key| item.get(*key).filter(|v| v.is_object()))
				.unwrap_or(item);
			let key = item["url"].as_str()?.trim_start_matches('/');
			if key.is_empty() {
				return None;
			}
			Some(Novel {
				key: key.to_string(),
				title: item["name"].as_str().unwrap_or("").to_string(),
				cover: item["cover"].as_str().map(str::to_string),
				..Default::default()
			})
		})
		.collect()
}

// Splits an HTML fragment into plain-text paragraphs. Paragraphs arrive either as
// <p> blocks or separated by runs of <br/>, and a chapter can mix both.
//
// Chapter content is also peppered with empty <div></div> elements. They are not
// scene breaks — chapters mark those with a literal "***" paragraph, and the empty
// divs appear mid-dialogue — so they collapse to blank lines and get dropped.
fn html_paragraphs(html: &str) -> Vec<String> {
	let mut text = String::with_capacity(html.len());
	let mut rest = html;
	while let Some(start) = rest.find('<') {
		text.push_str(&rest[..start]);
		let Some(end) = rest[start..].find('>') else {
			break;
		};
		let tag = rest[start + 1..start + end].trim().to_lowercase();
		if tag.starts_with("br")
			|| tag.starts_with("/br")
			|| tag.starts_with("/p")
			|| tag.starts_with("/div")
		{
			text.push('\n');
		}
		rest = &rest[start + end + 1..];
	}
	text.push_str(rest);

	text.split('\n')
		.map(str::trim)
		.filter(|line| !line.is_empty())
		.map(|line| Html::unescape(line).unwrap_or_else(|| line.to_string()))
		.map(|line| line.trim().to_string())
		.filter(|line| !line.is_empty())
		.collect()
}

// Parses an ISO 8601 UTC timestamp such as "2023-02-22T05:05:14.000Z".
fn parse_timestamp(value: &str) -> Option<i64> {
	let value = value
		.split('.')
		.next()?
		.trim_end_matches('Z')
		.replace('T', " ");
	parse_date(value, DATE_FORMAT)
}

register_source!(NovelBuddy, ListingProvider);

#[cfg(test)]
mod test {
	use super::*;
	use buny_test::buny_test;

	#[buny_test]
	fn test_search_and_novel() {
		let source = NovelBuddy::new();
		let result = source
			.get_search_novel_list(Some("shadow slave".into()), 1, Vec::new())
			.unwrap();
		assert!(!result.entries.is_empty());
		let novel = source
			.get_novel_update(result.entries[0].clone(), true, true, 1)
			.unwrap();
		println!(
			"{} {:?} {:?} {:?}",
			novel.title, novel.authors, novel.tags, novel.status
		);
		assert_eq!(novel.title, "Shadow Slave");
		let chapters = novel.chapters.unwrap();
		assert!(chapters.len() > 3000);
		assert_eq!(chapters[0].chapter_number, Some(1.0));
		assert!(chapters[0].date_uploaded.is_some());
		let content = source
			.get_chapter_content_list(Novel::default(), chapters[0].clone())
			.unwrap();
		println!("{:?}", &content[..2]);
		assert!(content.len() > 10);
	}

	#[buny_test]
	fn test_listings() {
		let source = NovelBuddy::new();
		for id in [
			"latest",
			"popular",
			"trending",
			"newest",
			"completed",
			"top/month",
		] {
			let listing = Listing {
				id: id.into(),
				..Default::default()
			};
			let result = source.get_novel_list(listing, 1).unwrap();
			println!(
				"{id}: {} entries, next={}",
				result.entries.len(),
				result.has_next_page
			);
			assert!(!result.entries.is_empty(), "{id}");
		}
	}
}
