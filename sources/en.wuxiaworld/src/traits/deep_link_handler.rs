use buny::{DeepLinkHandler, DeepLinkResult, Result, alloc::String, alloc::string::ToString};

use crate::Wuxiaworld;

impl DeepLinkHandler for Wuxiaworld {
	// Handles https://www.wuxiaworld.com/novel/<slug> and
	// https://www.wuxiaworld.com/novel/<slug>/<chapter-slug>.
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = ["https://", "http://"]
			.iter()
			.filter_map(|scheme| url.strip_prefix(scheme))
			.find_map(|rest| {
				rest.strip_prefix("www.wuxiaworld.com")
					.or_else(|| rest.strip_prefix("wuxiaworld.com"))
			})
		else {
			return Ok(None);
		};
		let path = path.split(['?', '#']).next().unwrap_or("");
		let Some(path) = path.strip_prefix("/novel/") else {
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
