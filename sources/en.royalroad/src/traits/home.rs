use buny::{
	Home, HomeComponent, HomeComponentValue, HomeLayout, HomePartialResult, Listing,
	ListingProvider, Novel, Result,
	alloc::{String, Vec, string::ToString},
	imports::std::send_partial_result,
};

use crate::RoyalRoad;

#[derive(Clone, Copy)]
enum Kind {
	Details,
	Scroller,
	Stack,
	// Rendered by the app at the bottom of the page whatever its position, as an
	// endless grid that pages through the listing itself, so it is sent empty.
	Vertical,
}

// (listing id, title, subtitle, component kind). Ids are the listings handled by
// `get_novel_list`.
const SECTIONS: [(&str, &str, Option<&str>, Kind); 5] = [
	(
		"trending",
		"Trending",
		Some("Popular stories right now"),
		Kind::Details,
	),
	(
		"rising-stars",
		"Rising Stars",
		Some("Newer stories gaining readers fast"),
		Kind::Stack,
	),
	(
		"best-rated",
		"Best Rated",
		Some("The highest rated stories of all time"),
		Kind::Stack,
	),
	("new-releases", "New Releases", None, Kind::Scroller),
	("latest-updates", "Latest Updates", None, Kind::Vertical),
];

fn component(
	kind: Kind,
	title: &str,
	subtitle: Option<&str>,
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
		subtitle: subtitle.map(str::to_string),
		value,
	}
}

impl Home for RoyalRoad {
	fn get_home(&self) -> Result<HomeLayout> {
		send_partial_result(&HomePartialResult::Layout(HomeLayout {
			components: SECTIONS
				.iter()
				.map(|(_, title, subtitle, kind)| {
					component(*kind, title, *subtitle, Vec::new(), None)
				})
				.collect(),
		}));

		// One failed section should not blank the whole page.
		let components = SECTIONS
			.iter()
			.map(|(id, title, subtitle, kind)| {
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
				component(*kind, title, *subtitle, entries, Some(listing))
			})
			.collect();

		Ok(HomeLayout { components })
	}
}
