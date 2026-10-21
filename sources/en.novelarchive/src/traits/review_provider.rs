use buny::{
	Novel, Result, Review, ReviewPageResult, ReviewProvider,
	alloc::{String, Vec},
	imports::{net::Request, std::parse_date},
	prelude::*,
};

use crate::{API_BASE, BASE_URL, NovelArchive, model::CommentsResponse};

impl ReviewProvider for NovelArchive {
	// The API ignores paging and sort params and returns every novel-level comment
	// at once, newest first, so everything comes on page 1.
	fn get_novel_reviews(&self, novel: Novel, page: i32) -> Result<ReviewPageResult> {
		if page > 1 {
			return Ok(ReviewPageResult::default());
		}
		let url = format!("{}/novels/{}/comments", API_BASE, novel.key);
		let response: CommentsResponse = Request::get(&url)?.json_owned()?;
		if let Some(error) = response.error {
			bail!("{error}");
		}

		// Replies are nested under each comment and left out; a reply whose parent
		// was deleted comes back top level, as the site shows it.
		let entries = response
			.comments
			.into_iter()
			.filter_map(|c| {
				let content = c
					.content
					.lines()
					.map(str::trim)
					.filter(|line| !line.is_empty())
					.collect::<Vec<_>>()
					.join("\n\n");
				if content.is_empty() {
					return None;
				}
				let (author, avatar) = c.author.map_or_else(
					|| (String::from("Anonymous"), None),
					|a| (a.username, a.avatar),
				);
				// Avatars are site paths; raw base64 ones (no leading "/" or scheme) are skipped.
				let avatar = avatar.and_then(|a| {
					if a.starts_with("http") {
						Some(a)
					} else if a.starts_with('/') && !a.starts_with("/9j/") {
						Some(format!("{}{a}", BASE_URL))
					} else {
						None
					}
				});
				// "2026-10-13T21:23:11.055Z", always UTC
				let date = c
					.created_at
					.get(..19)
					.and_then(|d| parse_date(d.replace('T', " "), "yyyy-MM-dd HH:mm:ss"));
				Some(Review {
					author,
					avatar,
					title: None,
					content: Some(content),
					rating: None,
					date,
				})
			})
			.collect();

		Ok(ReviewPageResult {
			entries,
			has_next_page: false,
		})
	}
}
