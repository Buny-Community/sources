#![no_std]
use buny::{
	Chapter, ContentBlock, ContentRating, FilterValue, Novel, NovelPageResult, NovelStatus, Result,
	Source,
	alloc::{String, Vec, string::ToString, vec},
	imports::{html::Html, std::send_partial_result},
	prelude::*,
};

mod content;
mod proto;
mod traits;

use proto::{Message, Writer, owned};

const BASE_URL: &str = "https://www.wuxiaworld.com";

// SearchNovels silently caps `count` at 100. The site asks for 16 at a time.
const PAGE_SIZE: i32 = 20;
const MAX_COUNT: i32 = 100;

// SearchNovelsRequest.SortType, in the order of the sort filter options.
// An unknown value falls back to the API's default order without an error.
const SORT_POPULAR: i32 = 1;
const SORT_NEW: i32 = 2;
const SORT_RATING: i32 = 6;
const SORT_TRENDING: i32 = 7;
const SORT_VALUES: [i32; 6] = [SORT_POPULAR, SORT_TRENDING, SORT_NEW, SORT_RATING, 3, 4];
// SearchNovelsRequest.SortDirection.
const ASC: i32 = 0;
const DESC: i32 = 1;

// NovelItem.Status. Finished is 0, the protobuf default, so a request that
// leaves status out gets finished novels only; "all" has to be sent as -1.
const STATUS_ALL: i32 = -1;
const STATUS_FINISHED: i32 = 0;
const STATUS_ACTIVE: i32 = 1;
const STATUS_HIATUS: i32 = 2;

struct Wuxiaworld;

/// A SearchNovels query. Pages go by cursor: `searchAfterId` is the id of the
/// last novel of the previous page, and it follows every sort order.
struct Search {
	title: Option<String>,
	language: Option<String>,
	status: i32,
	sort: i32,
	direction: i32,
	genres: Vec<String>,
}

impl Default for Search {
	fn default() -> Self {
		Self {
			title: None,
			language: None,
			status: STATUS_ALL,
			sort: SORT_POPULAR,
			direction: DESC,
			genres: Vec::new(),
		}
	}
}

impl Search {
	fn request(&self, after: Option<i32>, count: i32) -> Writer {
		let mut w = Writer::default();
		if let Some(title) = &self.title {
			w.string_value(1, title);
		}
		if let Some(language) = &self.language {
			w.string_value(2, language);
		}
		// Field order matches the site's encoder; protobuf doesn't care.
		w.int32(3, self.status);
		w.int32(4, self.sort);
		w.int32(5, self.direction);
		if let Some(id) = after {
			w.int32_value(6, id);
		}
		w.int32(7, count);
		if !self.genres.is_empty() {
			// GenresFilter { genres, operator }: And (0) matches novels that
			// have every genre, as the site's filter does.
			let mut filter = Writer::default();
			for genre in &self.genres {
				filter.string(1, genre);
			}
			filter.int32(2, 0);
			w.message(10, &filter);
		}
		w
	}

	// Returns (novel ids, novels, total).
	fn fetch(&self, after: Option<i32>, count: i32) -> Result<(Vec<i32>, Vec<Novel>, i64)> {
		let data = proto::call("Novels/SearchNovels", &self.request(after, count))?;
		let reply = Message::new(&data);
		let mut ids = Vec::new();
		let mut novels = Vec::new();
		for item in reply.messages(1) {
			if let Some(novel) = parse_novel(&item) {
				ids.push(item.int32(1).unwrap_or(0));
				novels.push(novel);
			}
		}
		Ok((ids, novels, reply.int64(2).unwrap_or(0)))
	}

	/// The app pages by number, the API by cursor, so page N first walks past
	/// the earlier pages (at most 100 per call; the catalogue is ~250 novels).
	fn page(&self, page: i32) -> Result<NovelPageResult> {
		let mut skip = (page.max(1) - 1) * PAGE_SIZE;
		let mut after = None;
		while skip > 0 {
			let count = skip.min(MAX_COUNT);
			let (ids, _, _) = self.fetch(after, count)?;
			let Some(&last) = ids.last() else {
				return Ok(NovelPageResult::default());
			};
			if (ids.len() as i32) < count {
				return Ok(NovelPageResult::default());
			}
			after = Some(last);
			skip -= count;
		}
		let (ids, entries, total) = self.fetch(after, PAGE_SIZE)?;
		let seen = (page.max(1) - 1) as i64 * PAGE_SIZE as i64 + ids.len() as i64;
		Ok(NovelPageResult {
			has_next_page: ids.len() as i32 == PAGE_SIZE && seen < total,
			entries,
		})
	}
}

impl Wuxiaworld {
	fn listing(id: &str, page: i32) -> Result<NovelPageResult> {
		let search = match id {
			"popular" => Search::default(),
			"trending" => Search {
				sort: SORT_TRENDING,
				..Default::default()
			},
			"new" => Search {
				sort: SORT_NEW,
				..Default::default()
			},
			"top_rated" => Search {
				sort: SORT_RATING,
				..Default::default()
			},
			"completed" => Search {
				status: STATUS_FINISHED,
				..Default::default()
			},
			_ => bail!("Unknown listing: {id}"),
		};
		search.page(page)
	}

	fn get_novel(slug: &str) -> Result<Vec<u8>> {
		let mut request = Writer::default();
		request.string(2, slug);
		let data = proto::call("Novels/GetNovel", &request)?;
		if Message::new(&data).message(1).is_none() {
			bail!("Novel not found on Wuxiaworld");
		}
		Ok(data)
	}
}

impl Source for Wuxiaworld {
	fn new() -> Self {
		Self
	}

	fn get_search_novel_list(
		&self,
		query: Option<String>,
		page: i32,
		filters: Vec<FilterValue>,
	) -> Result<NovelPageResult> {
		let mut search = Search {
			title: query
				.as_deref()
				.map(str::trim)
				.filter(|q| !q.is_empty())
				.map(ToString::to_string),
			..Default::default()
		};
		for filter in filters {
			match filter {
				FilterValue::Sort {
					index, ascending, ..
				} => {
					if let Some(&sort) = SORT_VALUES.get(index as usize) {
						search.sort = sort;
						search.direction = if ascending { ASC } else { DESC };
					}
				}
				FilterValue::Select { id, value } if id == "status" => {
					search.status = match value.as_str() {
						"Ongoing" => STATUS_ACTIVE,
						"Completed" => STATUS_FINISHED,
						"Hiatus" => STATUS_HIATUS,
						_ => STATUS_ALL,
					};
				}
				// The API matches the language name exactly ("Chinese"); an
				// abbreviation such as "CN" returns nothing.
				FilterValue::Select { id, value } if id == "language" && value != "All" => {
					search.language = Some(value);
				}
				FilterValue::MultiSelect { id, included, .. } if id == "genres" => {
					search.genres = included;
				}
				_ => {}
			}
		}
		search.page(page)
	}

	fn get_novel_update(
		&self,
		mut novel: Novel,
		needs_details: bool,
		needs_chapters: bool,
		page: i32,
	) -> Result<Novel> {
		// The chapter list is keyed by the numeric novel id, which only
		// GetNovel returns, so it is fetched either way.
		let data = Self::get_novel(&novel.key)?;
		let item = Message::new(&data).message(1).unwrap_or_default();
		let novel_id = item.int32(1).ok_or(error!("Invalid novel"))?;

		if needs_details {
			if let Some(details) = parse_novel(&item) {
				novel = Novel {
					chapters: novel.chapters,
					has_more_chapters: novel.has_more_chapters,
					..details
				};
			}
			if needs_chapters {
				send_partial_result(&novel);
			}
		}

		if needs_chapters {
			// The whole list comes in one reply (~120 bytes a chapter, 860 KB
			// for the longest series), so there is only ever one page.
			let _ = page;
			let mut request = Writer::default();
			request.int32(1, novel_id);
			let data = proto::call("Chapters/GetChapterList", &request)?;
			novel.chapters = Some(parse_chapters(&Message::new(&data), &novel.key));
			novel.has_more_chapters = Some(false);
		}

		Ok(novel)
	}

	fn get_chapter_content_list(
		&self,
		novel: Novel,
		chapter: Chapter,
	) -> Result<Vec<ContentBlock>> {
		let mut slugs = Writer::default();
		slugs.string(1, &novel.key).string(2, &chapter.key);
		let mut property = Writer::default();
		property.message(2, &slugs);
		let mut request = Writer::default();
		request.message(1, &property);

		let data = proto::call("Chapters/GetChapter", &request)?;
		let Some(item) = Message::new(&data).message(1) else {
			bail!("Chapter not found on Wuxiaworld");
		};

		// A locked chapter comes back either as a ~200-word teaser flagged
		// `isTeaser` (8), or, for advance chapters, with no `content` at all.
		// `pricingInfo.isFree` isn't the test here: a few series serve their
		// first chapters in full without it.
		let body = item.string_value(5).filter(|b| !b.trim().is_empty());
		let Some(body) = body.filter(|_| !item.bool(8)) else {
			bail!("{}", lock_reason(&item));
		};

		let series = item.message(14).unwrap_or_default();
		let name = item.str(2).unwrap_or("");
		let headings = [
			name,
			series.str(2).unwrap_or(novel.title.as_str()),
			// "Chapter 1 - Prologue" opens with just "Prologue".
			split_label(name).map_or("", |(_, rest)| rest),
		];
		let mut blocks = content::chapter_blocks(body, &headings);
		if blocks.is_empty() {
			bail!("Chapter has no text");
		}

		// The site shows the translator's note in a box under the chapter.
		if let Some(thoughts) = item.string_value(19) {
			let note = content::chapter_blocks(thoughts, &["", "", ""]);
			if !note.is_empty() {
				blocks.push(ContentBlock::Divider);
				blocks.push(ContentBlock::paragraph("**Translator's thoughts**", None));
				blocks.extend(note);
			}
		}
		Ok(blocks)
	}
}

// Why a chapter is only a teaser or empty: its Karma price (ChapterItem.karmaInfo)
// and, for advance chapters, the sponsor plan that includes it (sponsorInfo).
fn lock_reason(item: &Message) -> String {
	let karma = item
		.message(13)
		.and_then(|k| k.int32_value(1))
		.filter(|&p| p > 0);
	let sponsor = item.message(15).filter(|s| s.bool(1));
	let plan = sponsor
		.and_then(|s| s.messages(3).next())
		.and_then(|p| owned(p.str(1)));
	let how = match (karma, plan, sponsor.is_some()) {
		(Some(k), Some(p), _) => format!(" ({k} Karma, or the {p} sponsor plan)"),
		(Some(k), None, true) => format!(" ({k} Karma, or a sponsor plan)"),
		(Some(k), None, false) => format!(" ({k} Karma)"),
		(None, Some(p), _) => format!(" (the {p} sponsor plan)"),
		(None, None, true) => " (a sponsor plan)".to_string(),
		(None, None, false) => String::new(),
	};
	format!(
		"This chapter is locked on Wuxiaworld{how}. Only a preview is free; it can only be unlocked on the website."
	)
}

/// A NovelItem, from a search reply or GetNovel.
fn parse_novel(item: &Message) -> Option<Novel> {
	let key = owned(item.str(3))?;
	let tags: Vec<String> = item
		.strs(16)
		.map(str::trim)
		.filter(|g| !g.is_empty())
		.map(ToString::to_string)
		.collect();
	// There is no rating flag. 18 novels carry the "Mature" genre.
	let content_rating = if tags.iter().any(|t| t == "Mature") {
		ContentRating::Suggestive
	} else {
		ContentRating::Safe
	};
	Some(Novel {
		url: Some(format!("{BASE_URL}/novel/{key}")),
		title: item.str(2).unwrap_or("").trim().to_string(),
		// ~150 KB WebP (1200x1750), small enough to use as is.
		cover: owned(item.string_value(10)),
		// `synopsis` is the story blurb. `description` is the translator's
		// page (credits, schedule, fanart links), so it is left out.
		description: item
			.string_value(9)
			.map(content::plain_text)
			.filter(|d| !d.is_empty()),
		authors: owned(item.string_value(13)).map(|a| vec![a]),
		status: match item.int32(4).unwrap_or(STATUS_FINISHED) {
			STATUS_FINISHED => NovelStatus::Completed,
			STATUS_ACTIVE => NovelStatus::Ongoing,
			STATUS_HIATUS => NovelStatus::Hiatus,
			_ => NovelStatus::Unknown,
		},
		tags: Some(tags),
		content_rating,
		key,
		..Default::default()
	})
}

struct RawChapter<'a> {
	slug: &'a str,
	name: &'a str,
	number: Option<f64>,
	offset: i32,
	published: Option<i64>,
	free: bool,
}

/// GetChapterListResponse: chapter groups (volumes, "Side Stories", ...), each
/// with its chapters, already in reading order.
fn parse_chapters(reply: &Message, novel_key: &str) -> Vec<Chapter> {
	let raw: Vec<RawChapter> = reply
		.messages(1)
		.flat_map(|group| group.messages(6))
		.filter_map(|ch| {
			Some(RawChapter {
				slug: ch.str(3).filter(|s| !s.is_empty())?,
				name: ch.str(2).unwrap_or(""),
				number: ch.decimal(4),
				offset: ch.int32(17).unwrap_or(0),
				published: ch.timestamp(18),
				// pricingInfo.isFree: ~4% of all chapters. The rest are teasers
				// without Karma, VIP or a sponsor plan, except that the 3 series
				// that flag no chapter free serve their first 50 in full. Those
				// stay marked locked (one such series serves only teasers, so
				// there's no safe rule); opening one still works, since the
				// chapter reply is what decides.
				free: ch.message(20).is_some_and(|p| p.bool(1)),
			})
		})
		.collect();

	let use_number = numbers_are_labels(&raw);
	raw.iter()
		.map(|ch| {
			let number = if use_number {
				ch.number.unwrap_or(0.0)
			} else {
				ch.offset as f64
			};
			Chapter {
				key: ch.slug.to_string(),
				title: chapter_title(ch.name, number),
				chapter_number: Some(number as f32),
				date_uploaded: ch.published,
				url: Some(format!("{BASE_URL}/novel/{novel_key}/{}", ch.slug)),
				locked: !ch.free,
				..Default::default()
			}
		})
		.collect()
}

// The app sorts by chapter number and treats equal numbers as one chapter, so
// the number has to rise through the list. The API's `number` is the one in
// the chapter names ("Chapter 12"), with extras in between (0.1, 245.1), but in
// some series it is a sort key instead ("Book 2 Chapter 5" is 2.005). Those
// fall back to `offset`, the 1-based position in the list.
fn numbers_are_labels(chapters: &[RawChapter]) -> bool {
	if chapters.is_empty() || chapters.iter().any(|c| c.number.is_none()) {
		return false;
	}
	let rising = chapters
		.windows(2)
		.all(|w| w[0].number.unwrap_or(0.0) < w[1].number.unwrap_or(0.0));
	let agree = chapters
		.iter()
		.filter(|c| {
			split_label(c.name).is_some_and(|(n, _)| n as f64 == trunc(c.number.unwrap_or(0.0)))
		})
		.count();
	rising && agree * 10 >= chapters.len() * 9
}

// no_std has no f64::trunc.
fn trunc(x: f64) -> f64 {
	x as i64 as f64
}

fn is_chapter_word(word: &str) -> bool {
	// Includes the misspellings found in the catalogue.
	[
		"chapter", "chapters", "chater", "chaptere", "chpater", "chaper", "chatper", "ch",
		"episode",
	]
	.iter()
	.any(|w| word.eq_ignore_ascii_case(w))
}

/// Splits a leading chapter label off a name: an optional series acronym
/// ("AST", "WDQK"), an optional chapter word ("Chapter", "Ch.", "Episode",
/// typos), and the number ("0283"). Returns the number and the rest after its
/// separator: "Chapter 12: Name", "AST 2194 - Name", "1180 - Name",
/// "Chapter 1 Memories". A number glued to letters ("1929A") is not a label.
fn split_label(name: &str) -> Option<(u32, &str)> {
	let mut s = name.trim_start();

	// All-caps acronym of 2-4 letters, then ':' or whitespace.
	let caps = s.bytes().take_while(u8::is_ascii_uppercase).count();
	if (2..=4).contains(&caps)
		&& s[caps..].starts_with([':', ' ', '\t'])
		&& !is_chapter_word(&s[..caps])
	{
		s = s[caps..].trim_start_matches(':').trim_start();
	}

	let word = s.bytes().take_while(u8::is_ascii_alphabetic).count();
	if word > 0 {
		if !is_chapter_word(&s[..word]) {
			return None;
		}
		s = s[word..].trim_start_matches('.').trim_start();
		s = s.trim_start_matches(':').trim_start();
	}

	let digits = s.bytes().take_while(u8::is_ascii_digit).count();
	if digits == 0 {
		return None;
	}
	let number: u32 = s[..digits].parse().ok()?;
	let rest = &s[digits..];
	// A decimal ("197.1") or glued suffix ("1929A", "273-2") is part of the
	// number, not a separator.
	let next = rest.chars().next();
	let second = rest.chars().nth(1);
	match next {
		None => {}
		Some(c) if c.is_whitespace() || matches!(c, ':' | '–' | '—' | ',' | ')') => {}
		Some('-' | '.') if !second.is_some_and(|c| c.is_ascii_digit()) => {}
		_ => return None,
	}
	Some((
		number,
		rest.trim_start_matches(|c: char| {
			c.is_whitespace() || matches!(c, ':' | '-' | '–' | '—' | '.' | ',')
		})
		.trim_end(),
	))
}

/// The app shows the chapter number next to the title, so a leading
/// "Chapter N" is dropped, but only when N is the number the app shows.
/// Otherwise the name stays whole: "Book 3, Chapter 5", or a series whose
/// names skip numbers.
fn chapter_title(name: &str, number: f64) -> Option<String> {
	let name = name.trim();
	if let Some((n, rest)) = split_label(name)
		&& n as f64 == trunc(number)
	{
		return (!rest.is_empty()).then(|| rest.to_string());
	}
	(!name.is_empty()).then(|| name.to_string())
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
			"eacute" => Some('é'),
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

register_source!(Wuxiaworld, ListingProvider, Home, DeepLinkHandler);

#[cfg(test)]
mod tests;
