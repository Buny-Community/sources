#![no_std]
use buny::{
	Chapter, ContentBlock, ContentRating, FilterValue, Novel, NovelPageResult, NovelStatus, Result,
	Source,
	alloc::{String, Vec, string::ToString},
	helpers::uri::QueryParameters,
	imports::{
		html::Html,
		net::Request,
		std::{parse_date, send_partial_result},
	},
	prelude::*,
};
use serde_json::Value;

mod traits;

const BASE_URL: &str = "https://chikari.moe";
const API_URL: &str = "https://chikari.moe/api";

// The API silently caps larger limits to these values.
const NOVEL_PAGE_SIZE: i32 = 60;
const CHAPTER_PAGE_SIZE: i32 = 500;

// Listing ids of the form "list:<id>" open a user-curated list (/api/lists/<id>).
const USER_LIST_PREFIX: &str = "list:";
// Popular completed novels; every other listing id is an /api/novels sort value.
const COMPLETED_LISTING: &str = "completed";

// Sort values accepted by /api/novels, in the order of the sort filter options.
// An unknown sort value falls back to "updated" rather than erroring.
const SORT_VALUES: [&str; 6] = [
	"popular",
	"trending",
	"top_rated",
	"updated",
	"added",
	"most_bookmarked",
];

struct Chikari;

impl Chikari {
	// Errors come back as JSON too (`{"detail": "Not Found"}` for a 404,
	// `{"detail": [{"msg": ...}]}` for a rejected query param), so a bare parse
	// would hand callers an object with none of the expected fields.
	fn get_json(url: &str) -> Result<Value> {
		let json: Value = Request::get(url)?
			.header("Accept", "application/json")
			.header("Referer", &format!("{BASE_URL}/"))
			.json_owned()?;
		match &json["detail"] {
			Value::String(msg) => Err(error!("{msg}")),
			Value::Array(errors) => Err(error!(
				"{}",
				errors
					.first()
					.and_then(|e| e["msg"].as_str())
					.unwrap_or("Request rejected")
			)),
			_ => Ok(json),
		}
	}

	fn novel_list(qs: &mut QueryParameters, page: i32) -> Result<NovelPageResult> {
		let offset = (page.max(1) - 1) * NOVEL_PAGE_SIZE;
		qs.set("limit", Some(&NOVEL_PAGE_SIZE.to_string()));
		qs.set("offset", Some(&offset.to_string()));

		let json = Self::get_json(&format!("{API_URL}/novels?{qs}"))?;
		let items = json["items"]
			.as_array()
			.ok_or(error!("Invalid novel list"))?;
		let total = json["total"].as_i64().unwrap_or(0);

		Ok(NovelPageResult {
			entries: items.iter().filter_map(parse_novel_item).collect(),
			has_next_page: (offset as i64) + (items.len() as i64) < total,
		})
	}

	fn listing(id: &str, page: i32) -> Result<NovelPageResult> {
		if let Some(list_id) = id.strip_prefix(USER_LIST_PREFIX) {
			return Self::user_list(list_id, page);
		}
		let mut qs = QueryParameters::new();
		if id == COMPLETED_LISTING {
			qs.push("sort", Some("popular"));
			qs.push("status", Some("completed"));
		} else {
			qs.push("sort", Some(id));
		}
		Self::novel_list(&mut qs, page)
	}

	// A user list returns every item in one response, so there is only one page.
	// Lists can mix manga into a novel list; those have an empty `medium` and
	// their slugs belong to /series, not /novels, so they are dropped.
	fn user_list(id: &str, page: i32) -> Result<NovelPageResult> {
		if page > 1 {
			return Ok(NovelPageResult::default());
		}
		let json = Self::get_json(&format!("{API_URL}/lists/{id}"))?;
		let items = json["items"]
			.as_array()
			.ok_or(error!("Invalid user list"))?;
		Ok(NovelPageResult {
			entries: items
				.iter()
				.filter(|item| item["medium"].as_str() == Some("novel"))
				.filter_map(parse_novel_item)
				.collect(),
			has_next_page: false,
		})
	}
}

impl Source for Chikari {
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
		qs.push("sort", Some(SORT_VALUES[0]));
		if let Some(q) = query.as_deref().map(str::trim)
			&& !q.is_empty()
		{
			qs.push("q", Some(q));
		}

		for filter in filters {
			match filter {
				FilterValue::Sort { index, .. } => {
					if let Some(value) = SORT_VALUES.get(index as usize) {
						qs.set("sort", Some(value));
					}
				}
				FilterValue::Select { id, value } => match id.as_str() {
					"status" if value != "all" => qs.push("status", Some(&value)),
					// adult=true returns only NSFW novels; omitting it returns only SFW ones.
					// There is no value that returns both.
					"content" if value == "adult" => qs.push("adult", Some("true")),
					_ => {}
				},
				// Genres are repeated params; a comma-joined value matches nothing.
				FilterValue::MultiSelect {
					id,
					included,
					excluded,
				} if id == "genres" => {
					for genre in &included {
						qs.push("genre", Some(genre));
					}
					for genre in &excluded {
						qs.push("genre_exclude", Some(genre));
					}
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
		if needs_details {
			let json = Self::get_json(&format!("{API_URL}/novels/{}", novel.key))?;

			if let Some(title) = json["title"].as_str() {
				novel.title = title.to_string();
			}
			novel.cover = json["cover_url"]
				.as_str()
				.filter(|s| !s.is_empty())
				.map(str::to_string);
			novel.url = Some(format!("{BASE_URL}/novels/{}", novel.key));
			novel.description = json["description"].as_str().map(|desc| {
				desc.replace("\r\n", "\n")
					.split("\n\n")
					.map(str::trim)
					.filter(|p| !p.is_empty())
					.collect::<Vec<_>>()
					.join("\n\n")
			});
			novel.authors = json["authors"].as_array().map(|authors| {
				authors
					.iter()
					.filter_map(|author| author["name"].as_str())
					.map(str::to_string)
					.collect()
			});

			// Genres are the broad categories from the filter; tags are the finer
			// descriptors. Tags flagged as spoilers are left out.
			let genres = json["genres"].as_array().into_iter().flatten();
			let tags = json["tags"]
				.as_array()
				.into_iter()
				.flatten()
				.filter(|tag| !tag["is_spoiler"].as_bool().unwrap_or(false));
			let mut all_tags: Vec<String> = Vec::new();
			for entry in genres.chain(tags) {
				let name = entry["name"].as_str().unwrap_or("").trim();
				if !name.is_empty() && !all_tags.iter().any(|t| t.eq_ignore_ascii_case(name)) {
					all_tags.push(name.to_string());
				}
			}

			novel.status = parse_status(json["status"].as_str());
			let has_genre = |slug: &str| {
				json["genres"]
					.as_array()
					.is_some_and(|g| g.iter().any(|g| g["slug"].as_str() == Some(slug)))
			};
			novel.content_rating = if json["is_nsfw"].as_bool().unwrap_or(false) {
				ContentRating::NSFW
			} else if has_genre("mature") || has_genre("ecchi") {
				ContentRating::Suggestive
			} else {
				ContentRating::Safe
			};
			novel.tags = Some(all_tags);

			if needs_chapters {
				send_partial_result(&novel);
			}
		}

		if needs_chapters {
			let offset = (page.max(1) - 1) * CHAPTER_PAGE_SIZE;
			let json = Self::get_json(&format!(
				"{API_URL}/novels/{}/chapters?order=asc&limit={CHAPTER_PAGE_SIZE}&offset={offset}",
				novel.key
			))?;
			let items = json["items"]
				.as_array()
				.ok_or(error!("Invalid chapter list"))?;
			let total = json["total"].as_i64().unwrap_or(0);

			let chapters = items
				.iter()
				.filter_map(|ch| {
					let number = ch["number"].as_f64()?;
					let key = format_number(number);
					Some(Chapter {
						title: ch["title"].as_str().and_then(chapter_title),
						chapter_number: Some(number as f32),
						volume_number: ch["volume"].as_str().and_then(|v| v.trim().parse().ok()),
						date_uploaded: ch["created_at"].as_str().and_then(parse_timestamp),
						url: Some(format!("{BASE_URL}/novels/{}/{key}", novel.key)),
						language: ch["lang"].as_str().map(str::to_string),
						key,
						..Default::default()
					})
				})
				.collect();

			novel.chapters = Some(chapters);
			novel.has_more_chapters = Some((offset as i64) + (items.len() as i64) < total);
		}

		Ok(novel)
	}

	fn get_chapter_content_list(
		&self,
		novel: Novel,
		chapter: Chapter,
	) -> Result<Vec<ContentBlock>> {
		let json = Self::get_json(&format!(
			"{API_URL}/novels/{}/chapters/{}/read",
			novel.key, chapter.key
		))?;

		// Early-access chapters come back with an empty body and a lock reason.
		if json["locked"].as_bool().unwrap_or(false) {
			let reason = json["lock_reason"].as_str().unwrap_or("").replace('_', " ");
			if reason.is_empty() {
				bail!("Chapter is locked");
			}
			bail!("Chapter is locked ({reason})");
		}

		let body = json["body"]
			.as_str()
			.ok_or(error!("Missing chapter body"))?;
		Ok(body_paragraphs(body)
			.into_iter()
			.map(|text| {
				if is_scene_break(&text) {
					ContentBlock::Divider
				} else if is_banner(&text) {
					ContentBlock::block_quote(text)
				} else {
					ContentBlock::paragraph(text, None)
				}
			})
			.collect())
	}
}

fn parse_novel_item(item: &Value) -> Option<Novel> {
	let key = item["slug"].as_str().filter(|s| !s.is_empty())?;
	Some(Novel {
		key: key.to_string(),
		title: item["title"].as_str().unwrap_or("").to_string(),
		cover: item["cover_url"]
			.as_str()
			.filter(|s| !s.is_empty())
			.map(str::to_string),
		url: Some(format!("{BASE_URL}/novels/{key}")),
		status: parse_status(item["status"].as_str()),
		content_rating: if item["is_nsfw"].as_bool().unwrap_or(false) {
			ContentRating::NSFW
		} else {
			ContentRating::Safe
		},
		..Default::default()
	})
}

// The API only reports these two; a handful of novels have an empty status.
fn parse_status(status: Option<&str>) -> NovelStatus {
	match status {
		Some("releasing") => NovelStatus::Ongoing,
		Some("completed") => NovelStatus::Completed,
		_ => NovelStatus::Unknown,
	}
}

// Chapter keys are the chapter number as the site writes it in URLs: "12", or
// "12.5" for an interstitial chapter. The read endpoint accepts either form.
fn format_number(number: f64) -> String {
	let whole = number as i64;
	if whole as f64 == number {
		format!("{whole}")
	} else {
		format!("{number}")
	}
}

// Titles repeat the chapter number in a few shapes: "Chapter 3198 Secret Technique",
// "Chapter 1 - 1: Nightmare Begins", or just "Chapter 12". The app shows the number
// separately, so strip it and keep only the name.
fn chapter_title(raw: &str) -> Option<String> {
	fn strip_number(s: &str) -> Option<&str> {
		let rest = s.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.');
		(rest.len() < s.len()).then(|| {
			rest.trim_start_matches(|c: char| {
				c.is_whitespace() || matches!(c, '-' | ':' | '–' | '.')
			})
		})
	}

	let title = raw.trim();
	let mut rest = title;
	if rest.len() >= 7 && rest[..7].eq_ignore_ascii_case("chapter") {
		match strip_number(rest[7..].trim_start()) {
			Some(r) => rest = r,
			None => return Some(title.to_string()),
		}
		// "Chapter 1 - 1: Name" repeats the number once more.
		if let Some(r) = strip_number(rest) {
			rest = r;
		}
	}
	let rest = rest.trim();
	(!rest.is_empty()).then(|| rest.to_string())
}

fn is_scene_break(text: &str) -> bool {
	let stripped: String = text.chars().filter(|c| !c.is_whitespace()).collect();
	stripped.len() >= 3 && stripped.chars().all(|c| matches!(c, '*' | '~' | '=' | '#'))
}

// System-window messages, e.g. "[You have slain a dormant beast.]".
fn is_banner(text: &str) -> bool {
	text.len() > 2 && text.starts_with('[') && text.ends_with(']')
}

// Chapter bodies are plain text with "\n"-separated paragraphs, sprinkled with a few
// inline HTML tags (<i>, <em>, <strong>, <br>). Those become markdown, since paragraph
// blocks are rendered as markdown. Anything else in angle brackets is left alone:
// stories use it as in-world markup, e.g. "<Dark Knight>" as a skill name.
fn body_paragraphs(body: &str) -> Vec<String> {
	let mut text = String::with_capacity(body.len());
	let mut rest = body;
	while let Some(start) = rest.find('<') {
		text.push_str(&rest[..start]);
		let tag_end = rest[start..].find('>').map(|end| start + end);
		let replacement = tag_end.and_then(|end| {
			let tag = rest[start + 1..end].trim().trim_end_matches('/').trim();
			match tag.to_ascii_lowercase().as_str() {
				"i" | "/i" | "em" | "/em" => Some("*"),
				"b" | "/b" | "strong" | "/strong" => Some("**"),
				"br" | "p" | "/p" => Some("\n"),
				_ => None,
			}
		});
		match (replacement, tag_end) {
			(Some(r), Some(end)) => {
				text.push_str(r);
				rest = &rest[end + 1..];
			}
			_ => {
				text.push('<');
				rest = &rest[start + 1..];
			}
		}
	}
	text.push_str(rest);

	text.split('\n')
		.map(str::trim)
		.filter(|line| !line.is_empty())
		.map(|line| Html::unescape(line).unwrap_or_else(|| line.to_string()))
		.collect()
}

// Parses a timestamp such as "2026-02-21T22:08:14.600092+00:00". The API always
// reports UTC, so the fraction and offset are dropped.
fn parse_timestamp(value: &str) -> Option<i64> {
	let value = value.get(..19)?.replace('T', " ");
	parse_date(value, "yyyy-MM-dd HH:mm:ss")
}

register_source!(Chikari, ListingProvider, Home, DeepLinkHandler);

#[cfg(test)]
mod test {
	use super::*;
	use buny::{
		DeepLinkHandler, DeepLinkResult, Home, HomeComponentValue, Listing, ListingProvider,
		alloc::vec,
	};
	use buny_test::buny_test;

	#[buny_test]
	fn test_search_and_novel() {
		let source = Chikari::new();
		let result = source
			.get_search_novel_list(Some("shadow slave".into()), 1, Vec::new())
			.unwrap();
		assert!(!result.entries.is_empty());
		let novel = source
			.get_novel_update(result.entries[0].clone(), true, true, 1)
			.unwrap();
		println!(
			"{} {:?} {:?} {:?}",
			novel.title, novel.authors, novel.status, novel.content_rating
		);
		assert_eq!(novel.title, "Shadow Slave");
		assert_eq!(novel.has_more_chapters, Some(true));
		let chapters = novel.chapters.clone().unwrap();
		assert_eq!(chapters.len(), CHAPTER_PAGE_SIZE as usize);
		assert_eq!(chapters[0].key, "1");
		assert_eq!(chapters[0].title.as_deref(), Some("Nightmare Begins"));
		assert!(chapters[0].date_uploaded.is_some());

		let page2 = source
			.get_novel_update(novel.clone(), false, true, 2)
			.unwrap();
		assert_eq!(page2.chapters.unwrap()[0].key, "501");

		let content = source
			.get_chapter_content_list(novel, chapters[0].clone())
			.unwrap();
		println!("{:?}", &content[..2]);
		assert!(content.len() > 10);
	}

	#[buny_test]
	fn test_filters() {
		let source = Chikari::new();
		let result = source
			.get_search_novel_list(
				None,
				1,
				vec![
					FilterValue::Sort {
						id: "sort".into(),
						index: 2,
						ascending: false,
					},
					FilterValue::MultiSelect {
						id: "genres".into(),
						included: vec!["action".into(), "romance".into()],
						excluded: vec!["harem".into()],
					},
					FilterValue::Select {
						id: "status".into(),
						value: "completed".into(),
					},
				],
			)
			.unwrap();
		assert!(!result.entries.is_empty());
		assert!(
			result
				.entries
				.iter()
				.all(|n| n.status == NovelStatus::Completed)
		);

		let adult = source
			.get_search_novel_list(
				None,
				1,
				vec![FilterValue::Select {
					id: "content".into(),
					value: "adult".into(),
				}],
			)
			.unwrap();
		assert!(
			adult
				.entries
				.iter()
				.all(|n| n.content_rating == ContentRating::NSFW)
		);
	}

	#[buny_test]
	fn test_listings() {
		let source = Chikari::new();
		for id in [
			"popular",
			"updated",
			"trending",
			"top_rated",
			"added",
			"completed",
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

	#[buny_test]
	fn test_user_list() {
		let source = Chikari::new();
		// List 4 mixes manga into a novel list; only the novels should come back.
		let listing = Listing {
			id: "list:4".into(),
			..Default::default()
		};
		let result = source.get_novel_list(listing.clone(), 1).unwrap();
		println!("list:4: {} novels", result.entries.len());
		assert!(!result.entries.is_empty());
		assert!(!result.has_next_page);
		assert!(
			result
				.entries
				.iter()
				.all(|n| n.cover.as_deref().is_none_or(|c| c.contains("/novels/")))
		);
		assert!(
			source
				.get_novel_list(listing, 2)
				.unwrap()
				.entries
				.is_empty()
		);

		let link = source
			.handle_deep_link("https://chikari.moe/lists/193-masterpieces-peak".into())
			.unwrap();
		match link {
			Some(DeepLinkResult::Listing(l)) => {
				assert_eq!(l.id, "list:193");
				// The title lookup is best-effort (falls back to "List"), so don't
				// fail on a transient error while the other live tests run.
				assert!(!l.name.is_empty());
			}
			other => panic!("unexpected deep link result: {other:?}"),
		}
	}

	#[buny_test]
	fn test_home() {
		let home = Chikari::new().get_home().unwrap();
		for c in &home.components {
			let count = match &c.value {
				HomeComponentValue::ImageScroller { links, .. } => links.len(),
				HomeComponentValue::Details { entries, .. }
				| HomeComponentValue::Scroller { entries, .. }
				| HomeComponentValue::Stack { entries, .. }
				| HomeComponentValue::Vertical { entries, .. } => entries.len(),
				_ => 0,
			};
			println!("{:?}: {count}", c.title);
		}
		assert_eq!(home.components.len(), 7);
		assert!(matches!(
			&home.components[0].value,
			HomeComponentValue::ImageScroller { links, .. } if !links.is_empty()
		));
	}

	#[buny_test]
	fn test_banner() {
		assert!(is_banner(
			"[You have slain a dormant beast, Mountain King's Larva.]"
		));
		assert!(!is_banner("[]"));
		assert!(!is_banner("[Sunny] nodded."));
		assert!(!is_banner("He read [the note]. Then left"));
	}

	#[buny_test]
	fn test_titles() {
		assert_eq!(
			chapter_title("Chapter 1 - 1: Nightmare Begins").as_deref(),
			Some("Nightmare Begins")
		);
		assert_eq!(
			chapter_title("Chapter 3198 Secret Technique").as_deref(),
			Some("Secret Technique")
		);
		assert_eq!(chapter_title("Chapter 12"), None);
		assert_eq!(
			chapter_title("Chapterhouse Dune").as_deref(),
			Some("Chapterhouse Dune")
		);
		assert_eq!(chapter_title("Prologue").as_deref(), Some("Prologue"));
	}
}
