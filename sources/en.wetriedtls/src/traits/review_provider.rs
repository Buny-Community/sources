use buny::{
	Novel, Result, Review, ReviewPageResult, ReviewProvider,
	alloc::{String, Vec, string::ToString},
	helpers::uri::encode_uri_component,
	prelude::*,
};
use serde_json::Value;

use crate::{API_URL, WeTriedTLS, content, parse_timestamp, unescape};

impl ReviewProvider for WeTriedTLS {
	// 10 per page; `perPage`/`limit` are ignored. `sort` takes "popular" (by votes)
	// or "recent", and anything else orders like "popular".
	fn get_novel_reviews(&self, novel: Novel, page: i32) -> Result<ReviewPageResult> {
		let json = Self::get_json(&format!(
			"{API_URL}/series/{}/reviews?page={}&sort=popular",
			encode_uri_component(&novel.key),
			page.max(1)
		))?;
		let entries = json["data"]
			.as_array()
			.ok_or(error!("Invalid review list"))?
			.iter()
			.filter(|r| {
				r["moderation_status"]
					.as_str()
					.is_none_or(|s| s == "visible")
			})
			.filter_map(parse_review)
			.collect();
		let current = json["meta"]["current_page"].as_i64().unwrap_or(1);
		let last = json["meta"]["last_page"].as_i64().unwrap_or(1);
		Ok(ReviewPageResult {
			entries,
			has_next_page: current < last,
		})
	}
}

fn parse_review(r: &Value) -> Option<Review> {
	let content = review_text(r["review"].as_str()?);
	if content.is_empty() {
		return None;
	}
	let user = &r["user"];
	Some(Review {
		author: user["username"].as_str().unwrap_or("Anonymous").to_string(),
		avatar: user["profile_picture"]
			.as_str()
			.filter(|s| !s.is_empty())
			.map(str::to_string),
		title: None,
		content: Some(content),
		// `grade` is a 1-5 star string such as "5.0".
		rating: r["grade"]
			.as_str()
			.and_then(|g| g.parse::<f32>().ok())
			.map(|g| (g / 5.0).clamp(0.0, 1.0)),
		date: r["created_at"].as_str().and_then(parse_timestamp),
	})
}

// Newer reviews are editor HTML (`<p>` blocks); older ones are plain text with
// `\n` line breaks and entity-escaped apostrophes.
pub(crate) fn review_text(raw: &str) -> String {
	if raw.contains('<') {
		return content::plain_text(raw);
	}
	unescape(raw)
		.replace('\u{a0}', " ")
		.lines()
		.map(str::trim)
		.filter(|l| !l.is_empty())
		.collect::<Vec<_>>()
		.join("\n\n")
}
