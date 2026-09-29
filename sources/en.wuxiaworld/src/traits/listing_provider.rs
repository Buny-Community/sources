use buny::{Listing, ListingProvider, NovelPageResult, Result};

use crate::Wuxiaworld;

impl ListingProvider for Wuxiaworld {
	// Listing ids map to SearchNovels sorts (see `Wuxiaworld::listing`).
	fn get_novel_list(&self, listing: Listing, page: i32) -> Result<NovelPageResult> {
		Self::listing(&listing.id, page)
	}
}
