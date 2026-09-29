use buny::{
	DeepLinkHandler, DeepLinkResult, Result, alloc::String, alloc::string::ToString, prelude::*,
};
use serde_json::Value;

use crate::{API_URL, BASE_URL, FenrirRealm};

impl DeepLinkHandler for FenrirRealm {
	// Handles https://fenrirealm.com/series/<slug> and chapter pages,
	// https://fenrirealm.com/series/<slug>/<chapter-slug>, where the chapter slug
	// can include a volume ("vol-1/42-1"). Chapters are keyed by their numeric
	// id, which only the chapter list has, so it is looked up there.
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = url
			.strip_prefix(BASE_URL)
			.or_else(|| url.strip_prefix("https://www.fenrirealm.com"))
			.or_else(|| url.strip_prefix("http://fenrirealm.com"))
		else {
			return Ok(None);
		};
		let path = path.split(['?', '#']).next().unwrap_or("");
		let Some(path) = path.strip_prefix("/series/") else {
			return Ok(None);
		};
		let path = path.trim_end_matches('/');
		let (slug, chapter_slug) = match path.split_once('/') {
			Some((slug, chapter)) => (slug, Some(chapter)),
			None => (path, None),
		};
		if slug.is_empty() {
			return Ok(None);
		}
		let Some(chapter_slug) = chapter_slug else {
			return Ok(Some(DeepLinkResult::Novel {
				key: slug.to_string(),
			}));
		};

		let chapters = Self::get_json(&format!("{API_URL}/series/{slug}/chapters"))?;
		let id = chapters
			.as_array()
			.into_iter()
			.flatten()
			.find(|ch| ch["slug"].as_str() == Some(chapter_slug))
			.and_then(|ch: &Value| ch["id"].as_i64());
		Ok(Some(match id {
			Some(id) => DeepLinkResult::Chapter {
				novel_key: slug.to_string(),
				key: id.to_string(),
			},
			// An unknown chapter still opens the series.
			None => DeepLinkResult::Novel {
				key: slug.to_string(),
			},
		}))
	}
}
