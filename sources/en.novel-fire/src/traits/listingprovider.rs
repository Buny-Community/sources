use buny::{
	Listing, ListingProvider, Novel, NovelPageResult, NovelStatus, Result,
	alloc::{String, string::ToString},
	imports::net::Request,
	prelude::*,
};

use crate::NovelFire;

const BASE_URL: &str = "https://novelfire.net";

impl ListingProvider for NovelFire {
	// this method will be called when a listing or a home section with an associated listing is opened
	fn get_novel_list(&self, listing: Listing, _: i32) -> Result<NovelPageResult> {
		let url: String = match listing.id.as_str() {
			"overall-ranking" => format!("{}/ranking", BASE_URL),
			"most-review" => format!("{}/ranking/most-review", BASE_URL),
			"most-lib" => format!("{}/ranking/most-lib", BASE_URL),
			"ratings" => format!("{}/ranking/ratings", BASE_URL),
			_ => format!("{}/ranking", BASE_URL),
		};

		let html = Request::get(url)?.html()?;
		let novels = html
			.select(".rank-novels .novel-item")
			.map(|els| {
				els.filter_map(|novel_node| {
					let key = novel_node
						.select_first("a")?
						.attr("href")?
						.to_string()
						.replace("https://novelfire.net/book/", "");
					let cover = novel_node.select_first("img")?.attr("data-src")?;
					let title = String::from(
						novel_node
							.select_first("a:not(:has(img))")?
							.text()?
							.to_string()
							.trim(),
					);

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
						status: NovelStatus::Ongoing,
						url: Some(url),
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
