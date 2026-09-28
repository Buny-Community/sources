use buny::{Listing, ListingProvider, NovelPageResult, Result};

use crate::Chikari;

impl ListingProvider for Chikari {
	// Listing ids are the API's own sort values (see res/source.json).
	fn get_novel_list(&self, listing: Listing, page: i32) -> Result<NovelPageResult> {
		Self::listing(&listing.id, page)
	}
}
