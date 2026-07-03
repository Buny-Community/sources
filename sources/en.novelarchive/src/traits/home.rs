use buny::{
	Home, HomeComponent, HomeLayout, HomePartialResult, Listing, Novel, Result,
	alloc::{String, Vec},
	imports::{net::Request, std::send_partial_result},
	prelude::*,
};

use crate::{API_BASE, NovelArchive, model::NovelListResponse, novel_summary_to_novel};

const HOME_ENDPOINTS: [(&str, &str); 4] = [
	("trending", "Trending"),
	("editors-choice", "Editor's Choice"),
	("recently-updated", "Recently Updated"),
	("recent", "Recently Added"),
];

// use the home trait to implement a home page for a source
// where possible, try to replicate the associated web page's layout
impl Home for NovelArchive {
	fn get_home(&self) -> Result<HomeLayout> {
		// show the section titles immediately instead of leaving the screen
		// blank while the requests below are in flight
		send_partial_result(&HomePartialResult::Layout(HomeLayout {
			components: HOME_ENDPOINTS
				.iter()
				.map(|(_, title)| HomeComponent {
					title: Some(String::from(*title)),
					value: buny::HomeComponentValue::empty_scroller(),
					..Default::default()
				})
				.collect(),
		}));

		// fire all of the home section requests together instead of one at a
		// time, since sequential requests would otherwise add up to several
		// seconds of load time
		let requests = HOME_ENDPOINTS
			.iter()
			.filter_map(|(endpoint, _)| {
				Request::get(format!("{}/novels/{}?limit=50", API_BASE, endpoint)).ok()
			})
			.collect::<Vec<_>>();
		let responses = Request::send_all(requests);

		let components = HOME_ENDPOINTS
			.iter()
			.zip(responses)
			.map(|((id, title), response)| {
				let entries: Vec<Novel> = response
					.ok()
					.and_then(|r| r.get_json_owned::<NovelListResponse>().ok())
					.map(|list| {
						list.novels
							.into_iter()
							.map(novel_summary_to_novel)
							.collect()
					})
					.unwrap_or_default();

				let listing = Listing {
					id: String::from(*id),
					name: String::from(*title),
					..Default::default()
				};

				HomeComponent {
					title: Some(String::from(*title)),
					value: buny::HomeComponentValue::Scroller {
						entries,
						auto_scroll_interval: if *id == "trending" { Some(10.0) } else { None },
						listing: Some(listing),
						size: 200,
					},
					..Default::default()
				}
			})
			.collect();

		Ok(HomeLayout { components })
	}
}
