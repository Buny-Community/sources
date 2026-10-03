#![no_std]
use buny::{
	Chapter, ContentBlock, ContentRating, FilterValue, Novel, NovelPageResult, NovelStatus, Result,
	Source,
	alloc::{String, Vec, string::ToString, vec},
	helpers::{element::ElementHelpers, uri::QueryParameters},
	imports::{defaults::defaults_get, net::Request, std::parse_date},
	prelude::*,
};
use chapter_numbers::{chapter_numbers, chapter_title};

pub mod traits;

// to create a source, you need a struct that implements the Source trait
// the struct can contain properties that are initialized with the new() method
struct RoyalRoad;

const BASE_URL: &str = "https://royalroad.com";

impl Source for RoyalRoad {
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
		// https://novelfire.net/search?keyword=shadow&page=4
		let mut qs = QueryParameters::new();
		qs.push("globalFilters", Some("false"));
		qs.push("title", query.as_deref());
		qs.push("page", Some(&page.to_string()));

		for filter in filters {
			match filter {
				FilterValue::MultiSelect {
					included, excluded, ..
				} => {
					for id in included {
						qs.push("tagsAdd", Some(&id.to_lowercase()));
					}
					for id in excluded {
						qs.push("tagsRemove", Some(&id.to_lowercase()));
					}
				}
				FilterValue::Sort { id, .. } => {
					qs.push("orderBy", Some(&id));
				}
				FilterValue::Select { id, .. } => {
					qs.push("type", Some(&id));
				}
				FilterValue::Text { value, .. } => {
					qs.push("author", Some(&value));
				}
				_ => {}
			}
		}

		let url = format!("{}/fictions/search?{qs}", BASE_URL);
		let html = Request::get(url)?.html()?;
		let entries: Vec<Novel> = html
			.select(".fiction-list > .fiction-list-item")
			.map(|els| {
				els.filter_map(|novel_node| {
					let key = novel_node
						.select_first("a")?
						.attr("href")?
						.to_string()
						.replace("/fiction/", "");
					let title = String::from(
						novel_node
							.select_first("a:not(:has(img))")?
							.text()?
							.to_string()
							.trim(),
					);

					let mut cover = novel_node
						.select_first("a")?
						.select_first("img")?
						.attr("src")?
						.to_string();
					if !cover.starts_with("https://") {
						cover = format!("{}{cover}", BASE_URL);
					}

					let tags: Option<Vec<String>> =
						novel_node.select(".label:not(.pull-right)").map(|els| {
							els.filter_map(|el| {
								let tag = el.text().unwrap();
								Some(tag)
							})
							.collect()
						});

					let status = novel_node
						.select(".label")
						.map(|els| {
							els.filter_map(|el| {
								let statustxt = el.text().unwrap();
								match statustxt.as_str() {
									"COMPLETED" => Some(NovelStatus::Completed),
									"ONGOING" => Some(NovelStatus::Ongoing),
									"INACTIVE" => Some(NovelStatus::Hiatus),
									"HIATUS" => Some(NovelStatus::Hiatus),
									"CANCELLED" => Some(NovelStatus::Cancelled),
									_ => None,
								}
							})
							.next()
							.unwrap_or(NovelStatus::Unknown)
						})
						.unwrap_or(NovelStatus::Unknown);

					let description = novel_node
						.select(format!("#description-{}", key.split('/').next().unwrap()))
						.map(|els| {
							els.filter_map(|el| {
								let desc = el.untrimmed_text().unwrap();
								Some(desc)
							})
							.collect::<Vec<String>>()
							.join("\n\n")
						});

					Some(Novel {
						key,
						title,
						tags,
						status,
						description,
						cover: Some(cover),
						..Default::default()
					})
				})
				.collect()
			})
			.unwrap_or_default();

		let has_next_page = html
			.select_first(".pagination")
			.is_some_and(|el| !el.has_class("page-active"));

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
		let url = format!("{}/fiction/{}", BASE_URL, novel.key);
		let html = Request::get(&url)?.html()?;
		let info_div = html.select_first(".fiction-info").unwrap();

		if needs_details {
			let main_div = html.select_first(".fic-header img").unwrap();
			let mut cover = main_div.attr("src").unwrap();
			if !cover.starts_with("https://") {
				cover = format!("{}{cover}", BASE_URL);
			}
			let title = html.select_first(".fic-title h1").unwrap().text();
			let author = html.select_first(".fic-title a").unwrap().text().unwrap();

			let description = info_div.select(".description .hidden-content").map(|els| {
				els.filter_map(|el| {
					let desc = el.text_with_newlines().unwrap();
					Some(desc)
				})
				.collect::<Vec<String>>()
				.join("\n\n")
			});

			let content_rating = info_div
				.select(".text-center")
				.map(|els| {
					let rating = els.text().unwrap();
					if rating.contains("Sexual Content") {
						ContentRating::NSFW
					} else if rating.contains("Graphic Violence")
						|| rating.contains("Sensitive Content")
					{
						ContentRating::Suggestive
					} else {
						ContentRating::Safe
					}
				})
				.unwrap_or(ContentRating::Unknown);

			let tags: Option<Vec<String>> = info_div.select(".label:not(.pull-right)").map(|els| {
				els.filter_map(|el| {
					let tag = el.text().unwrap();
					Some(tag)
				})
				.collect()
			});

			let status = info_div
				.select(".label")
				.map(|els| {
					els.filter_map(|el| {
						let statustxt = el.text().unwrap();
						match statustxt.as_str() {
							"COMPLETED" => Some(NovelStatus::Completed),
							"ONGOING" => Some(NovelStatus::Ongoing),
							"INACTIVE" => Some(NovelStatus::Hiatus),
							"HIATUS" => Some(NovelStatus::Hiatus),
							"CANCELLED" => Some(NovelStatus::Cancelled),
							_ => None,
						}
					})
					.next()
					.unwrap_or(NovelStatus::Unknown)
				})
				.unwrap_or(NovelStatus::Unknown);

			if let Some(title) = title {
				novel.title = title;
			}

			novel.cover = Some(cover.to_string());
			novel.authors = Some(vec![author]);
			novel.description = description;
			novel.status = status;
			novel.content_rating = content_rating;
			novel.tags = tags;
			novel.url = Some(url);
		}
		if needs_chapters {
			let rows: Vec<(String, String, Option<i64>)> = info_div
				.select(".chapter-row")
				.map(|els| {
					els.filter_map(|el| {
						let link = el.select_first("a")?;
						let chapter_key = link
							.attr("href")?
							.replace(&format!("/fiction/{}/chapter/", novel.key), "");
						let name = link.text()?.trim().to_string();
						let date_uploaded = el
							.select_first(".text-right a time")
							.and_then(|time| time.attr("datetime"))
							.and_then(|date| parse_date(date, "yyyy-MM-dd'T'HH:mm:ss.SSSSSSZ"));
						Some((chapter_key, name, date_uploaded))
					})
					.collect()
				})
				.unwrap_or_default();
			let names: Vec<&str> = rows.iter().map(|(_, name, _)| name.as_str()).collect();
			let chapters: Vec<Chapter> = rows
				.iter()
				.zip(chapter_numbers(&names))
				.map(|((chapter_key, name, date_uploaded), number)| Chapter {
					key: chapter_key.clone(),
					chapter_number: Some(number),
					title: chapter_title(name, number),
					url: Some(format!(
						"{}/fiction/{}/chapter/{}",
						BASE_URL, novel.key, chapter_key
					)),
					date_uploaded: *date_uploaded,
					..Default::default()
				})
				.collect();

			novel.chapters = Some(chapters);
			novel.has_more_chapters = Some(false);
		}
		Ok(novel)
	}

	fn get_chapter_content_list(
		&self,
		novel: Novel,
		chapter: Chapter,
	) -> Result<Vec<ContentBlock>> {
		let url = format!("{}/fiction/{}/chapter/{}", BASE_URL, novel.key, chapter.key);
		let html = Request::get(&url)?.html()?;

		let mut content_list: Vec<ContentBlock> = html
			.select(".chapter-content")
			.map(|els| {
				els.filter_map(|content_node| {
					// paragraph might have a "read at website" element in it so we use own_text.
					let content = content_node.text_with_newlines().unwrap();
					if content.starts_with('[') && content.ends_with(']') {
						let mut quote = content.chars();
						quote.next();
						quote.next_back();
						quote.as_str().to_string();

						return Some(ContentBlock::block_quote(quote.as_str().to_string()));
					} else if content == "***" {
						return Some(ContentBlock::Divider);
					}
					Some(ContentBlock::paragraph(content, None))
				})
				.collect()
			})
			.unwrap_or_default();

		// author note placement: "after" (default) | "before" | "hidden"
		let author_note_position =
			defaults_get::<String>("authorNotePosition").unwrap_or_else(|| "after".into());
		if author_note_position != "hidden" && html.select_first(".author-note").is_some() {
			let author_note = html
				.select(".author-note")
				.unwrap()
				.text_with_newlines()
				.unwrap();
			let author_note_title = html
				.select_first(".author-note-portlet .portlet-title")
				.unwrap()
				.text()
				.unwrap();
			let block = ContentBlock::BlockQuote(author_note_title + "\n" + &author_note);
			if author_note_position == "before" {
				content_list.insert(0, block);
			} else {
				content_list.push(block);
			}
		}

		let review_link = format!("LINK: [click here for chapter reviews.]({})", url);
		content_list.push(ContentBlock::Divider);
		content_list.push(ContentBlock::paragraph(review_link, None));
		Ok(content_list)
	}
}

// the register_source! macro generates the necessary wasm functions for buny
register_source!(
	RoyalRoad,
	// after the name of the source struct, list all the extra traits it implements
	ListingProvider,
	Home,
	DeepLinkHandler
);

#[cfg(test)]
mod test {
	use super::*;
	use buny::{Home, HomeComponentValue, Listing, ListingProvider};
	use buny_test::buny_test;

	#[buny_test]
	fn test_chapter_numbers() {
		// "Prologue", then "Chapter 1 - The Void": the prologue goes before
		// chapter 1 instead of pushing every chapter up by one.
		let novel = RoyalRoad::new()
			.get_novel_update(
				Novel {
					key: "145896/dao-of-the-world-walker-craft-based-slice-of-life".into(),
					..Default::default()
				},
				false,
				true,
				1,
			)
			.unwrap();
		let chapters = novel.chapters.unwrap();
		println!("{} chapters", chapters.len());
		assert!(chapters.len() > 200);
		assert_eq!(chapters[0].chapter_number, Some(0.5));
		assert_eq!(chapters[0].title.as_deref(), Some("Prologue"));
		assert_eq!(chapters[1].chapter_number, Some(1.0));
		assert_eq!(chapters[1].title.as_deref(), Some("The Void"));
		assert!(
			chapters
				.windows(2)
				.all(|w| w[0].chapter_number < w[1].chapter_number)
		);
	}

	#[buny_test]
	fn test_listings() {
		let source = RoyalRoad::new();
		for id in [
			"best-rated",
			"trending",
			"rising-stars",
			"new-releases",
			"latest-updates",
		] {
			let listing = Listing {
				id: id.into(),
				..Default::default()
			};
			let result = source.get_novel_list(listing, 1).unwrap();
			println!("{id}: {} entries", result.entries.len());
			assert!(!result.entries.is_empty(), "{id}");
		}
	}

	#[buny_test]
	fn test_home() {
		let home = RoyalRoad::new().get_home().unwrap();
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
		assert_eq!(home.components.len(), 5);
	}
}
