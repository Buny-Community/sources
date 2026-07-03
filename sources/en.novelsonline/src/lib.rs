#![no_std]
use buny::{
	Chapter, ContentBlock, FilterValue, Novel, NovelPageResult, NovelStatus, Result, Source,
	alloc::{String, Vec, string::ToString},
	helpers::{string::StripPrefixOrSelf, uri::encode_uri_component},
	imports::net::Request,
	prelude::*,
};

pub mod traits;

struct NovelsOnline;

const BASE_URL: &str = "https://novelsonline.org";

impl Source for NovelsOnline {
	fn new() -> Self {
		Self
	}

	fn get_search_novel_list(
		&self,
		query: Option<String>,
		_page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<NovelPageResult> {
		let url = format!("{BASE_URL}/detailed-search");
		let body = format!(
			"keyword={}&search=1",
			encode_uri_component(query.unwrap_or_default())
		);
		let req = Request::post(&url)?.body(&body);
		let html = req.html()?;
		let novels = html
			.select(".top-novel-block")
			.map(|els| {
				els.filter_map(|novel_node| {
					let key = novel_node
						.select_first("a")?
						.attr("href")?
						.strip_prefix_or_self(format!("{}/", &BASE_URL))
						.into();

					println!("Extracted novel key: {}", key);
					let cover = novel_node.select_first("img")?.attr("src")?.to_string();
					println!("Extracted cover URL: {}", cover);
					let title = novel_node.select_first("a")?.text()?.to_string();
					println!("Extracted title: {}", title);

					Some(Novel {
						key,
						title,
						cover: Some(cover),
						status: NovelStatus::Ongoing,
						..Default::default()
					})
				})
				.collect()
			})
			.unwrap_or_default();

		Ok(NovelPageResult {
			entries: novels,
			has_next_page: false,
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
		let html = Request::get(url)?.html()?;

		if needs_details {
			let cover = html
				.select_first(".novel-cover img")
				.unwrap()
				.attr("src")
				.unwrap();

			let title = html
				.select_first(".novel-cover img")
				.unwrap()
				.attr("alt")
				.unwrap();

			novel.title = title;
			novel.cover = Some(cover.to_string());
			novel.url = Some(format!("{BASE_URL}/{}", novel.key));

			if let Some(els) = html.select(".novel-detail-item") {
				for el in els {
					let header = match el
						.select_first(".novel-detail-header")
						.and_then(|h| h.text())
					{
						Some(h) => h,
						None => continue,
					};
					let body = match el.select_first(".novel-detail-body").and_then(|b| b.text()) {
						Some(b) => b,
						None => continue,
					};

					match header.as_str() {
						"Author(s)" => {
							let authors = body
								.split(' ')
								.map(|s| s.trim().to_string())
								.collect::<Vec<String>>();
							novel.authors = Some(authors);
						}
						"Description" => {
							novel.description = Some(body);
						}
						"Genre" => {
							let tags = body
								.split(' ')
								.map(|s| s.trim().to_string())
								.collect::<Vec<String>>();
							novel.tags = Some(tags);
						}
						"Status" => {
							if body.contains("Ongoing") {
								novel.status = NovelStatus::Ongoing;
							} else if body.contains("Completed") {
								novel.status = NovelStatus::Completed;
							}
						}
						_ => {}
					}
				}
			}
		}

		if needs_chapters {
			let mut chapters: Vec<Chapter> = Vec::new();

			if let Some(els) = html.select(".chapters .panel") {
				for chapter_node in els {
					let volume = chapter_node
						.select_first(".panel-title a")
						.unwrap()
						.text()
						.unwrap_or_default()
						.strip_prefix("Volume ")
						.unwrap_or_default()
						.parse::<f32>()
						.unwrap_or(-1.0);

					if let Some(chapter_els) = chapter_node.select(".chapter-chs li") {
						let mut chapter_number = 1.0;
						for chapter_el in chapter_els {
							let title: String = chapter_el
								.select_first("a")
								.unwrap()
								.text()
								.unwrap()
								.trim()
								.strip_prefix_or_self("CH ")
								.into();

							let chapter_id = chapter_el
								.select_first("a")
								.unwrap()
								.attr("href")
								.unwrap()
								.strip_prefix_or_self(format!("{}/{}/", &BASE_URL, novel.key))
								.into();

							// print everything for debug
							println!(
								"Extracted chapter - Volume: {}, Chapter Number: {}, Title: {}, ID: {}",
								volume, chapter_number, &title, chapter_id
							);
							chapters.push(Chapter {
								key: chapter_id,
								title: Some(title),
								chapter_number: Some(chapter_number),
								volume_number: Some(volume),
								..Default::default()
							});
							chapter_number += 1.0;
						}
					}
				}
			}

			let _has_more = html
				.select_first(".pagination li.page-item:last-child")
				.is_some_and(|el| !el.has_class("disabled"));
			println!("novel chapter count {}", chapters.len());
			novel.chapters = Some(chapters);
			// novel.has_more_chapters = Some(has_more);
		}
		Ok(novel)
	}

	fn get_chapter_content_list(
		&self,
		novel: Novel,
		chapter: Chapter,
	) -> Result<Vec<ContentBlock>> {
		let url = format!("{}/{}/{}", BASE_URL, novel.key, chapter.key);
		let html = Request::get(&url)?.html()?;

		let mut content_list: Vec<ContentBlock> = html
			.select("#contentall p")
			.map(|els| {
				els.filter_map(|content_node| {
					let content = content_node.own_text()?.to_string();

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

		let review_link = format!("CHAPTER LINK: [click here for chapter reviews.]({})", url);
		content_list.push(ContentBlock::Divider);
		content_list.push(ContentBlock::paragraph(review_link, None));
		Ok(content_list)
	}
}

register_source!(NovelsOnline, ListingProvider);
