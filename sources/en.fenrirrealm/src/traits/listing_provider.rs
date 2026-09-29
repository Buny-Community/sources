use buny::{Listing, ListingProvider, NovelPageResult, Result};

use crate::FenrirRealm;

impl ListingProvider for FenrirRealm {
	// Listing ids are /series sort values (see `FenrirRealm::listing`).
	fn get_novel_list(&self, listing: Listing, page: i32) -> Result<NovelPageResult> {
		Self::listing(&listing.id, page)
	}
}
