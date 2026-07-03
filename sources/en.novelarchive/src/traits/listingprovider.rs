use buny::{
	Listing, ListingProvider, NovelPageResult, Result, alloc::String, imports::net::Request,
	prelude::*,
};

use crate::{API_BASE, model::NovelListResponse, novel_summary_to_novel};

impl ListingProvider for crate::NovelArchive {
	// this method will be called when a listing or a home section with an associated listing is opened
	fn get_novel_list(&self, listing: Listing, _page: i32) -> Result<NovelPageResult> {
		let endpoint: String = match listing.id.as_str() {
			"trending" => "trending".into(),
			"editors-choice" => "editors-choice".into(),
			"recent" => "recent".into(),
			"recently-updated" => "recently-updated".into(),
			_ => "recent".into(),
		};

		// these curated endpoints don't support pagination (the "page" param is
		// ignored server-side); 50 is the max the server will return regardless
		// of the requested limit, so ask for the full curated batch up front
		let url = format!("{}/novels/{}?limit=50", API_BASE, endpoint);
		let response: NovelListResponse = Request::get(&url)?.json_owned()?;

		let entries = response
			.novels
			.into_iter()
			.map(novel_summary_to_novel)
			.collect();

		Ok(NovelPageResult {
			entries,
			has_next_page: false,
		})
	}
}
