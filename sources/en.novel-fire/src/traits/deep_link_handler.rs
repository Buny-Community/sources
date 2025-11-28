use buny::{DeepLinkHandler, DeepLinkResult, Result, alloc::String};

use crate::NovelFire;

impl DeepLinkHandler for NovelFire {
	fn handle_deep_link(&self, _url: String) -> Result<Option<DeepLinkResult>> {
		Ok(Some(DeepLinkResult::Novel {
			key: String::from("novel_key"),
		}))
	}
}
