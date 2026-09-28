use buny::{
	DeepLinkHandler, DeepLinkResult, Listing, Result, alloc::String, alloc::string::ToString,
};

use crate::{API_URL, BASE_URL, Chikari, USER_LIST_PREFIX, format};

impl DeepLinkHandler for Chikari {
	// Handles https://chikari.moe/novels/<slug>, https://chikari.moe/novels/<slug>/<number>
	// and https://chikari.moe/lists/<id>-<slug>.
	fn handle_deep_link(&self, url: String) -> Result<Option<DeepLinkResult>> {
		let Some(path) = url
			.strip_prefix(BASE_URL)
			.or_else(|| url.strip_prefix("https://www.chikari.moe"))
		else {
			return Ok(None);
		};
		let path = path.split(['?', '#']).next().unwrap_or("");

		if let Some(rest) = path.strip_prefix("/lists/") {
			let id = rest.split(['-', '/']).next().unwrap_or("");
			if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
				return Ok(None);
			}
			// The title is only cosmetic, so a failed lookup still opens the list.
			let name = Self::get_json(&format!("{API_URL}/lists/{id}"))
				.ok()
				.and_then(|json| json["title"].as_str().map(str::to_string))
				.unwrap_or_else(|| "List".to_string());
			return Ok(Some(DeepLinkResult::Listing(Listing {
				id: format!("{USER_LIST_PREFIX}{id}"),
				name,
				..Default::default()
			})));
		}

		let Some(path) = path.strip_prefix("/novels/") else {
			return Ok(None);
		};
		let mut parts = path.split('/').filter(|p| !p.is_empty());

		let Some(slug) = parts.next() else {
			return Ok(None);
		};
		let result = match parts.next() {
			Some(number) if number.parse::<f64>().is_ok() => DeepLinkResult::Chapter {
				novel_key: slug.to_string(),
				key: number.to_string(),
			},
			Some(_) => return Ok(None),
			None => DeepLinkResult::Novel {
				key: slug.to_string(),
			},
		};
		Ok(Some(result))
	}
}
