use buny::{
	CheckFilter, DynamicFilters, Filter, MultiSelectFilter, RangeFilter, Result, SelectFilter,
	SortFilter, TextFilter,
	alloc::{Vec, vec},
};

use crate::NovelFire;

// if your source changes filters frequently or only has some filters available conditionally, use the DynamicFilters trait
// where possible, static filters are preferred
impl DynamicFilters for NovelFire {
	fn get_dynamic_filters(&self) -> Result<Vec<Filter>> {
		Ok(vec![
			TextFilter {
				id: "text".into(),
				title: Some("Text".into()),
				placeholder: Some("Search".into()),
				..Default::default()
			}
			.into(),
			SortFilter {
				id: "sort".into(),
				title: Some("Sort".into()),
				can_ascend: true,
				options: vec!["Popular".into(), "Recent".into()],
				..Default::default()
			}
			.into(),
			CheckFilter {
				id: "check".into(),
				title: Some("Check".into()),
				can_exclude: true,
				..Default::default()
			}
			.into(),
			SelectFilter {
				id: "select".into(),
				title: Some("Select".into()),
				uses_tag_style: true,
				options: vec!["One".into(), "Two".into()],
				..Default::default()
			}
			.into(),
			MultiSelectFilter {
				id: "mselect".into(),
				title: Some("Multi-Select".into()),
				can_exclude: true,
				uses_tag_style: false,
				options: vec!["One".into(), "Two".into()],
				..Default::default()
			}
			.into(),
			Filter::note("Testing note"),
			RangeFilter {
				id: "range".into(),
				title: Some("Range".into()),
				min: Some(0.0),
				max: Some(100.0),
				decimal: true,
				..Default::default()
			}
			.into(),
		])
	}
}
