use buny::{
	Listing, ListingProvider, Novel, NovelPageResult, NovelStatus, Result,
	alloc::{String, Vec, string::ToString},
	imports::net::Request,
	prelude::*,
};

use crate::RoyalRoad;

const BASE_URL: &str = "https://royalroad.com";

impl ListingProvider for RoyalRoad {
	// this method will be called when a listing or a home section with an associated listing is opened
	fn get_novel_list(&self, listing: Listing, page: i32) -> Result<NovelPageResult> {
		let url: String = match listing.id.as_str() {
			"best-rated" => format!("{}/fictions/best-rated?page={}", BASE_URL, page),
			"new-releases" => format!("{}/fictions/new?page={}", BASE_URL, page),
			"rising-stars" => format!("{}/fictions/rising-stars?page={}", BASE_URL, page),
			"trending" => format!("{}/fictions/trending?page={}", BASE_URL, page),
			"latest-updates" => format!("{}/fictions/latest-updates?page={}", BASE_URL, page),
			_ => format!("{}/fictions/best-rated?page={}", BASE_URL, page),
		};

		let html = Request::get(url)?.html()?;
		let novels = html
			.select(".fiction-list > .fiction-list-item")
			.map(|els| {
				els.filter_map(|novel_node| {
					let key = novel_node
						.select_first("a")?
						.attr("href")?
						.to_string()
						.replace("/fiction/", "");

					let mut cover = novel_node
						.select_first("a")?
						.select_first("img")?
						.attr("src")?
						.to_string();
					if !cover.starts_with("https://") {
						cover = format!("{}{cover}", BASE_URL);
					}

					let title = String::from(
						novel_node
							.select_first("a:not(:has(img))")?
							.text()?
							.to_string()
							.trim(),
					);

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

					// Join the paragraphs directly: text_with_newlines() returns None
					// under buny-test-runner (its fragment parser has no <body>), and
					// unwrapping it panicked every listing in tests.
					let description = novel_node
						.select_first(format!("#description-{}", key.split('/').next()?))
						.and_then(|el| {
							let paragraphs = el
								.select("p")
								.map(|els| {
									els.filter_map(|p| p.text())
										.filter(|t| !t.is_empty())
										.collect::<Vec<String>>()
										.join("\n\n")
								})
								.filter(|d| !d.is_empty());
							paragraphs.or_else(|| el.text())
						});

					let url = String::from(BASE_URL)
						+ &novel_node
							.select_first("a")
							.unwrap()
							.attr("href")
							.unwrap()
							.to_string();

					Some(Novel {
						key,
						title,
						cover: Some(cover),
						tags,
						description,
						status,
						url: Some(url),
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
			entries: novels,
			has_next_page, //: false,
		})
	}
}
