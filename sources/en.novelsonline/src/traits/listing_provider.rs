use buny::{
	Listing, ListingProvider, Novel, NovelPageResult, NovelStatus, Result,
	alloc::{String, string::ToString},
	helpers::string::StripPrefixOrSelf,
	imports::net::Request,
	prelude::*,
};

use crate::NovelsOnline;

const BASE_URL: &str = "https://novelsonline.org";

impl ListingProvider for NovelsOnline {
	// this method will be called when a listing or a home section with an associated listing is opened
	fn get_novel_list(&self, listing: Listing, _: i32) -> Result<NovelPageResult> {
		let url: String = match listing.id.as_str() {
			"top-novel" => format!("{}/top-novel", BASE_URL),
			_ => format!("{}/ranking", BASE_URL),
		};

		println!("Fetching novel list from URL: {}", url);
		let html = Request::get(url)?.html()?;
		let novels = html
			.select(".top-novel-block")
			.map(|els| {
				els.filter_map(|novel_node| {
					let key = novel_node
						.select_first("a")?
						.attr("href")?
						.strip_prefix_or_self(format!("{}/", &BASE_URL))
						.into();

					let cover = novel_node.select_first("img")?.attr("src")?.to_string();
					let title = novel_node.select_first("img")?.attr("alt")?.to_string();

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
}
