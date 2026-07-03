use buny::{
	DeepLinkHandler, DeepLinkResult, Result,
	alloc::{String, string::ToString},
};

use crate::NovelArchive;

fn query_param(query: &str, key: &str) -> Option<String> {
	query.split('&').find_map(|pair| {
		let (name, value) = pair.split_once('=')?;
		if name == key {
			Some(value.to_string())
		} else {
			None
		}
	})
}

impl DeepLinkHandler for NovelArchive {
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some((path, query)) = url.split_once('?') else {
			return Ok(None);
		};

		if path.ends_with("/reader") {
			let novel_key = query_param(query, "novel");
			let chapter_key = query_param(query, "chapter");
			if let (Some(novel_key), Some(chapter_key)) = (novel_key, chapter_key) {
				return Ok(Some(DeepLinkResult::Chapter {
					novel_key,
					key: chapter_key,
				}));
			}
		} else if path.ends_with("/novel")
			&& let Some(key) = query_param(query, "id").or_else(|| query_param(query, "novel"))
		{
			return Ok(Some(DeepLinkResult::Novel { key }));
		}

		Ok(None)
	}
}
