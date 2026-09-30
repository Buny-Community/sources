use buny::{
	DeepLinkHandler, DeepLinkResult, Result, alloc::String, alloc::string::ToString, prelude::*,
};

use crate::{ScribbleHub, chapter_id, novel_key};

impl DeepLinkHandler for ScribbleHub {
	// Handles series pages, https://www.scribblehub.com/series/<id>/<slug>/, and
	// chapter pages, https://www.scribblehub.com/read/<id>-<slug>/chapter/<chapter id>/.
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = ["https://www.scribblehub.com", "https://scribblehub.com"]
			.iter()
			.find_map(|base| url.strip_prefix(base))
		else {
			return Ok(None);
		};
		let path = path.split(['?', '#']).next().unwrap_or("");
		if path.starts_with("/series/") {
			return Ok(novel_key(path).map(|key| DeepLinkResult::Novel { key }));
		}
		let Some(rest) = path.strip_prefix("/read/") else {
			return Ok(None);
		};
		// "117137-the-runesmith/chapter/2508376/"
		let Some((series, _)) = rest.split_once('/') else {
			return Ok(None);
		};
		let Some((id, slug)) = series.split_once('-') else {
			return Ok(None);
		};
		if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
			return Ok(None);
		}
		let novel_key = format!("{id}/{slug}");
		Ok(Some(match chapter_id(path) {
			Some(key) => DeepLinkResult::Chapter {
				novel_key,
				key: key.to_string(),
			},
			None => DeepLinkResult::Novel { key: novel_key },
		}))
	}
}
