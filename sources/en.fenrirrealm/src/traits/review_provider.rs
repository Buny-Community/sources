use buny::{
	Novel, Result, Review, ReviewPageResult, ReviewProvider,
	alloc::{String, Vec, string::ToString},
	prelude::*,
};

use serde_json::Value;

use crate::{API_URL, BASE_URL, FenrirRealm, parse_timestamp};

// `per_page` is capped at 100. Replies share the pages with top-level comments.
const COMMENT_PAGE_SIZE: i32 = 50;

impl ReviewProvider for FenrirRealm {
	fn get_novel_reviews(&self, novel: Novel, page: i32) -> Result<ReviewPageResult> {
		// Comments are keyed by the series uuid, not the slug used as the novel key.
		let series = Self::get_json(&format!("{API_URL}/series/{}", novel.key))?;
		let uuid = series["uuid"]
			.as_str()
			.filter(|s| !s.is_empty())
			.ok_or(error!("Missing series id"))?;
		let json = Self::get_json(&format!(
			"{API_URL}/comments/series/{uuid}?sort=latest&page={}&per_page={COMMENT_PAGE_SIZE}",
			page.max(1)
		))?;

		// Replies come flat, next to their comment, and make no sense out of their thread.
		let entries = json["data"]
			.as_array()
			.into_iter()
			.flatten()
			.filter(|c| c["parent_id"].is_null())
			.filter(|c| c["moderation_status"]["is_public"].as_bool() != Some(false))
			.filter_map(parse_comment)
			.collect();

		let meta = &json["meta"];
		Ok(ReviewPageResult {
			entries,
			has_next_page: meta["current_page"].as_i64().unwrap_or(1)
				< meta["last_page"].as_i64().unwrap_or(1),
		})
	}
}

// Comments are plain text with `\n` line breaks, and a rare literal `<br>`.
fn parse_comment(c: &Value) -> Option<Review> {
	let content = c["content"]
		.as_str()?
		.replace("<br>", "\n")
		.lines()
		.map(str::trim)
		.filter(|l| !l.is_empty())
		.collect::<Vec<_>>()
		.join("\n\n");
	if content.is_empty() {
		return None;
	}
	let user = &c["user"];
	let name = |key: &str| user[key].as_str().map(str::trim).filter(|s| !s.is_empty());
	Some(Review {
		author: name("name")
			.or(name("username"))
			.unwrap_or("Anonymous")
			.to_string(),
		avatar: name("avatar").map(|a| -> String {
			if a.starts_with("http") {
				a.to_string()
			} else {
				format!("{BASE_URL}/{}", a.trim_start_matches('/'))
			}
		}),
		title: None,
		content: Some(content),
		rating: None,
		date: c["created_at"].as_str().and_then(parse_timestamp),
	})
}
