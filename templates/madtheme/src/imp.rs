use crate::{Params, helpers};
use buny::{
	Chapter, ContentBlock, ContentRating, FilterValue, Listing, Novel, NovelPageResult,
	NovelStatus, Result,
	alloc::{String, Vec, string::ToString},
	helpers::{string::StripPrefixOrSelf, uri::QueryParameters},
	imports::{
		html::Document,
		net::Request,
		std::{current_date, parse_date, send_partial_result},
	},
	prelude::*,
};

pub trait Impl {
	fn new() -> Self;

	fn params(&self) -> Params;

	fn get_search_novel_list(
		&self,
		params: &Params,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<NovelPageResult> {
		let mut qs = QueryParameters::new();

		qs.set("page", Some(&page.to_string()));
		if let Some(q) = query.as_deref()
			&& !q.is_empty()
		{
			qs.set("q", Some(q));
		}
		qs.set("status", Some("all"));

		for filter in filters {
			match filter {
				FilterValue::Sort { id, index, .. } => {
					let value = match index {
						0 => "",
						1 => "latest",
						2 => "popular",
						3 => "rating",
						4 => "views",
						5 => "chapters",
						6 => "alphabetical",
						_ => "",
					};
					if value.is_empty() {
						qs.set(&id, None);
					} else {
						qs.set(&id, Some(value));
					}
				}
				FilterValue::Select { id, value } => {
					if value == "all" {
						qs.set(&id, None);
					} else {
						qs.set(&id, Some(&value));
					}
				}
				FilterValue::MultiSelect {
					included, excluded, ..
				} => {
					if !included.is_empty() {
						let included_str = included.join(",");
						qs.push("genres", Some(&included_str));
					}
					if !excluded.is_empty() {
						let excluded_str = excluded.join(",");
						qs.push("exclude", Some(&excluded_str));
					}
				}
				_ => {}
			}
		}

		let url = format!("{}/titles/search?{}", params.api_url, qs);
		let text = Request::get(&url)?.string()?;
		let json: serde_json::Value =
			serde_json::from_str(&text).map_err(|_| error!("Invalid JSON"))?;

		Ok(NovelPageResult {
			entries: json["data"]["items"]
				.as_array()
				.ok_or(error!("Invalid items array"))?
				.iter()
				.map(|item| {
					let link = item["url"].as_str().unwrap_or("");
					let name = item["name"].as_str().unwrap_or("");

					Novel {
						key: link.strip_prefix('/').unwrap_or(link).to_string(),
						title: name.to_string(),
						cover: item["cover"].as_str().map(|s| s.to_string()),
						..Default::default()
					}
				})
				.collect(),

			has_next_page: json["data"]["pagination"]["has_next"]
				.as_bool()
				.unwrap_or(false),
		})
	}

	fn get_novel_update(
		&self,
		params: &Params,
		mut novel: Novel,
		needs_details: bool,
		needs_chapters: bool,
		_page: i32,
	) -> Result<Novel> {
		let novel_url = format!("{}/{}", params.base_url, novel.key);
		let html = Request::get(&novel_url)?.html()?;

		if needs_details {
			novel.title = html
				.select_first("h1")
				.and_then(|h1| h1.text())
				.unwrap_or(novel.title);
			novel.cover = html
				.select_first("img[alt]")
				.and_then(|img| img.attr("abs:src"))
				.or(novel.cover);
			novel.authors = html.select("a[href*='/authors/']").map(|els| {
				els.filter_map(|el| el.text())
					.map(|s| s.trim().trim_end_matches(',').trim().into())
					.collect()
			});
			novel.description = html.select_first("p").and_then(|el| el.text());

			novel.tags = html.select("a[href*='/genres/']").map(|els| {
				els.filter_map(|el| el.text())
					.map(|s| s.trim().trim_end_matches(',').into())
					.collect()
			});
			novel.status = html
				.select("span")
				.and_then(|els| {
					els.filter_map(|el| el.text())
						.map(|t| t.trim().to_lowercase())
						.find_map(|t| {
							Some(match t.as_str() {
								"ongoing" => NovelStatus::Ongoing,
								"completed" => NovelStatus::Completed,
								"on-hold" => NovelStatus::Hiatus,
								"canceled" => NovelStatus::Cancelled,
								_ => return None,
							})
						})
				})
				.unwrap_or_default();
			let tags = novel.tags.as_deref().unwrap_or(&[]);
			novel.content_rating = if tags
				.iter()
				.any(|e| matches!(e.as_str(), "Adult" | "Hentai" | "Mature" | "Smut"))
			{
				ContentRating::NSFW
			} else if tags.iter().any(|e| e == "Ecchi") {
				ContentRating::Suggestive
			} else if params.default_rating != ContentRating::Unknown {
				params.default_rating
			} else {
				ContentRating::Safe
			};

			send_partial_result(&novel);
		}

		if needs_chapters {
			fn parse_chapter_elements(html: &Document, params: &Params) -> Vec<Chapter> {
				html.select("ul a[href*='/chapter-']")
					.map(|els| {
						els.filter_map(|el| {
							let link = el.attr("href")?;
							let title = el
								.select("span")
								.and_then(|els| {
									els.filter_map(|el| el.text())
										.find(|t| t.trim().starts_with("Chapter"))
								})
								.or_else(|| el.text())?;

							let chapter_number = helpers::find_first_f32(&title);

							let date_uploaded = el
								.select("span")
								.and_then(|els| {
									els.filter_map(|el| el.text()).find(|t| t.contains("ago"))
								})
								.map(|s| {
									parse_date(s, &params.date_format).unwrap_or(current_date())
								});

							Some(Chapter {
								key: link.strip_prefix_or_self(&params.base_url).into(),
								title: Some(title),
								chapter_number,
								date_uploaded,
								url: Some(if link.starts_with("http") {
									link
								} else {
									format!("{}{}", params.base_url, link)
								}),
								..Default::default()
							})
						})
						.collect()
					})
					.unwrap_or_default()
			}
			novel.chapters = None;
			send_partial_result(&novel);

			let chapters = (|| -> Result<Vec<Chapter>> {
				let data = html
					.select_first("script#__NEXT_DATA__")
					.and_then(|el| el.data())
					.ok_or(error!("Cannot find __NEXT_DATA__ script"))?;

				let next_data: serde_json::Value = serde_json::from_str(&data)
					.map_err(|_| error!("Invalid __NEXT_DATA__ JSON"))?;

				let page_props = &next_data["props"]["pageProps"];
				let id = page_props["mangaHsid"]
					.as_str()
					.ok_or(error!("mangaHsid not found"))?;
				let cv = page_props["initialManga"]["cv"].as_u64();

				let url = if let Some(cv) = cv {
					format!("{}/titles/{}/chapters?cv={}", params.api_url, id, cv)
				} else {
					format!("{}/titles/{}/chapters", params.api_url, id)
				};

				let text = Request::get(&url)?.string()?;

				let json: serde_json::Value =
					serde_json::from_str(&text).map_err(|_| error!("Invalid JSON"))?;

				let mut chapters: Vec<Chapter> = json["data"]["chapters"]
					.as_array()
					.ok_or(error!("Invalid chapters array"))?
					.iter()
					.map(|ch| {
						let name = ch["name"].as_str().unwrap_or("").to_string();
						let link = ch["url"].as_str().unwrap_or("");

						Chapter {
							key: link.strip_prefix_or_self(&params.base_url).into(),
							title: Some(name.clone()),
							chapter_number: helpers::find_first_f32(&name),
							date_uploaded: ch["updated_at"]
								.as_str()
								.and_then(|d| parse_date(d, &params.date_format)),
							url: Some(format!("{}{}", params.base_url, link)),
							..Default::default()
						}
					})
					.collect();

				for chapter in parse_chapter_elements(&html, params) {
					if !chapters.iter().any(|existing| existing.key == chapter.key) {
						chapters.push(chapter);
					}
				}

				chapters.reverse();
				Ok(chapters)
			})()
			.unwrap_or_else(|_| {
				let mut chapters = parse_chapter_elements(&html, params);
				chapters.reverse();
				chapters
			});

			novel.chapters = Some(chapters);
		}

		Ok(novel)
	}

	fn get_chapter_content_list(
		&self,
		_novel: Novel,
		chapter: Chapter,
		params: &Params,
	) -> Result<Vec<ContentBlock>> {
		let url = format!("{}{}", params.base_url, chapter.key);
		let html = Request::get(&url)?.html()?;

		let mut content_list: Vec<ContentBlock> = html
			.select(".novel-tts-content p")
			.map(|els| {
				els.filter_map(|content_node| {
					let content = content_node.text()?.to_string();

					if content.starts_with('[') && content.ends_with(']') {
						let mut quote = content.chars();
						quote.next();
						quote.next_back();
						quote.as_str().to_string();

						return Some(ContentBlock::block_quote(quote.as_str().to_string()));
					} else if content == "***" {
						return Some(ContentBlock::Divider);
					}
					Some(ContentBlock::paragraph(content, None))
				})
				.collect()
			})
			.unwrap_or_default();

		let review_link = format!("LINK: [click here for chapter reviews.]({})", url);
		content_list.push(ContentBlock::Divider);
		content_list.push(ContentBlock::paragraph(review_link, None));
		Ok(content_list)
	}

	fn get_novel_list(
		&self,
		params: &Params,
		listing: Listing,
		page: i32,
	) -> Result<NovelPageResult> {
		let url = format!("{}/{}?page={}", params.base_url, listing.id, page);
		let html = Request::get(&url)?.html()?;
		let has_next_page = html
			.select("button")
			.map(|mut buttons| {
				buttons.any(|el| {
					el.text()
						.map(|t| t.trim().contains("Load More"))
						.unwrap_or(false)
				})
			})
			.unwrap_or(false);

		let entries: Vec<Novel> = html
			.select(".flex.flex-col.h-full")
			.map(|els| {
				els.filter_map(|el| {
					let link = el
						.select_first("a[aria-label][href]")
						.or_else(|| el.select_first("a[title][href]"))
						.or_else(|| el.select_first("a[href]:not([href*='chapter'])"))?;
					let href = link.attr("href")?;

					let key: String = href
						.strip_prefix_or_self(&params.base_url)
						.strip_prefix_or_self(format!("/{}/", &params.novel_path).as_str())
						.strip_prefix_or_self("/")
						.into();

					let cover = el.select_first("img").and_then(|img| {
						img.attr("abs:data-src")
							.or_else(|| img.attr("data-src"))
							.or_else(|| img.attr("abs:src"))
							.or_else(|| img.attr("src"))
					});

					let title = link
						.attr("title")
						.or_else(|| link.attr("aria-label"))
						.or_else(|| el.select_first("img[alt]").and_then(|img| img.attr("alt")))
						.or_else(|| el.select_first("a[title]").and_then(|el| el.attr("title")))
						.or_else(|| {
							link.text()
								.and_then(|text| (!text.trim().is_empty()).then_some(text))
						})
						.or_else(|| el.select_first("h1, h2, h3, h4").and_then(|el| el.text()))?;

					Some(Novel {
						key,
						title,
						cover,
						..Default::default()
					})
				})
				.collect()
			})
			.unwrap_or_default();

		Ok(NovelPageResult {
			entries,
			has_next_page,
		})
	}
}
