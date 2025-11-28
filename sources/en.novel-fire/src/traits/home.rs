use buny::{
	Home, HomeComponent, HomeLayout, HomePartialResult, Listing, ListingProvider, Result,
	alloc::{Vec, string::ToString, vec},
	imports::std::send_partial_result,
};

use crate::NovelFire;

// Send initial layout structure
pub fn send_initial_layout() {
	send_partial_result(&HomePartialResult::Layout(HomeLayout {
		components: vec![
			HomeComponent {
				title: Some("Featured".to_string()),
				subtitle: None,
				value: buny::HomeComponentValue::empty_image_scroller(),
			},
			HomeComponent {
				title: Some("Newly Hotted Updates".to_string()),
				subtitle: Some("Hot updates from across the source!".to_string()),
				value: buny::HomeComponentValue::empty_scroller(),
			},
			HomeComponent {
				title: Some("User Ratings".to_string()),
				subtitle: Some("Novels based on user ratings!".to_string()),
				value: buny::HomeComponentValue::empty_details(),
			},
			HomeComponent {
				title: Some("Most Reviewed Novels".to_string()),
				subtitle: Some("Novels based on most reviews!".to_string()),
				value: buny::HomeComponentValue::empty_stack(),
			},
			HomeComponent {
				title: Some("Most Reviewed Novels".to_string()),
				value: buny::HomeComponentValue::empty_vertical(),
				..Default::default()
			},
		],
	}));
}

// use the home trait to implement a home page for a source
// where possible, try to replicate the associated web page's layout
impl Home for NovelFire {
	fn get_home(&self) -> Result<HomeLayout> {
		send_initial_layout();

		let listing = Listing {
			id: "overall-ranking".into(),
			name: "".into(),
			..Default::default()
		};
		let listing2 = Listing {
			id: "ratings".into(),
			name: "".into(),
			..Default::default()
		};

		let listing3 = Listing {
			id: "most-lib".into(),
			name: "".into(),
			..Default::default()
		};

		Ok(HomeLayout {
			components: vec![
				HomeComponent {
					title: Some("Newly Hotted Updates".to_string()),
					subtitle: Some("Hot updates from across the source!".to_string()),
					value: buny::HomeComponentValue::Scroller {
						entries: self
							.get_novel_list(listing.clone(), 1)
							.unwrap_or_default()
							.entries,
						auto_scroll_interval: Some(10.0),
						listing: Some(listing),
						size: 200,
					},
				},
				HomeComponent {
					title: Some("User Ratings".to_string()),
					subtitle: Some("Novels based on user ratings!".to_string()),
					value: buny::HomeComponentValue::Details {
						entries: self
							.get_novel_list(listing2.clone(), 1)
							.unwrap_or_default()
							.entries,
						auto_scroll_interval: Some(10.0),
						listing: Some(listing2),
					},
				},
				HomeComponent {
					title: Some("Most Reviewed Novels".to_string()),
					subtitle: Some("Novels based on most reviews!".to_string()),
					value: buny::HomeComponentValue::Stack {
						entries: self
							.get_novel_list(listing3.clone(), 1)
							.unwrap_or_default()
							.entries,
						auto_scroll_interval: Some(10.0),
						listing: Some(listing3),
					},
				},
				HomeComponent {
					title: Some("Latest Novels".to_string()),
					subtitle: Some("Recently updated novels!".to_string()),
					value: buny::HomeComponentValue::Vertical {
						entries: Vec::new(),
						auto_scroll_interval: Some(10.0),
						listing: Some(Listing {
							id: "most-review".into(),
							name: "".into(),
							..Default::default()
						}),
					},
				},
			],
		})
	}
}
