use buny::{
	Novel, Result, Review, ReviewPageResult, ReviewProvider,
	alloc::{Vec, string::ToString},
	prelude::*,
};

use crate::{API_URL, NovelBuddy, html_paragraphs, parse_timestamp, title_id};

impl ReviewProvider for NovelBuddy {
	fn get_novel_reviews(&self, novel: Novel, page: i32) -> Result<ReviewPageResult> {
		let json = self.next_data(&novel.key, 0, Some(&novel.key))?;
		let id = title_id(&json["pageProps"])?;
		// The site pages with an opaque cursor, but the API also takes `page`, 10 top-level
		// comments each (`limit=20` still returns 10; above 20 is rejected).
		let json = Self::get_json(&format!(
			"{API_URL}/comments/title/{id}?sort=newest&page={}",
			page.max(1)
		))?;
		if !json["success"].as_bool().unwrap_or(false) {
			bail!(
				"{}",
				json["message"]
					.as_str()
					.unwrap_or("Comments request failed")
			);
		}
		let data = &json["data"];

		// Replies come nested under their comment and make no sense out of their thread.
		let entries = data["items"]
			.as_array()
			.into_iter()
			.flatten()
			.filter(|c| c["is_deleted"].as_i64() == Some(0))
			.filter_map(|c| {
				let content = html_paragraphs(c["content"].as_str()?).join("\n\n");
				if content.is_empty() {
					return None;
				}
				let user = &c["user"];
				Some(Review {
					author: user["name"].as_str().unwrap_or("Anonymous").to_string(),
					avatar: user["avatar"]
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

		Ok(ReviewPageResult {
			entries,
			has_next_page: data["has_more"].as_bool().unwrap_or(false),
		})
	}
}
