#![no_std]
use buny::{
	Chapter, ContentBlock, ContentRating, FilterValue, Novel, NovelPageResult, NovelStatus, Result,
	Source,
	alloc::{String, Vec, string::ToString, vec},
	imports::std::parse_date,
	imports::{net::Request, std::print},
	prelude::*,
};

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
		print("url is:");
		print(&url);
		let html = Request::get(&url)?.html()?;
		let entries: Vec<Novel> = html
			.select(".novel-list.chapters > li.novel-item")
			.map(|els| {
				els.filter_map(|novel_node| {
					let key = novel_node
						.select_first("a")?
						.attr("href")?
						.to_string()
						.replace("https://novelfire.net/book/", "");
					let title = novel_node.select_first("a")?.attr("title")?.to_string();

					let cover = novel_node
						.select_first(".cover-wrap img")?
						.attr("src")?
						.to_string();

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
		if needs_details {
			let url = format!("{}/book/{}", &BASE_URL, novel.key);

			let html = Request::get(&url)?.html()?;

			let main_div = html.select_first(".cover img").unwrap();
			let cover = main_div.attr("data-src").unwrap();
			let title = html.select_first(".main-head .novel-title").unwrap().text();
			let author = html
				.select_first(".author a")
				.unwrap()
				.attr("title")
				.unwrap();

			let description = html.select(".summary .content p").map(|els| {
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
			let url = format!("{}/book/{}/chapters?page={}", &BASE_URL, novel.key, page);
			println!("URL NEW IS: {}", url);
			let html = Request::get(url)?.html()?;

			let chapters: Vec<Chapter> = html
				.select(".chapter-list > li")
				.map(|els| {
					els.filter_map(|chapter_node| {
						let title = chapter_node
							.select_first("a")?
							.attr("title")?
							.split(":")
							.nth(1)
							.unwrap_or("")
							.trim()
							.to_string();

						let chapter_number = chapter_node
							.select_first(".chapter-no")?
							.text()?
							.to_string()
							.trim()
							.parse::<f32>()
							.expect("Failed to get chapter number.");
						let chapter_id = chapter_node
							.select_first("a")?
							.attr("href")?
							.split("/")
							.last()
							.unwrap_or("")
							.to_string();
						let _chapter_url =
							chapter_node.select_first("a")?.attr("href")?.to_string();
						let date_updated = chapter_node
							.select_first("time.chapter-update")?
							.attr("datetime")
							.and_then(|d| parse_date(d, "yyyy-MM-dd HH:mm:ss"));

						Some(Chapter {
							key: chapter_id,
							chapter_number: Some(chapter_number),
							title: Some(title),
							date_uploaded: date_updated,
							..Default::default()
						})
					})
					.collect()
				})
				.unwrap_or_default();

			let has_more = html
				.select_first(".pagination li.page-item:last-child")
				.is_some_and(|el| !el.has_class("disabled"));
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
		let url = format!("{}/book/{}/{}", &BASE_URL, novel.key, chapter.key);
		let html = Request::get(&url)?.html()?;

		let mut content_list: Vec<ContentBlock> = html
			.select("#chapter-container p")
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
