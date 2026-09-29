use buny::{Listing, ListingProvider, NovelPageResult, Result};

use crate::WeTriedTLS;

impl ListingProvider for WeTriedTLS {
	// Listing ids map to /query orderBy values (see `WeTriedTLS::listing`).
	fn get_novel_list(&self, listing: Listing, page: i32) -> Result<NovelPageResult> {
		Self::listing(&listing.id, page)
	}
}
