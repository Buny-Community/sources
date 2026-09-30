use buny::{Listing, ListingProvider, NovelPageResult, Result};

use crate::ScribbleHub;

impl ListingProvider for ScribbleHub {
	// Listing ids are the ones `ScribbleHub::listing` knows.
	fn get_novel_list(&self, listing: Listing, page: i32) -> Result<NovelPageResult> {
		Self::listing(&listing.id, page)
	}
}
