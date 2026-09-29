use buny::{
	Home, HomeComponent, HomeComponentValue, HomeLayout, HomePartialResult, Listing, Novel, Result,
	alloc::{String, Vec, string::ToString},
	imports::std::send_partial_result,
};

use crate::Wuxiaworld;

#[derive(Clone, Copy)]
enum Kind {
	Details,
	Scroller,
	Stack,
	// Rendered by the app at the bottom of the page whatever its position, as an
	// endless grid that pages through the listing itself, so it is sent empty.
	Vertical,
}

// (listing id, title, component kind). The five sorts share at most 3 novels
// between any two top 10s. "Most chapters" is left out: it repeats popular.
const SECTIONS: [(&str, &str, Kind); 5] = [
	("trending", "Trending", Kind::Details),
	("top_rated", "Top Rated", Kind::Stack),
	("new", "New Releases", Kind::Scroller),
	("completed", "Completed", Kind::Stack),
	("popular", "Popular", Kind::Vertical),
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
		// Size stays 0: BunyRunner decodes this field as an Option, so any other
		// value misdecodes (Reader open bug #18).
		Kind::Scroller => HomeComponentValue::Scroller {
			entries,
			auto_scroll_interval: None,
			listing,
			size: 0,
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

impl Home for Wuxiaworld {
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
				let entries = match kind {
					Kind::Vertical => Vec::new(),
					_ => Self::listing(id, 1).unwrap_or_default().entries,
				};
				let listing = Listing {
					id: String::from(*id),
					name: String::from(*title),
					..Default::default()
				};
				component(*kind, title, entries, Some(listing))
			})
			.collect();

		Ok(HomeLayout { components })
	}
}
