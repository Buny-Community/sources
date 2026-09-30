#![no_std]
use buny::{
	Chapter, ContentBlock, ContentRating, FilterValue, Novel, NovelPageResult, NovelStatus, Result,
	Source,
	alloc::{String, Vec, string::ToString, vec},
	helpers::uri::QueryParameters,
	imports::{
		html::{Document, Element, Html},
		net::{Request, Response, TimeUnit, set_rate_limit},
		std::{current_date, parse_date, send_partial_result},
	},
	prelude::*,
};

mod content;
mod traits;

const BASE_URL: &str = "https://www.scribblehub.com";

// Series finder sort values, in the order of the sort filter's options. An
// unknown value silently falls back to the default order.
const SORT_VALUES: [&str; 12] = [
	"pageviews",
	"lastchpdate",
	"dateadded",
	"ratings",
	"numofrate",
	"favorites",
	"readers",
	"reviews",
	"chapters",
	"frequency",
	"pages",
	"totalwords",
];

struct ScribbleHub;

// Cloudflare challenges requests that don't look like a browser's: chapter
// pages without a Referer, and the chapter list's POST without an Origin
// (both checked with curl over HTTP/1.1, 2026-09-30). A browser sends both, so
// every request does. The User-Agent is left to the host: the app sends its
// WebView's, which its Cloudflare handler also uses to get clearance.
fn get(url: &str) -> Result<Request> {
	Ok(Request::get(url)?.header("Referer", &format!("{BASE_URL}/")))
}

fn send(request: Request) -> Result<Response> {
	let response = request.send()?;
	match response.status_code() {
		200..=299 => Ok(response),
		404 => bail!("Not found on Scribble Hub"),
		// Reading chapters fast (about 8 in a row) gets 429 with a Cloudflare
		// challenge page. The app's Cloudflare handler solves it for 403, 429 and
		// 503 before the source sees the response; this is what's left over.
		429 => bail!("Scribble Hub is limiting how fast pages load. Try again in a minute."),
		403 | 503 if response.get_header("cf-mitigated").is_some() => {
			bail!("Scribble Hub's Cloudflare check blocked the request. Try again later.")
		}
		code => bail!("Scribble Hub returned HTTP {code}"),
	}
}

fn get_text(url: &str) -> Result<String> {
	send(get(url)?)?.get_string()
}

fn parse(text: &str) -> Result<Document> {
	Html::parse_with_url(text, BASE_URL).map_err(|_| error!("Invalid page"))
}

impl ScribbleHub {
	// Series finder and ranking pages: 25 series per page as `.search_main_box`
	// (a `mb` variant for mobile user agents, with the same inner classes).
	fn novel_page(url: &str) -> Result<NovelPageResult> {
		parse_novel_page(&get_text(url)?)
	}

	pub(crate) fn listing(id: &str, page: i32) -> Result<NovelPageResult> {
		let page = page.max(1);
		let url = match id {
			// The home page's "Trending" (ranking by "rising", daily).
			"trending" => format!("{BASE_URL}/series-ranking/?sort=5&order=1&pg={page}"),
			// Ranking by popularity, all time. Same order as the finder by pageviews.
			"popular" => format!("{BASE_URL}/series-ranking/?sort=1&order=4&pg={page}"),
			"monthly" => format!("{BASE_URL}/series-ranking/?sort=1&order=3&pg={page}"),
			"latest" => {
				format!("{BASE_URL}/series-finder/?sf=1&sort=lastchpdate&order=desc&pg={page}")
			}
			"new" => format!("{BASE_URL}/latest-series/?pg={page}"),
			"completed" => format!(
				"{BASE_URL}/series-finder/?sf=1&cp=completed&sort=pageviews&order=desc&pg={page}"
			),
			_ => bail!("Unknown listing: {id}"),
		};
		Self::novel_page(&url)
	}

	// Every chapter comes in one reply (`pagenum=-1`, ~270 bytes each), newest
	// first. Each item's `order` is its 1-based position in reading order.
	fn chapters(novel_key: &str) -> Result<Vec<Chapter>> {
		let id = novel_id(novel_key);
		let request = Request::post(format!("{BASE_URL}/wp-admin/admin-ajax.php"))?
			.header(
				"Content-Type",
				"application/x-www-form-urlencoded; charset=UTF-8",
			)
			.header("Origin", BASE_URL)
			.header("Referer", &format!("{BASE_URL}/series/{novel_key}/"))
			.body(format!(
				"action=wi_getreleases_pagination&pagenum=-1&mypostid={id}"
			));
		parse_chapter_list(&send(request)?.get_string()?)
	}
}

impl Source for ScribbleHub {
	fn new() -> Self {
		// Cloudflare limits reads: 15 chapter pages in a row, or about 38 at 10 a
		// minute, then 429 with a challenge page, and it takes over 90 seconds to
		// lift (curl, 2026-09-30). The host queues requests past this limit, so
		// downloading many chapters waits instead of failing. It counts every
		// request; the home page's four lists fit in it.
		set_rate_limit(8, 60, TimeUnit::Seconds);
		Self
	}

	fn get_search_novel_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<NovelPageResult> {
		let mut qs = QueryParameters::new();
		qs.push("sf", Some("1"));
		// "Title contains", the finder's own search box.
		if let Some(query) = query.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
			qs.push("sh", Some(query));
		}
		let mut sort = SORT_VALUES[0];
		let mut ascending = false;
		// (filter id, included, excluded) and (mode filter id, value).
		let mut lists: Vec<(String, Vec<String>, Vec<String>)> = Vec::new();
		let mut modes: Vec<(String, String)> = Vec::new();

		for filter in filters {
			match filter {
				FilterValue::Sort {
					index,
					ascending: asc,
					..
				} => {
					if let Some(value) = SORT_VALUES.get(index as usize) {
						sort = value;
					}
					ascending = asc;
				}
				FilterValue::Select { id, value } => {
					if id == "status" {
						if value != "all" {
							qs.push("cp", Some(&value));
						}
					} else {
						modes.push((id, value));
					}
				}
				FilterValue::MultiSelect {
					id,
					included,
					excluded,
				} => lists.push((id, included, excluded)),
				_ => {}
			}
		}
		// Lists are comma-joined ids ("gi=9,19"; `%2C` works the same). The match
		// mode is only sent with included values, as the site does.
		for (id, included, excluded) in lists {
			let (include, exclude, mode) = match id.as_str() {
				"genres" => ("gi", "ge", "mgi"),
				"warnings" => ("cti", "cte", "mct"),
				"tags" => ("tgi", "tge", "mtgi"),
				_ => continue,
			};
			if !included.is_empty() {
				qs.push(include, Some(&included.join(",")));
				let mode_id = format!("{id}_mode");
				let value = modes
					.iter()
					.find(|(m, _)| *m == mode_id)
					.map_or("or", |(_, v)| v.as_str());
				qs.push(mode, Some(value));
			}
			if !excluded.is_empty() {
				qs.push(exclude, Some(&excluded.join(",")));
			}
		}
		qs.push("sort", Some(sort));
		qs.push("order", Some(if ascending { "asc" } else { "desc" }));
		qs.push("pg", Some(&page.max(1).to_string()));

		Self::novel_page(&format!("{BASE_URL}/series-finder/?{qs}"))
	}

	fn get_novel_update(
		&self,
		mut novel: Novel,
		needs_details: bool,
		needs_chapters: bool,
		_page: i32,
	) -> Result<Novel> {
		if needs_details {
			let url = novel_url(&novel.key);
			apply_details(&mut novel, &get_text(&url)?)?;
			novel.url = Some(url);

			if needs_chapters {
				send_partial_result(&novel);
			}
		}

		if needs_chapters {
			novel.chapters = Some(Self::chapters(&novel.key)?);
			novel.has_more_chapters = Some(false);
		}

		Ok(novel)
	}

	fn get_chapter_content_list(
		&self,
		novel: Novel,
		chapter: Chapter,
	) -> Result<Vec<ContentBlock>> {
		let url = chapter
			.url
			.clone()
			.filter(|u| u.starts_with(BASE_URL))
			.unwrap_or_else(|| chapter_url(&novel.key, &chapter.key));
		let html = parse(&get_text(&url)?)?;
		let body = html
			.select_first("#chp_raw")
			.and_then(|e| e.html())
			.ok_or(error!("Chapter has no text"))?;
		let name = html
			.select_first(".chapter-title")
			.and_then(|e| e.text())
			.unwrap_or_default();
		let series = html
			.select_first(".chp_byauthor a")
			.and_then(|e| e.text())
			.unwrap_or_else(|| novel.title.clone());
		let blocks = content::html_blocks(
			&body,
			&content::Headings {
				name: &name,
				series: &series,
			},
		);
		if blocks.is_empty() {
			bail!("Chapter has no text");
		}
		Ok(blocks)
	}
}

fn parse_novel_page(text: &str) -> Result<NovelPageResult> {
	let html = parse(text)?;
	let entries = html
		.select(".search_main_box")
		.map(|boxes| boxes.filter_map(|el| parse_list_item(&el)).collect())
		.unwrap_or_default();
	Ok(NovelPageResult {
		entries,
		has_next_page: pagination(text).is_some_and(|(current, last)| current < last),
	})
}

fn parse_chapter_list(text: &str) -> Result<Vec<Chapter>> {
	let html = parse(text)?;
	let mut items: Vec<(i32, String, String, Option<i64>)> = html
		.select(".toc_w")
		.map(|els| {
			els.filter_map(|el| {
				let order = el.attr("order")?.trim().parse::<i32>().ok()?;
				let link = el.select_first("a.toc_a")?;
				let href = link.attr("href")?;
				let name = link.text().unwrap_or_default();
				let date = el
					.select_first(".fic_date_pub")
					.and_then(|d| d.attr("title").or_else(|| d.text()))
					.and_then(|d| parse_chapter_date(&d));
				Some((order, href, name, date))
			})
			.collect()
		})
		.unwrap_or_default();
	items.sort_by_key(|(order, ..)| *order);

	let names: Vec<&str> = items.iter().map(|(_, _, name, _)| name.as_str()).collect();
	let numbers = chapter_numbers(&names);
	Ok(items
		.iter()
		.zip(numbers)
		.filter_map(|((_, href, name, date), number)| {
			let key = chapter_id(href)?;
			Some(Chapter {
				key: key.to_string(),
				title: chapter_title(name, number),
				chapter_number: Some(number),
				date_uploaded: *date,
				url: Some(href.clone()),
				..Default::default()
			})
		})
		.collect())
}

// The series page's details.
fn apply_details(novel: &mut Novel, text: &str) -> Result<()> {
	let html = parse(text)?;
	if let Some(title) = html.select_first(".fic_title").and_then(|e| e.text()) {
		novel.title = title.trim().to_string();
	}
	novel.cover = html
		.select_first(".fic_image img")
		.and_then(|e| e.attr("src"))
		.filter(|s| s.starts_with("http"));
	novel.authors = html
		.select_first(".auth_name_fic")
		.and_then(|e| e.text())
		.map(|a| vec![a.trim().to_string()]);
	novel.description = html
		.select_first(".wi_fic_desc")
		.and_then(|e| e.html())
		.map(|h| content::plain_text(&h))
		.filter(|d| !d.is_empty());
	let genres = texts(&html, ".wi_fic_genre a.fic_genre");
	let warnings = texts(&html, ".mature_contains a");
	novel.content_rating = content_rating(&genres, &warnings);
	let mut tags = genres;
	tags.extend(texts(&html, ".wi_fic_showtags a.stag"));
	novel.tags = Some(tags);
	novel.status = parse_status(&html);
	Ok(())
}

// Novel keys are "<id>/<slug>", the series URL's path: /series/117137/the-runesmith/.
fn novel_url(key: &str) -> String {
	format!("{BASE_URL}/series/{key}/")
}

fn novel_id(key: &str) -> &str {
	key.split('/').next().unwrap_or(key)
}

// Chapter pages are /read/<id>-<slug>/chapter/<chapter id>/. The slug isn't
// checked, so the id alone finds the page.
fn chapter_url(novel_key: &str, chapter_key: &str) -> String {
	let slug = novel_key.replacen('/', "-", 1);
	format!("{BASE_URL}/read/{slug}/chapter/{chapter_key}/")
}

fn chapter_id(href: &str) -> Option<&str> {
	let rest = &href[href.find("/chapter/")? + "/chapter/".len()..];
	let id = rest.trim_end_matches('/');
	(!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}

// "https://www.scribblehub.com/series/117137/the-runesmith/" -> "117137/the-runesmith"
fn novel_key(href: &str) -> Option<String> {
	let rest = &href[href.find("/series/")? + "/series/".len()..];
	let key = rest.trim_end_matches('/');
	let (id, slug) = key.split_once('/')?;
	(!id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) && !slug.is_empty())
		.then(|| key.to_string())
}

fn texts(html: &Document, selector: &str) -> Vec<String> {
	html.select(selector)
		.map(|els| {
			els.filter_map(|e| e.text())
				.map(|t| t.trim().to_string())
				.filter(|t| !t.is_empty())
				.collect()
		})
		.unwrap_or_default()
}

fn parse_list_item(el: &Element) -> Option<Novel> {
	let link = el.select_first(".search_title a")?;
	let key = novel_key(&link.attr("href")?)?;
	let title = link.text()?.trim().to_string();
	let genres: Vec<String> = el
		.select(".search_genre a.fic_genre")
		.map(|els| {
			els.filter_map(|e| e.text())
				.map(|t| t.trim().to_string())
				.collect()
		})
		.unwrap_or_default();
	Some(Novel {
		url: Some(novel_url(&key)),
		key,
		title,
		cover: el
			.select_first(".search_img img")
			.and_then(|e| e.attr("src"))
			.filter(|s| s.starts_with("http")),
		content_rating: content_rating(&genres, &[]),
		tags: Some(genres),
		..Default::default()
	})
}

// The page scripts set up the pager with `items: <pages>` and
// `currentPage: '<page>'`. A page past the end has neither.
fn pagination(text: &str) -> Option<(i32, i32)> {
	let number_after = |marker: &str| -> Option<i32> {
		let rest = &text[text.find(marker)? + marker.len()..];
		let rest = rest.trim_start_matches(['\'', '"', ' ']);
		let end = rest
			.find(|c: char| !c.is_ascii_digit())
			.unwrap_or(rest.len());
		rest[..end].parse().ok()
	};
	Some((number_after("currentPage:")?, number_after("items:")?))
}

// The sidebar's status line starts with the status: "Ongoing - Updated 6 hours
// ago", or just "Completed" (the date in a tooltip). It's the `<li>` holding
// the `fa-question status` icon. Statuses: Ongoing, Completed, Hiatus.
fn parse_status(html: &Document) -> NovelStatus {
	let text = html
		.select_first("i.status")
		.and_then(|i| i.parent()?.parent()?.text())
		.unwrap_or_default();
	match text.split_whitespace().next() {
		Some("Ongoing") => NovelStatus::Ongoing,
		Some("Completed") => NovelStatus::Completed,
		Some("Hiatus") => NovelStatus::Hiatus,
		_ => NovelStatus::Unknown,
	}
}

// Genres "Adult" and "Smut" and the "Sexual Content" warning mean explicit
// content; "Mature", "Ecchi" and the other warnings (Gore, Strong Language)
// mean suggestive.
fn content_rating(genres: &[String], warnings: &[String]) -> ContentRating {
	let has = |list: &[String], name: &str| list.iter().any(|g| g.eq_ignore_ascii_case(name));
	if has(genres, "Adult") || has(genres, "Smut") || has(warnings, "Sexual Content") {
		ContentRating::NSFW
	} else if has(genres, "Mature") || has(genres, "Ecchi") || !warnings.is_empty() {
		ContentRating::Suggestive
	} else {
		ContentRating::Safe
	}
}

// "Sep 27, 2026 09:00 AM" (older chapters), or "7 hours ago" / "1 day ago"
// (recent ones).
fn parse_chapter_date(value: &str) -> Option<i64> {
	let value = value.trim();
	if let Some(ago) = value.strip_suffix(" ago") {
		let (n, unit) = ago.split_once(' ')?;
		let n: i64 = n.parse().ok()?;
		let unit = unit.trim_end_matches('s');
		let secs = match unit {
			"sec" | "second" => 1,
			"min" | "minute" => 60,
			"hour" => 3600,
			"day" => 86400,
			"week" => 7 * 86400,
			"month" => 30 * 86400,
			"year" => 365 * 86400,
			_ => return None,
		};
		return Some(current_date() - n * secs);
	}
	parse_date(value, "MMM d, yyyy hh:mm a").or_else(|| parse_date(value, "MMM d, yyyy"))
}

// Splits a chapter label off the start of `s`: "Chapter 12", "Chapter 01 :",
// "Ch. 12", "Chapter no.1:", "Episode 3", or a bare "12." / "12:" / "12 -" /
// "12 |" / "12". Returns the number and the rest, without its leading
// separator.
fn split_label(s: &str) -> Option<(f32, &str)> {
	split_label_with(s, false)
}

// `loose` also takes a bare number followed by a space ("01 Long Ago"), which
// is only trusted when most of a series' chapters are named that way.
fn split_label_with(s: &str, loose: bool) -> Option<(f32, &str)> {
	let s = s.trim_start();
	let word_end = s
		.find(|c: char| !c.is_ascii_alphabetic() && c != '.')
		.unwrap_or(s.len());
	let word = &s[..word_end];
	let rest = if word.is_empty() {
		s
	} else if [
		"chapter", "chapter.", "ch.", "ch", "chp", "chp.", "chatper", "chaper", "chapater",
		"episode", "ep", "ep.",
	]
	.iter()
	.any(|w| word.eq_ignore_ascii_case(w))
	{
		// "Chapter . 5", "Chapter: 5", "Chapter #5", "Chapter no.5"
		let rest = s[word_end..]
			.trim_start_matches(|c: char| c.is_whitespace() || matches!(c, '.' | ':' | '#'));
		match rest.get(..2) {
			Some(no) if no.eq_ignore_ascii_case("no") => {
				rest[2..].trim_start_matches(|c: char| c.is_whitespace() || c == '.')
			}
			_ => rest,
		}
	} else {
		return None;
	};
	let digits_end = rest
		.find(|c: char| !c.is_ascii_digit() && c != '.')
		.unwrap_or(rest.len());
	let digits = rest[..digits_end].trim_end_matches('.');
	if digits.is_empty() {
		return None;
	}
	let number: f32 = digits.parse().ok()?;
	let after = &rest[digits.len()..];
	// A bare number must be the whole name or be followed by a separator
	// ("12. Name", "12: Name", "12 - Name"), so a title that starts with a
	// number ("100 Days") stays.
	if word.is_empty()
		&& !after.trim().is_empty()
		&& !after.starts_with(['.', ':'])
		&& !after.trim_start().starts_with(['-', '–', '—', ':', '|'])
		&& !(loose && after.starts_with(' '))
	{
		return None;
	}
	// "Chapter 3" followed by more digits or letters ("Chapter 3rd") isn't a label.
	if after.starts_with(|c: char| c.is_ascii_alphanumeric()) {
		return None;
	}
	Some((
		number,
		after.trim_start_matches(|c: char| {
			c.is_whitespace() || matches!(c, '-' | ':' | '–' | '—' | '.' | '|')
		}),
	))
}

// Reader sorts chapters on their number and merges ones with the same number,
// so numbers must be unique and rising. Authors name chapters freely: most
// label them ("Chapter 12 – Name", "12: Name"), and many add unlabeled ones
// (a prologue, character sheets, interludes, an epilogue).
//
// When at least 80% of chapters carry a label and the labels rise in reading
// order, the labels are used, and each unlabeled chapter gets a number between
// its labeled neighbours (a prologue before "Chapter 1" becomes 0.5). Otherwise
// every chapter is numbered by position.
fn chapter_numbers(names: &[&str]) -> Vec<f32> {
	let labels = |loose: bool| -> Vec<Option<f32>> {
		names
			.iter()
			.map(|n| split_label_with(n, loose).map(|(num, _)| num))
			.collect()
	};
	label_numbers(&labels(false))
		.or_else(|| label_numbers(&labels(true)))
		.unwrap_or_else(|| (1..=names.len()).map(|i| i as f32).collect())
}

fn label_numbers(labels: &[Option<f32>]) -> Option<Vec<f32>> {
	let labeled: Vec<f32> = labels.iter().flatten().copied().collect();
	let rising = labeled.windows(2).all(|w| w[0] < w[1]);
	if labeled.is_empty() || labeled.len() * 5 < labels.len() * 4 || !rising {
		return None;
	}

	let mut out = Vec::with_capacity(labels.len());
	let mut i = 0;
	while i < labels.len() {
		if let Some(n) = labels[i] {
			out.push(n);
			i += 1;
			continue;
		}
		// A run of unlabeled chapters between two labels (or an end).
		let run_end = (i..labels.len())
			.find(|&j| labels[j].is_some())
			.unwrap_or(labels.len());
		let prev = if i == 0 { None } else { labels[i - 1] };
		let next = labels.get(run_end).copied().flatten();
		let (low, high) = match (prev, next) {
			(Some(p), Some(n)) => (p, n),
			(None, Some(n)) => (n - 1.0, n),
			(Some(p), None) => (p, p + 1.0),
			(None, None) => (0.0, 1.0),
		};
		let count = run_end - i;
		for k in 1..=count {
			let n = low + (high - low) * k as f32 / (count + 1) as f32;
			out.push(round2(n));
		}
		i = run_end;
	}
	// Rounding can collapse numbers in a long run of unlabeled chapters between
	// close labels.
	out.windows(2).all(|w| w[0] < w[1]).then_some(out)
}

// Two decimals, so a prologue shows as 0.5 rather than 0.49999.
fn round2(n: f32) -> f32 {
	let scaled = n * 100.0;
	let rounded = if scaled >= 0.0 {
		(scaled + 0.5) as i64
	} else {
		(scaled - 0.5) as i64
	};
	rounded as f32 / 100.0
}

// The app shows the chapter number separately, so a label is dropped from the
// title when its number is the one sent. A name that is only a label
// ("Chapter 5") has no title.
fn chapter_title(name: &str, number: f32) -> Option<String> {
	let name = name.trim();
	let title = [false, true]
		.iter()
		.find_map(|&loose| match split_label_with(name, loose) {
			Some((n, rest)) if n == number => Some(rest.trim()),
			_ => None,
		})
		.unwrap_or(name);
	(!title.is_empty()).then(|| title.to_string())
}

// Decodes the entities the site's editor emits. Anything else goes to the
// host's full decoder; buny-test-runner's only knows a handful (not &nbsp;),
// so the common ones are handled here to behave the same in tests and the app.
fn unescape(text: &str) -> String {
	if !text.contains('&') {
		return text.to_string();
	}
	let mut out = String::with_capacity(text.len());
	let mut rest = text;
	while let Some(amp) = rest.find('&') {
		out.push_str(&rest[..amp]);
		rest = &rest[amp..];
		let entity = rest[1..]
			.find(';')
			.filter(|&end| end <= 10)
			.map(|end| &rest[1..=end]);
		let decoded = entity.and_then(|name| match name {
			"nbsp" => Some('\u{a0}'),
			"amp" => Some('&'),
			"lt" => Some('<'),
			"gt" => Some('>'),
			"quot" => Some('"'),
			"apos" => Some('\''),
			"lsquo" => Some('‘'),
			"rsquo" => Some('’'),
			"ldquo" => Some('“'),
			"rdquo" => Some('”'),
			"hellip" => Some('…'),
			"mdash" => Some('—'),
			"ndash" => Some('–'),
			_ => {
				let num = name.strip_prefix('#')?;
				let code = match num.strip_prefix(['x', 'X']) {
					Some(hex) => u32::from_str_radix(hex, 16).ok()?,
					None => num.parse().ok()?,
				};
				char::from_u32(code)
			}
		});
		match (entity, decoded) {
			(Some(name), Some(c)) => {
				out.push(c);
				rest = &rest[name.len() + 2..];
			}
			(Some(name), None) if name.bytes().all(|b| b.is_ascii_alphanumeric()) => {
				return Html::unescape(text).unwrap_or_else(|| text.to_string());
			}
			_ => {
				out.push('&');
				rest = &rest[1..];
			}
		}
	}
	out.push_str(rest);
	out
}

register_source!(ScribbleHub, ListingProvider, Home, DeepLinkHandler);

#[cfg(test)]
mod tests;
