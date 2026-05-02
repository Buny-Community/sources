#![no_std]
use buny::{
	Chapter, ContentBlock, ContentRating, FilterValue, Listing, ListingProvider,
	Novel, NovelPageResult, Result, Source,
	alloc::{String, Vec, borrow::Cow},
};

mod helpers;
mod imp;

pub use imp::Impl;

pub struct Params {
	pub base_url: Cow<'static, str>,
	pub api_url: Cow<'static, str>,
	pub novel_path: Cow<'static, str>,
	pub use_slug_search: bool,
	pub default_rating: ContentRating,
	pub date_format: Cow<'static, str>,
}

impl Default for Params {
	fn default() -> Self {
		Self {
			base_url: "".into(),
			api_url: "".into(),
			novel_path: "novel".into(),
			use_slug_search: false,
			default_rating: ContentRating::default(),
			date_format: "MMM dd, yyyy".into(),
		}
	}
}

pub struct MadTheme<T: Impl> {
	inner: T,
	params: Params,
}

impl<T: Impl> Source for MadTheme<T> {
	fn new() -> Self {
		let inner = T::new();
		let params = inner.params();
		Self { inner, params }
	}

	fn get_search_novel_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<NovelPageResult> {
		self.inner
			.get_search_novel_list(&self.params, query, page, filters)
	}

	fn get_novel_update(
		&self,
		novel: Novel,
		needs_details: bool,
		needs_chapters: bool,
		page: i32,
	) -> Result<Novel> {
		self.inner
			.get_novel_update(&self.params, novel, needs_details, needs_chapters, page)
	}

	fn get_chapter_content_list(
		&self,
		novel: Novel,
		chapter: Chapter,
	) -> Result<Vec<ContentBlock>> {
		self.inner
			.get_chapter_content_list(novel, chapter, &self.params)
	}
}

impl<T: Impl> ListingProvider for MadTheme<T> {
	fn get_novel_list(&self, listing: Listing, page: i32) -> Result<NovelPageResult> {
		self.inner.get_novel_list(&self.params, listing, page)
	}
}
