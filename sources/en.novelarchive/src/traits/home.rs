use buny::{
	Home, HomeComponent, HomeComponentValue, HomeLayout, HomePartialResult, Listing, Novel, Result,
	alloc::{String, Vec},
	imports::{net::Request, std::send_partial_result},
	prelude::*,
};

use crate::{API_BASE, NovelArchive, model::NovelListResponse, novel_summary_to_novel};

#[derive(Clone, Copy, PartialEq)]
enum Kind {
	Details,
	Scroller,
	// Rendered by the app at the bottom of the page whatever its position, as a
	// grid that loads the listing itself, so it is sent empty.
	Vertical,
}

// (endpoint / listing id, title, component kind)
const SECTIONS: [(&str, &str, Kind); 4] = [
	("trending", "Trending", Kind::Details),
	("editors-choice", "Editor's Choice", Kind::Scroller),
	("recently-updated", "Recently Updated", Kind::Scroller),
	("recent", "Recently Added", Kind::Vertical),
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
		Kind::Vertical => HomeComponentValue::Vertical { entries, listing },
	};
	HomeComponent {
		title: Some(String::from(title)),
		subtitle: None,
		value,
	}
}

// use the home trait to implement a home page for a source
// where possible, try to replicate the associated web page's layout
impl Home for NovelArchive {
	fn get_home(&self) -> Result<HomeLayout> {
		// show the section titles immediately instead of leaving the screen
		// blank while the requests below are in flight
		send_partial_result(&HomePartialResult::Layout(HomeLayout {
			components: SECTIONS
				.iter()
				.map(|(_, title, kind)| component(*kind, title, Vec::new(), None))
				.collect(),
		}));

		// fire all of the home section requests together instead of one at a
		// time, since sequential requests would otherwise add up to several
		// seconds of load time
		let fetched: Vec<_> = SECTIONS
			.iter()
			.filter(|(_, _, kind)| *kind != Kind::Vertical)
			.collect();
		// responses are matched to sections by position, so a request that fails
		// to build must not be skipped
		let requests = fetched
			.iter()
			.map(|(endpoint, _, _)| {
				Request::get(format!("{}/novels/{}?limit=50", API_BASE, endpoint))
			})
			.collect::<core::result::Result<Vec<_>, _>>()?;
		let mut responses = Request::send_all(requests).into_iter();

		let components = SECTIONS
			.iter()
			.map(|(id, title, kind)| {
				// One failed section should not blank the whole page.
				let entries: Vec<Novel> = if *kind == Kind::Vertical {
					Vec::new()
				} else {
					responses
						.next()
						.and_then(|r| r.ok())
						.and_then(|r| r.get_json_owned::<NovelListResponse>().ok())
						.map(|list| {
							list.novels
								.into_iter()
								.map(novel_summary_to_novel)
								.collect()
						})
						.unwrap_or_default()
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
