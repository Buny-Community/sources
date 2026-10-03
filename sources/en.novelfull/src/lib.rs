#![no_std]
use buny::{
	Chapter, ContentBlock, ContentRating, FilterValue, Listing, ListingProvider, Novel,
	NovelPageResult, NovelStatus, Result, Source,
	alloc::{String, Vec, string::ToString},
	helpers::uri::encode_uri_component,
	imports::{html::Document, net::Request},
	prelude::*,
};
use chapter_numbers::{chapter_numbers, chapter_title};

mod home;

struct NovelFull;

const BASE_URL: &str = "https://novelfull.com";

fn parse_novel_list(html: &Document) -> Vec<Novel> {
	html.select(".col-truyen-main .list-truyen .row")
		.map(|els| {
			els.filter_map(|row| {
				let link = row.select_first("h3.truyen-title > a")?;
				let title = link.text()?.trim().to_string();
				let key = link.attr("href")?.trim_start_matches('/').to_string();

				let mut cover = row.select_first("img")?.attr("src")?.to_string();
				if !cover.starts_with("http") {
					cover = format!("{BASE_URL}/{}", cover.trim_start_matches('/'));
				}

				Some(Novel {
					key,
					title,
					cover: Some(cover),
					..Default::default()
				})
			})
			.collect()
		})
		.unwrap_or_default()
}

fn has_next_page(html: &Document, page: i32) -> bool {
	html.select_first(".pagination li:last-child a")
		.and_then(|el| el.attr("href"))
		.and_then(|href| href.rsplit("page=").next().map(|n| n.to_string()))
		.and_then(|n| n.parse::<i32>().ok())
		.is_some_and(|last_page| page < last_page)
}

fn status_from_text(text: &str) -> NovelStatus {
	let lower = text.to_lowercase();
	if lower.contains("completed") {
		NovelStatus::Completed
	} else if lower.contains("ongoing") {
		NovelStatus::Ongoing
	} else if lower.contains("hiatus") {
		NovelStatus::Hiatus
	} else if lower.contains("dropped") || lower.contains("cancelled") {
		NovelStatus::Cancelled
	} else {
		NovelStatus::Unknown
	}
}

impl Source for NovelFull {
	fn new() -> Self {
		Self
	}

	fn get_search_novel_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<NovelPageResult> {
		let url = if let Some(query) = query.filter(|q| !q.is_empty()) {
			format!(
				"{BASE_URL}/search?keyword={}&page={page}",
				encode_uri_component(&query)
			)
		} else {
			let mut genre: Option<String> = None;
			let mut sort_path = "most-popular".to_string();
			let sort_ids = [
				"latest-release-novel",
				"hot-novel",
				"completed-novel",
				"most-popular",
			];

			for filter in filters {
				match filter {
					FilterValue::Select { id, value } if id == "genre" && !value.is_empty() => {
						genre = Some(value);
					}
					FilterValue::Sort { id, index, .. } if id == "sort" => {
						if let Some(sort_id) = sort_ids.get(index as usize) {
							sort_path = sort_id.to_string();
						}
					}
					_ => {}
				}
			}

			match genre {
				Some(genre) => format!("{BASE_URL}/genre/{genre}?page={page}"),
				None => format!("{BASE_URL}/{sort_path}?page={page}"),
			}
		};

		let html = Request::get(&url)?.html()?;
		let entries = parse_novel_list(&html);
		let has_next_page = has_next_page(&html, page);

		Ok(NovelPageResult {
			entries,
			has_next_page,
		})
	}

	fn get_novel_update(
		&self,
		mut novel: Novel,
		needs_details: bool,
		needs_chapters: bool,
		_page: i32,
	) -> Result<Novel> {
		let url = format!("{BASE_URL}/{}", novel.key);
		let html = Request::get(&url)?.html()?;

		if needs_details {
			if let Some(img) = html.select_first("div.book > img") {
				if let Some(title) = img.attr("alt") {
					novel.title = title;
				}
				if let Some(mut cover) = img.attr("src") {
					if !cover.starts_with("http") {
						cover = format!("{BASE_URL}/{}", cover.trim_start_matches('/'));
					}
					novel.cover = Some(cover);
				}
			}

			// Join the paragraphs directly rather than via text_with_newlines(), which
			// returns None under buny-test-runner (its fragment parser has no <body>).
			novel.description = html
				.select("div.desc-text p")
				.map(|els| {
					els.filter_map(|p| p.text())
						.filter(|t| !t.is_empty())
						.collect::<Vec<_>>()
						.join("\n\n")
				})
				.filter(|d| !d.is_empty())
				.or_else(|| html.select_first("div.desc-text").and_then(|el| el.text()));

			// Each `.info > div` row is `<h3>Label:</h3>` followed by links. Match on the
			// label text: buny-test-runner's selector engine has no `:contains()`.
			for row in html.select(".info > div").into_iter().flatten() {
				let Some(label) = row.select_first("h3").and_then(|h| h.text()) else {
					continue;
				};
				let links: Vec<String> = row
					.select("a")
					.map(|els| {
						els.filter_map(|a| a.text())
							.map(|t| t.trim().to_string())
							.filter(|t| !t.is_empty())
							.collect()
					})
					.unwrap_or_default();
				match label.trim_end_matches(':').trim() {
					"Author" if !links.is_empty() => novel.authors = Some(links),
					"Status" => {
						novel.status = links
							.first()
							.map(|t| status_from_text(t))
							.unwrap_or(NovelStatus::Unknown);
					}
					"Genre" if !links.is_empty() => {
						novel.content_rating = if links.iter().any(|t| {
							let l = t.to_lowercase();
							l.contains("adult")
								|| l.contains("smut") || l.contains("mature")
								|| l.contains("lolicon") || l.contains("yaoi")
						}) {
							ContentRating::NSFW
						} else {
							ContentRating::Safe
						};
						novel.tags = Some(links);
					}
					_ => {}
				}
			}

			novel.url = Some(url);
		}

		if needs_chapters {
			let novel_id = html
				.select_first("#rating")
				.and_then(|el| el.attr("data-novel-id"));

			if let Some(novel_id) = novel_id {
				let chapter_list_url = format!("{BASE_URL}/ajax-chapter-option?novelId={novel_id}");
				let chapter_html = Request::get(&chapter_list_url)?.html()?;

				let options: Vec<(String, String)> = chapter_html
					.select("select > option")
					.map(|els| {
						els.filter_map(|opt| {
							let key = opt.attr("value")?.trim_start_matches('/').to_string();
							let name = opt.text()?.trim().to_string();
							Some((key, name))
						})
						.collect()
					})
					.unwrap_or_default();
				let names: Vec<&str> = options.iter().map(|(_, name)| name.as_str()).collect();
				let chapters: Vec<Chapter> = options
					.iter()
					.zip(chapter_numbers(&names))
					.map(|((key, name), number)| Chapter {
						key: key.clone(),
						title: chapter_title(name, number),
						chapter_number: Some(number),
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
		_novel: Novel,
		chapter: Chapter,
	) -> Result<Vec<ContentBlock>> {
		let url = format!("{BASE_URL}/{}", chapter.key);

		let html = Request::get(&url)?.html()?;

		let content_list: Vec<ContentBlock> = html
			.select("#chapter-content > p")
			.map(|els| {
				els.filter_map(|p| {
					let text = p.text()?;
					let text = text.trim();
					if text.is_empty() {
						None
					} else if text == "-" {
						Some(ContentBlock::divider())
					} else if text.starts_with('[') && text.ends_with(']') {
						Some(ContentBlock::block_quote(
							text[1..text.len() - 1].to_string(),
						))
					} else {
						Some(ContentBlock::paragraph(text.to_string(), None))
					}
				})
				.collect()
			})
			.unwrap_or_default();

		Ok(content_list)
	}
}

impl ListingProvider for NovelFull {
	fn get_novel_list(&self, listing: Listing, page: i32) -> Result<NovelPageResult> {
		let url = format!("{BASE_URL}/{}?page={page}", listing.id);
		let html = Request::get(&url)?.html()?;
		let entries = parse_novel_list(&html);
		let has_next_page = has_next_page(&html, page);

		Ok(NovelPageResult {
			entries,
			has_next_page,
		})
	}
}

register_source!(NovelFull, ListingProvider, Home);

// Temporary live-verification tests — run once via `cargo test`, then removed.
// Only lengths/counts are asserted or printed, never actual scraped prose.
#[cfg(test)]
mod tests {
	use super::*;
	use buny::alloc::vec;
	use buny_test::buny_test;

	#[buny_test]
	fn test_search() {
		let source = NovelFull::new();
		let result = source
			.get_search_novel_list(None, 1, vec![])
			.expect("search failed");
		println!(
			"entries: {}, has_next_page: {}",
			result.entries.len(),
			result.has_next_page
		);
		assert!(!result.entries.is_empty());
		let first = &result.entries[0];
		assert!(!first.title.is_empty());
		assert!(!first.key.is_empty());
		assert!(first.cover.is_some());
	}

	#[buny_test]
	fn test_home() {
		use buny::{Home, HomeComponentValue};
		let home = NovelFull::new().get_home().unwrap();
		for c in &home.components {
			let count = match &c.value {
				HomeComponentValue::Details { entries, .. }
				| HomeComponentValue::Scroller { entries, .. }
				| HomeComponentValue::Stack { entries, .. }
				| HomeComponentValue::Vertical { entries, .. } => entries.len(),
				_ => 0,
			};
			println!("{:?}: {count}", c.title);
			// The Vertical grid is sent empty; the app pages it itself.
			if !matches!(c.value, HomeComponentValue::Vertical { .. }) {
				assert!(count > 0, "{:?}", c.title);
			}
		}
		assert_eq!(home.components.len(), 3);
	}

	#[buny_test]
	fn test_novel_update_and_chapters() {
		let source = NovelFull::new();
		let novel = Novel {
			key: "reincarnation-of-the-strongest-sword-god.html".to_string(),
			title: String::new(),
			..Default::default()
		};
		let updated = source
			.get_novel_update(novel, true, true, 1)
			.expect("novel update failed");
		println!("title: {}", updated.title);
		println!(
			"description_len: {:?}",
			updated.description.as_ref().map(|d| d.len())
		);
		println!("authors: {:?}", updated.authors);
		println!("tags_count: {:?}", updated.tags.as_ref().map(|t| t.len()));
		println!("status: {:?}", updated.status);
		println!("content_rating: {:?}", updated.content_rating);
		assert!(!updated.title.is_empty());
		assert!(updated.description.is_some());
		let chapters = updated.chapters.expect("no chapters");
		println!("chapter_count: {}", chapters.len());
		assert!(!chapters.is_empty());
		// "Chapter 1 - Starting Over": the label is the number, the rest the title.
		assert_eq!(chapters[0].chapter_number, Some(1.0));
		assert_eq!(chapters[0].title.as_deref(), Some("Starting Over"));
		assert!(
			chapters
				.windows(2)
				.all(|w| w[0].chapter_number < w[1].chapter_number)
		);

		let content = source
			.get_chapter_content_list(
				Novel {
					key: "reincarnation-of-the-strongest-sword-god.html".to_string(),
					..Default::default()
				},
				chapters[0].clone(),
			)
			.expect("chapter content failed");
		println!("content_block_count: {}", content.len());
		assert!(!content.is_empty());
	}
}
