use buny::{
	Novel, Result, Review, ReviewPageResult, ReviewProvider,
	alloc::{String, Vec, string::ToString},
	prelude::*,
};

use crate::{API_URL, Chikari, parse_timestamp};

// The API caps limit at 20 top-level comments; their replies come inline after each one.
const REVIEW_PAGE_SIZE: i32 = 20;

impl ReviewProvider for Chikari {
	fn get_novel_reviews(&self, novel: Novel, page: i32) -> Result<ReviewPageResult> {
		let offset = (page.max(1) - 1) * REVIEW_PAGE_SIZE;
		let json = Self::get_json(&format!(
			"{API_URL}/novels/{}/comments?limit={REVIEW_PAGE_SIZE}&offset={offset}&sort=new",
			novel.key
		))?;

		// Replies make no sense out of their thread, so only top-level comments are kept.
		let entries = json["comments"]
			.as_array()
			.into_iter()
			.flatten()
			.filter(|c| c["parent_id"].is_null())
			.filter_map(|c| {
				let content = c["body"]
					.as_str()?
					.lines()
					.map(str::trim)
					.filter(|line| !line.is_empty())
					.collect::<Vec<_>>()
					.join("\n\n");
				if content.is_empty() {
					return None;
				}
				let author = &c["author"];
				Some(Review {
					author: author["username"]
						.as_str()
						.map_or_else(|| String::from("Anonymous"), str::to_string),
					avatar: author["avatar_url"]
						.as_str()
						.filter(|s| !s.is_empty())
						.map(str::to_string),
					title: None,
					content: Some(content),
					rating: None,
					date: c["created_at"].as_str().and_then(parse_timestamp),
				})
			})
			.collect::<Vec<_>>();

		// `total` counts top-level comments only, which is what limit/offset page over.
		let has_next_page =
			json["total"].as_i64().unwrap_or(0) > i64::from(offset + REVIEW_PAGE_SIZE);

		Ok(ReviewPageResult {
			entries,
			has_next_page,
		})
	}
}
