use buny::{
	Home, HomeComponent, HomeComponentValue, HomeLayout, HomePartialResult, Link, LinkValue,
	Listing, Novel, Result,
	alloc::{String, Vec, string::ToString},
	imports::std::send_partial_result,
};

use crate::{API_URL, Chikari, USER_LIST_PREFIX, format};

const LIST_COUNT: usize = 10;

#[derive(Clone, Copy)]
enum Kind {
	Details,
	Scroller,
	Stack,
	// Rendered by the app at the bottom of the page whatever its position, as an
	// endless grid that pages through the listing itself, so it is sent empty.
	Vertical,
}

// (listing id, title, component kind). Ids are /api/novels sort values, or
// "completed" (see `Chikari::listing`).
const SECTIONS: [(&str, &str, Kind); 6] = [
	("trending", "Trending", Kind::Details),
	("popular", "Popular", Kind::Scroller),
	("top_rated", "Top Rated", Kind::Stack),
	("updated", "Latest Updates", Kind::Scroller),
	// Not "most_bookmarked": its top entries largely repeat "popular".
	("completed", "Completed", Kind::Stack),
	("added", "Newest", Kind::Vertical),
];

const LISTS_TITLE: &str = "Popular Lists";
const LISTS_SUBTITLE: &str = "Curated by readers";

fn lists_component(links: Vec<Link>) -> HomeComponent {
	HomeComponent {
		title: Some(LISTS_TITLE.to_string()),
		subtitle: Some(LISTS_SUBTITLE.to_string()),
		value: HomeComponentValue::ImageScroller {
			links,
			auto_scroll_interval: Some(8.0),
			// Covers are 2:3; a list has no banner image of its own.
			width: Some(160),
			height: Some(240),
		},
	}
}

fn novel_component(
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

// The site draws a list's banner as a collage of up to 8 covers. The image
// scroller takes one image, so use the first novel cover; manga covers live
// under /series/ and would misrepresent a novel list.
fn popular_lists() -> Result<Vec<Link>> {
	let json = Chikari::get_json(&format!(
		"{API_URL}/lists?sort=popular&adult=false&medium=novels&limit={LIST_COUNT}&offset=0"
	))?;
	let items = json["items"]
		.as_array()
		.ok_or(buny::error!("Invalid list response"))?;
	Ok(items
		.iter()
		.filter_map(|list| {
			let id = list["id"].as_i64()?;
			let title = list["title"].as_str()?.trim();
			let covers = list["cover_urls"].as_array()?;
			let cover = covers
				.iter()
				.filter_map(|c| c.as_str())
				.find(|c| c.contains("/novels/"))?;
			Some(Link {
				title: title.to_string(),
				subtitle: list["owner"]["username"].as_str().map(str::to_string),
				image_url: Some(cover.to_string()),
				value: Some(LinkValue::Listing(Listing {
					id: format!("{USER_LIST_PREFIX}{id}"),
					name: title.to_string(),
					..Default::default()
				})),
			})
		})
		.collect())
}

impl Home for Chikari {
	fn get_home(&self) -> Result<HomeLayout> {
		let mut placeholders = Vec::with_capacity(SECTIONS.len() + 1);
		placeholders.push(lists_component(Vec::new()));
		placeholders.extend(
			SECTIONS
				.iter()
				.map(|(_, title, kind)| novel_component(*kind, title, Vec::new(), None)),
		);
		send_partial_result(&HomePartialResult::Layout(HomeLayout {
			components: placeholders,
		}));

		// One failed section should not blank the whole page.
		let mut components = Vec::with_capacity(SECTIONS.len() + 1);
		let links = popular_lists().unwrap_or_default();
		if !links.is_empty() {
			components.push(lists_component(links));
		}
		for (id, title, kind) in SECTIONS {
			let listing = Listing {
				id: String::from(id),
				name: String::from(title),
				..Default::default()
			};
			let entries = match kind {
				Kind::Vertical => Vec::new(),
				_ => Self::listing(id, 1).unwrap_or_default().entries,
			};
			components.push(novel_component(kind, title, entries, Some(listing)));
		}

		Ok(HomeLayout { components })
	}
}
