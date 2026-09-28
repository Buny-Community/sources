use buny::{DeepLinkHandler, DeepLinkResult, Result, alloc::String, alloc::string::ToString};

use crate::{BASE_URL, WeTriedTLS};

impl DeepLinkHandler for WeTriedTLS {
	// Handles https://wetriedtls.com/series/<slug> and
	// https://wetriedtls.com/series/<slug>/<chapter-slug>.
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = url
			.strip_prefix(BASE_URL)
			.or_else(|| url.strip_prefix("http://wetriedtls.com"))
		else {
			return Ok(None);
		};
		let path = path.split(['?', '#']).next().unwrap_or("");
		let Some(path) = path.strip_prefix("/series/") else {
			return Ok(None);
		};
		let mut parts = path.split('/').filter(|p| !p.is_empty());

		let Some(slug) = parts.next() else {
			return Ok(None);
		};
		let result = match (parts.next(), parts.next()) {
			(None, _) => DeepLinkResult::Novel {
				key: slug.to_string(),
			},
			(Some(chapter), None) => DeepLinkResult::Chapter {
				novel_key: slug.to_string(),
				key: chapter.to_string(),
			},
			_ => return Ok(None),
		};
		Ok(Some(result))
	}
}
