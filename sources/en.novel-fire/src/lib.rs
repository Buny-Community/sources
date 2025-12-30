#![no_std]
use buny::{
	Chapter, ContentBlock, ContentRating, FilterValue, Novel, NovelPageResult, NovelStatus, Result,
	Source,
	alloc::{String, Vec, string::ToString, vec},
	imports::std::parse_date,
	imports::{net::Request, std::print},
	prelude::*,
};

mod model;
use model::{ChapterData, ChapterResponse};

pub mod traits;

// to create a source, you need a struct that implements the Source trait
// the struct can contain properties that are initialized with the new() method
struct NovelFire;

const BASE_URL: &str = "https://novelfire.net";

impl Source for NovelFire {
	// this method is called once when the source is initialized
	// perform any necessary setup here
	fn new() -> Self {
		println!("hello is this source working");
		Self
	}

	// this method will be called first without a query when the search page is opened,
	// then when a search query is entered or filters are changed
	fn get_search_novel_list(
		&self,
		query: Option<String>,
		page: i32,
		_filters: Vec<FilterValue>,
	) -> Result<NovelPageResult> {
		// https://novelfire.net/search?keyword=shadow&page=4
		let url = format!(
			"{}/search?keyword={}&page={}",
			&BASE_URL,
			query.unwrap_or_default(),
			page
		);
		let html = Request::get(&url)?.html()?;
		let entries: Vec<Novel> = html
			.select(".novel-list.chapters > li.novel-item")
			.map(|els| {
				els.filter_map(|novel_node| {
					let key = novel_node
						.select_first("a")?
						.attr("href")?
						.to_string()
						.replace("/book/", "");
					let title = novel_node.select_first("a")?.attr("title")?.to_string();

					let cover = format!(
						"{}/{}",
						&BASE_URL,
						novel_node.select_first("img")?.attr("src")?
					);

					Some(Novel {
						key,
						title,
						cover: Some(cover),
						..Default::default()
					})
				})
				.collect()
			})
			.unwrap_or_default();

		let has_next_page = html
			.select_first(".pagination li.page-item:last-child")
			.is_some_and(|el| !el.has_class("disabled"));

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
		page: i32,
	) -> Result<Novel> {
		let url = format!("{}/book/{}", &BASE_URL, novel.key);
		let html = Request::get(&url)?.html()?;

		println!("Selecting post id...");
		let post_id = html
			.select_first("#novel-report")
			.unwrap()
			.attr("report-post_id")
			.unwrap_or_default();
		println!("Post id: {}", &post_id);

		if needs_details {
			let main_div = html.select_first(".cover img").unwrap();
			let cover = main_div.attr("src").unwrap();
			let title = html.select_first(".main-head .novel-title").unwrap().text();
			let author = html
				.select_first(".author a")
				.unwrap()
				.attr("title")
				.unwrap();

			let description = html.select("#info .content p").map(|els| {
				els.filter_map(|el| {
					let desc = el.text().unwrap();
					Some(desc)
				})
				.collect::<Vec<String>>()
				.join("\n\n")
			});
			let tags: Option<Vec<String>> = html.select(".categories ul li a").map(|els| {
				els.filter_map(|el| {
					let tag = el.text().unwrap();
					Some(tag)
				})
				.collect()
			});

			if let Some(title) = title {
				novel.title = title;
			}

			novel.cover = Some(cover.to_string());
			novel.authors = Some(vec![author]);
			novel.description = description;
			novel.status = NovelStatus::Ongoing;
			novel.content_rating = ContentRating::Safe;
			novel.tags = tags;
			novel.url = Some(url);
		}
		if needs_chapters {
			let url = format!("{}/listChapterDataAjax?post_id={}", &BASE_URL, post_id);

			let mut response = Request::get(url)?.send()?;
			let data = response.get_json::<ChapterResponse>()?;
			let chapters = data
				.data
				.iter()
				.map(|chapter_data| Chapter {
					key: format!("chapter-{}", chapter_data.n_sort),
					chapter_number: Some(chapter_data.n_sort as f32),
					title: Some(
						chapter_data
							.title
							.to_string()
							.split('-')
							.last()
							.unwrap_or(&chapter_data.title)
							.split(":")
							.last()
							.unwrap_or(&chapter_data.title)
							.trim()
							.to_string(),
					),
					date_uploaded: Some(0),
					..Default::default()
				})
				.collect::<Vec<Chapter>>();

			// let has_more = html
			// 	.select_first(".pagination li.page-item:last-child")
			// 	.is_some_and(|el| !el.has_class("disabled"));
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
		let url = format!("{}/book/{}/{}", &BASE_URL, novel.key, chapter.key);
		let html = Request::get(&url)?.html()?;

		println!("Fetching chapter content from URL: {}", &url);
		let mut content_list: Vec<ContentBlock> = html
			.select("#content p")
			.map(|els| {
				els.filter_map(|content_node| {
					// paragraph might have a "read at website" element in it so we use own_text.
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

		let review_link = format!("LINK: [click here for chapter reviews.]({})", url);
		content_list.push(ContentBlock::Divider);
		content_list.push(ContentBlock::paragraph(review_link, None));
		Ok(content_list)
	}
}

// the register_source! macro generates the necessary wasm functions for buny
register_source!(
	NovelFire,
	// after the name of the source struct, list all the extra traits it implements
	ListingProvider,
	Home,
	DynamicFilters,
	DynamicSettings,
	DynamicListings,
	NotificationHandler,
	AlternateCoverProvider,
	DeepLinkHandler
);
