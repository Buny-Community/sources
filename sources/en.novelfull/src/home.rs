use buny::{
	Home, HomeComponent, HomeComponentValue, HomeLayout, HomePartialResult, Listing,
	ListingProvider, Novel, Result,
	alloc::{String, Vec, string::ToString},
	imports::std::send_partial_result,
};

use crate::NovelFull;

#[derive(Clone, Copy)]
enum Kind {
	Details,
	Stack,
	// Rendered by the app at the bottom of the page whatever its position, as an
	// endless grid that pages through the listing itself, so it is sent empty.
	Vertical,
}

// (listing id, title, component kind). Ids are the listings handled by
// `get_novel_list`. "completed-novel" is left out: its top entries largely
// repeat "most-popular".
const SECTIONS: [(&str, &str, Kind); 3] = [
	("hot-novel", "Hot Novels", Kind::Details),
	("most-popular", "Most Popular", Kind::Stack),
	("latest-release-novel", "Latest Release", Kind::Vertical),
];

fn component(
	kind: Kind,
	title: &str,
	entries: Vec<Novel>,
	listing: Option<Listing>,
) -> HomeComponent {
	let value = match kind {
		Kind::Details => HomeComponentValue::Details {
			entries,
			auto_scroll_interval: Some(10.0),
			listing,
		},
		Kind::Stack => HomeComponentValue::Stack {
			entries,
			auto_scroll_interval: None,
			listing,
		},
		Kind::Vertical => HomeComponentValue::Vertical { entries, listing },
	};
	HomeComponent {
		title: Some(title.to_string()),
		subtitle: None,
		value,
	}
}

impl Home for NovelFull {
	fn get_home(&self) -> Result<HomeLayout> {
		send_partial_result(&HomePartialResult::Layout(HomeLayout {
			components: SECTIONS
				.iter()
				.map(|(_, title, kind)| component(*kind, title, Vec::new(), None))
				.collect(),
		}));

		// One failed section should not blank the whole page.
		let components = SECTIONS
			.iter()
			.map(|(id, title, kind)| {
				let listing = Listing {
					id: String::from(*id),
					name: String::from(*title),
					..Default::default()
				};
				let entries = match kind {
					Kind::Vertical => Vec::new(),
					_ => {
						self.get_novel_list(listing.clone(), 1)
							.unwrap_or_default()
							.entries
					}
				};
				component(*kind, title, entries, Some(listing))
			})
			.collect();

		Ok(HomeLayout { components })
	}
}
