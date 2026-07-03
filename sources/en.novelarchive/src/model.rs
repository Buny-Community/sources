use buny::alloc::{String, Vec};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct NovelSummary {
	pub id: String,
	pub title: String,
	#[serde(default)]
	pub author: Option<String>,
	#[serde(default)]
	pub genres: Option<String>,
	#[serde(default)]
	pub cover_url: Option<String>,
	#[serde(default)]
	pub release_status: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NovelListPagination {
	pub has_next: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NovelListResponse {
	pub novels: Vec<NovelSummary>,
	#[serde(default)]
	pub pagination: Option<NovelListPagination>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NovelDetail {
	pub title: String,
	#[serde(default)]
	pub author: Option<String>,
	#[serde(default)]
	pub genres: Option<String>,
	#[serde(default)]
	pub cover_url: Option<String>,
	#[serde(default)]
	pub description: Option<String>,
	#[serde(default)]
	pub release_status: Option<String>,
	#[serde(default)]
	pub chapter_names: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NovelDetailResponse {
	pub novel: NovelDetail,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChapterData {
	pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChapterResponse {
	pub chapter: ChapterData,
}
