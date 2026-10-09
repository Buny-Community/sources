#![no_std]
use buny::{
	Chapter, ContentBlock, ContentRating, FilterValue, Novel, NovelPageResult, NovelStatus, Result,
	Source, UpdateStrategy,
	alloc::{String, Vec, string::ToString, vec},
	helpers::uri::QueryParameters,
	imports::net::Request,
	prelude::*,
};
use chapter_numbers::{chapter_numbers, chapter_title};

pub(crate) mod model;
use model::{ChapterResponse, NovelDetailResponse, NovelListResponse, NovelSummary};

pub mod traits;

pub(crate) struct NovelArchive;

pub(crate) const BASE_URL: &str = "https://novelarchive.cc";
pub(crate) const API_BASE: &str = "https://novelarchive.cc/api";

const SORT_IDS: [&str; 4] = ["recent", "popular", "rating", "chapters"];

fn parse_status(status: Option<&str>) -> NovelStatus {
	match status {
		Some("ongoing") => NovelStatus::Ongoing,
		Some("completed") => NovelStatus::Completed,
		Some("hiatus") => NovelStatus::Hiatus,
		_ => NovelStatus::Unknown,
	}
}

// completed novels won't get new chapters, so exclude them from automatic
// library refreshes
fn update_strategy_for(status: NovelStatus) -> UpdateStrategy {
	if status == NovelStatus::Completed {
		UpdateStrategy::Never
	} else {
		UpdateStrategy::Always
	}
}

fn parse_tags(genres: Option<String>) -> Option<Vec<String>> {
	genres.map(|g| {
		g.split(',')
			.map(|t| t.trim().to_string())
			.filter(|t| !t.is_empty())
			.collect()
	})
}

fn content_rating_from_tags(tags: &Option<Vec<String>>) -> ContentRating {
	let Some(tags) = tags else {
		return ContentRating::Unknown;
	};
	let lower: Vec<String> = tags.iter().map(|t| t.to_lowercase()).collect();
	const NSFW_TAGS: [&str; 14] = [
		"adult",
		"erotica",
		"explicit sex",
		"smut",
		"rape",
		"nsfw",
		"bdsm",
		"hypnosis",
		"mind break",
		"ntr",
		"prostitution",
		"sex slavery",
		"sex work",
		"slave heroine",
	];
	const SUGGESTIVE_TAGS: [&str; 3] = ["ecchi", "mature", "yandere"];
	if lower.iter().any(|t| NSFW_TAGS.contains(&t.as_str())) {
		ContentRating::NSFW
	} else if lower.iter().any(|t| SUGGESTIVE_TAGS.contains(&t.as_str())) {
		ContentRating::Suggestive
	} else {
		ContentRating::Safe
	}
}

fn absolute_cover(cover: Option<String>) -> Option<String> {
	cover.map(|c| {
		if c.starts_with("http") {
			c
		} else {
			format!("{}{}", BASE_URL, c)
		}
	})
}

pub(crate) fn novel_summary_to_novel(item: NovelSummary) -> Novel {
	let tags = parse_tags(item.genres);
	let status = parse_status(item.release_status.as_deref());
	let content_rating = content_rating_from_tags(&tags);
	Novel {
		key: item.id.clone(),
		title: item.title,
		cover: absolute_cover(item.cover_url),
		authors: item.author.map(|a| vec![a]),
		status,
		content_rating,
		tags,
		url: Some(format!("{}/novel?id={}", BASE_URL, item.id)),
		update_strategy: update_strategy_for(status),
		..Default::default()
	}
}

impl Source for NovelArchive {
	// this method is called once when the source is initialized
	// perform any necessary setup here
	fn new() -> Self {
		Self
	}

	// this method will be called first without a query when the search page is opened,
	// then when a search query is entered or filters are changed
	fn get_search_novel_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<NovelPageResult> {
		let mut qs = QueryParameters::new();
		if let Some(query) = query.as_ref()
			&& !query.is_empty()
		{
			qs.push("search", Some(query));
		}
		qs.push("page", Some(&page.to_string()));
		qs.push("per_page", Some("20"));

		for filter in filters {
			match filter {
				FilterValue::Text { value, .. } => {
					if !value.is_empty() {
						qs.push("search", Some(&value));
					}
				}
				FilterValue::Sort { index, .. } => {
					if let Some(sort_id) = SORT_IDS.get(index as usize) {
						qs.push("sort", Some(sort_id));
					}
				}
				FilterValue::Select { id, value } => match id.as_str() {
					"status" => qs.push("status", Some(&value)),
					"ai_generated" => qs.push("ai_generated", Some(&value)),
					_ => {}
				},
				FilterValue::MultiSelect {
					id,
					included,
					excluded,
				} if id == "genre" => {
					if !included.is_empty() {
						qs.push("genres_include", Some(&included.join(",")));
					}
					if !excluded.is_empty() {
						qs.push("genres_exclude", Some(&excluded.join(",")));
					}
				}
				_ => {}
			}
		}

		let url = format!("{}/novels?{}", API_BASE, qs);
		let response: NovelListResponse = Request::get(&url)?.json_owned()?;

		let entries = response
			.novels
			.into_iter()
			.map(novel_summary_to_novel)
			.collect();

		let has_next_page = response.pagination.map(|p| p.has_next).unwrap_or(false);

		Ok(NovelPageResult {
			entries,
			has_next_page,
		})
	}

	// this method will be called when a novel page is opened
	fn get_novel_update(
		&self,
		mut novel: Novel,
		needs_details: bool,
		needs_chapters: bool,
		_page: i32,
	) -> Result<Novel> {
		if needs_details || needs_chapters {
			let url = format!("{}/novels/{}", API_BASE, novel.key);
			let response: NovelDetailResponse = Request::get(&url)?.json_owned()?;
			let detail = response.novel;

			if needs_details {
				let tags = parse_tags(detail.genres);
				novel.title = detail.title;
				novel.cover = absolute_cover(detail.cover_url);
				novel.authors = detail.author.map(|a| vec![a]);
				novel.description = detail.description.map(|d| {
					d.trim()
						.strip_suffix("Collapse")
						.unwrap_or(&d)
						.trim()
						.to_string()
				});
				novel.status = parse_status(detail.release_status.as_deref());
				novel.content_rating = content_rating_from_tags(&tags);
				novel.tags = tags;
				novel.url = Some(format!("{}/novel?id={}", BASE_URL, novel.key));
				novel.update_strategy = update_strategy_for(novel.status);
			}

			if needs_chapters {
				let names: Vec<&str> = detail.chapter_names.iter().map(String::as_str).collect();
				let numbers = chapter_numbers(&names);
				// The site addresses chapters by their 1-based position, so that's
				// the key. The number comes from the names: a list can start at
				// "Chapter 0".
				let chapters: Vec<Chapter> = names
					.iter()
					.zip(numbers)
					.enumerate()
					.map(|(i, (name, number))| Chapter {
						key: (i + 1).to_string(),
						title: chapter_title(name, number),
						chapter_number: Some(number),
						url: Some(format!(
							"{}/reader?novel={}&chapter={}",
							BASE_URL,
							novel.key,
							i + 1
						)),
						..Default::default()
					})
					.collect();
				novel.chapters = Some(chapters);
				novel.has_more_chapters = Some(false);
			}
		}
		Ok(novel)
	}

	fn get_chapter_content_list(
		&self,
		novel: Novel,
		chapter: Chapter,
	) -> Result<Vec<ContentBlock>> {
		let url = format!("{}/novels/{}/chapters/{}", API_BASE, novel.key, chapter.key);
		let response: ChapterResponse = Request::get(&url)?.json_owned()?;

		let content_list: Vec<ContentBlock> = response
			.chapter
			.content
			.split('\n')
			.filter(|p| !p.trim().is_empty())
			.map(|p| {
				let p = p.trim();
				// System-window messages such as "[You have slain a dormant beast.]".
				if p.len() > 2 && p.starts_with('[') && p.ends_with(']') {
					ContentBlock::block_quote(p)
				} else {
					ContentBlock::paragraph(p, None)
				}
			})
			.collect();

		Ok(content_list)
	}
}

register_source!(NovelArchive, ListingProvider, Home);

#[cfg(test)]
mod test {
	use super::*;
	use buny::{Home, HomeComponentValue};
	use buny_test::buny_test;

	#[buny_test]
	fn test_chapter_numbers() {
		// "A Knight Who Eternally Regresses" starts at "Chapter 0: Prologue",
		// and labels chapter 119 "Chapter 118: Leap".
		let novel = NovelArchive::new()
			.get_novel_update(
				Novel {
					key: "69fede66a5f4c7d1b734e190".into(),
					..Default::default()
				},
				true,
				true,
				1,
			)
			.unwrap();
		let chapters = novel.chapters.unwrap();
		println!("{} chapters", chapters.len());
		assert!(chapters.len() > 950);
		let first = &chapters[0];
		assert_eq!(first.key, "1");
		assert_eq!(first.chapter_number, Some(0.0));
		assert_eq!(first.title.as_deref(), Some("Prologue"));
		assert_eq!(
			first.url.as_deref(),
			Some("https://novelarchive.cc/reader?novel=69fede66a5f4c7d1b734e190&chapter=1")
		);
		assert_eq!(chapters[1].chapter_number, Some(1.0));
		assert_eq!(
			chapters[1].title.as_deref(),
			Some("My Dream was to be a Knight")
		);
		assert_eq!(chapters[119].chapter_number, Some(119.0));
		assert_eq!(chapters[119].title.as_deref(), Some("Chapter 118: Leap"));
		assert_eq!(chapters[120].chapter_number, Some(120.0));
		assert!(
			chapters
				.windows(2)
				.all(|w| w[0].chapter_number < w[1].chapter_number)
		);

		// The key still opens the chapter at that position.
		let blocks = NovelArchive::new()
			.get_chapter_content_list(
				Novel {
					key: "69fede66a5f4c7d1b734e190".into(),
					..Default::default()
				},
				first.clone(),
			)
			.unwrap();
		println!("{} blocks", blocks.len());
		assert!(blocks.len() > 10);
	}

	#[buny_test]
	fn test_banner() {
		// Shadow Slave chapter 5 opens with a system message.
		let blocks = NovelArchive::new()
			.get_chapter_content_list(
				Novel {
					key: "69faa859a5f4c7d1b734d496".into(),
					..Default::default()
				},
				Chapter {
					key: "5".into(),
					..Default::default()
				},
			)
			.unwrap();
		assert!(
			matches!(&blocks[0], ContentBlock::BlockQuote(t) if t.starts_with("[You have slain"))
		);
	}

	#[buny_test]
	fn test_home() {
		let home = NovelArchive::new().get_home().unwrap();
		for c in &home.components {
			let count = match &c.value {
				HomeComponentValue::Details { entries, .. }
				| HomeComponentValue::Scroller { entries, .. }
				| HomeComponentValue::Stack { entries, .. }
				| HomeComponentValue::Vertical { entries, .. } => entries.len(),
				_ => 0,
			};
			println!("{:?}: {count}", c.title);
			// The Vertical grid is sent empty; the app loads it itself.
			if !matches!(c.value, HomeComponentValue::Vertical { .. }) {
				assert!(count > 0, "{:?}", c.title);
			}
		}
		assert_eq!(home.components.len(), 4);
	}
}
