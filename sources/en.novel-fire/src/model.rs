use buny::alloc::{String, Vec};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct ChapterResponse {
	pub draw: i32,
	pub recordsTotal: i32,
	pub recordsFiltered: i32,
	pub data: Vec<ChapterData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChapterData {
	pub n_sort: i32,
	pub slug: String,
	pub title: String,
	pub created_at: String,
}
