use super::*;
use buny::{
	DeepLinkHandler, DeepLinkResult, Home, HomeComponentValue, Listing, ListingProvider, alloc::vec,
};
use buny_test::buny_test;

const REGRESSION: &str = "absolute-regression";

fn paragraphs(blocks: &[ContentBlock]) -> Vec<&str> {
	blocks
		.iter()
		.filter_map(|b| match b {
			ContentBlock::Paragraph(text, _) => Some(text.as_str()),
			_ => None,
		})
		.collect()
}

fn has_invisible(text: &str) -> bool {
	text.chars()
		.any(|c| matches!(c, '\u{200b}'..='\u{200f}' | '\u{2060}' | '\u{feff}'))
}

fn search(filters: Vec<FilterValue>) -> NovelPageResult {
	FenrirRealm::new()
		.get_search_novel_list(None, 1, filters)
		.unwrap()
}

// ---- live ----

#[buny_test]
fn test_search_and_novel() {
	let source = FenrirRealm::new();
	let result = source
		.get_search_novel_list(Some("absolute regression".into()), 1, Vec::new())
		.unwrap();
	let entry = result
		.entries
		.iter()
		.find(|n| n.key == REGRESSION)
		.expect("search result")
		.clone();
	assert_eq!(entry.title, "Absolute Regression");
	assert!(
		entry
			.cover
			.as_deref()
			.unwrap()
			.ends_with("?width=300&height=400")
	);

	let novel = source.get_novel_update(entry, true, true, 1).unwrap();
	println!(
		"{} {:?} {:?} {:?} {:?}",
		novel.title, novel.status, novel.content_rating, novel.cover, novel.tags
	);
	assert!(
		novel
			.cover
			.as_deref()
			.unwrap()
			.ends_with("?width=600&height=800")
	);
	let description = novel.description.as_deref().unwrap();
	assert!(description.contains("Send me to the past"));
	assert!(!description.contains('<'));
	assert_eq!(novel.status, NovelStatus::Ongoing);
	assert!(novel.tags.as_ref().unwrap().iter().any(|t| t == "Wuxia"));
	assert_eq!(novel.content_rating, ContentRating::Safe);

	// Every chapter in one reply, in reading order, free ones first.
	assert_eq!(novel.has_more_chapters, Some(false));
	let chapters = novel.chapters.unwrap();
	assert!(chapters.len() > 890, "{}", chapters.len());
	assert_eq!(chapters[0].key, "12471");
	assert_eq!(chapters[0].chapter_number, Some(1.0));
	assert_eq!(chapters[0].title.as_deref(), Some("Send Me to the Past"));
	assert_eq!(chapters[0].volume_number, None);
	assert!(!chapters[0].locked);
	assert!(chapters[0].date_uploaded.is_some());
	assert_eq!(
		chapters[0].url.as_deref(),
		Some("https://fenrirealm.com/series/absolute-regression/1")
	);
	assert!(
		chapters
			.windows(2)
			.all(|w| w[0].chapter_number < w[1].chapter_number)
	);
	// Paid chapters are the newest ones.
	assert!(chapters.last().unwrap().locked);
	println!(
		"{} chapters, {} locked",
		chapters.len(),
		chapters.iter().filter(|c| c.locked).count()
	);
}

#[buny_test]
fn test_chapter_json_body() {
	// Chapter 1 of Absolute Regression is a TipTap document that opens with four
	// credit lines, "=====", and "< Chapter 1: Send Me to the Past >".
	let source = FenrirRealm::new();
	let novel = Novel {
		key: REGRESSION.into(),
		..Default::default()
	};
	let chapter = Chapter {
		key: "12471".into(),
		..Default::default()
	};
	let blocks = source.get_chapter_content_list(novel, chapter).unwrap();
	let text = paragraphs(&blocks);
	println!(
		"{} blocks: {:?} .. {:?}",
		blocks.len(),
		&text[..3],
		text.last()
	);
	assert!(blocks.len() > 50);
	assert!(text[0].starts_with("Spirit Master Seo Gong silently stared"));
	assert!(text.iter().all(|t| !t.contains("Translator")));
}

#[buny_test]
fn test_chapter_html_body() {
	// An HTML chapter with both watermarks: hidden junk divs and zero-width runs.
	let source = FenrirRealm::new();
	let slug = "1-second-invincibility-in-the-game";
	let novel = source
		.get_novel_update(
			Novel {
				key: slug.into(),
				..Default::default()
			},
			false,
			true,
			1,
		)
		.unwrap();
	let first = novel.chapters.as_ref().unwrap()[0].clone();
	let blocks = source.get_chapter_content_list(novel, first).unwrap();
	let text = paragraphs(&blocks);
	println!(
		"{} blocks: {:?} .. {:?}",
		blocks.len(),
		&text[..3],
		text.last()
	);
	assert_eq!(text[0], "An unbelievable event has occurred.");
	assert!(text.len() > 50);
	assert!(text.iter().all(|t| !has_invisible(t)));
	// The junk strings are one long run of letters and digits.
	assert!(
		text.iter()
			.all(|t| t.contains(' ') || t.chars().count() < 40)
	);
}

#[buny_test]
fn test_locked_chapter() {
	let source = FenrirRealm::new();
	let novel = source
		.get_novel_update(
			Novel {
				key: REGRESSION.into(),
				..Default::default()
			},
			false,
			true,
			1,
		)
		.unwrap();
	let locked = novel
		.chapters
		.as_ref()
		.unwrap()
		.iter()
		.rev()
		.find(|c| c.locked)
		.expect("a paid chapter")
		.clone();
	let err = source
		.get_chapter_content_list(novel, locked)
		.expect_err("paid chapter must not return its teaser");
	println!("{err:?}");
	assert!(format!("{err:?}").contains("premium"));
}

#[buny_test]
fn test_volumes() {
	// Chapter numbers restart in each volume, and the API lists the volumes
	// interleaved.
	let novel = FenrirRealm::new()
		.get_novel_update(
			Novel {
				key: "miss-demon-king-please-endure-a-little-longer".into(),
				..Default::default()
			},
			false,
			true,
			1,
		)
		.unwrap();
	let chapters = novel.chapters.unwrap();
	assert_eq!(chapters[0].volume_number, Some(1.0));
	assert_eq!(chapters[0].chapter_number, Some(1.0));
	assert!(chapters.windows(2).all(|w| {
		(w[0].volume_number, w[0].chapter_number) < (w[1].volume_number, w[1].chapter_number)
	}));
	assert!(chapters[0].url.as_deref().unwrap().ends_with("/vol-1/1"));
}

#[buny_test]
fn test_listings() {
	let source = FenrirRealm::new();
	for id in ["popular", "trending", "updated", "latest"] {
		let listing = Listing {
			id: id.into(),
			name: id.into(),
			..Default::default()
		};
		let page = source.get_novel_list(listing.clone(), 1).unwrap();
		let page2 = source.get_novel_list(listing, 2).unwrap();
		println!(
			"{id}: {} {:?}",
			page.entries.len(),
			page.entries
				.iter()
				.take(3)
				.map(|n| &n.title)
				.collect::<Vec<_>>()
		);
		assert_eq!(page.entries.len(), NOVEL_PAGE_SIZE as usize);
		assert!(page.has_next_page);
		assert!(
			page.entries
				.iter()
				.all(|n| !n.title.is_empty() && n.cover.is_some())
		);
		assert_ne!(page.entries[0].key, page2.entries[0].key);
	}
	assert!(
		source
			.get_novel_list(
				Listing {
					id: "bogus".into(),
					..Default::default()
				},
				1
			)
			.is_err()
	);
}

#[buny_test]
fn test_sorts() {
	for index in 0..SORT_VALUES.len() as i32 {
		let result = search(vec![FilterValue::Sort {
			id: "sort".into(),
			index,
			ascending: false,
		}]);
		println!(
			"{}: {}",
			SORT_VALUES[index as usize], result.entries[0].title
		);
		assert!(!result.entries.is_empty());
	}
	let title = search(vec![FilterValue::Sort {
		id: "sort".into(),
		index: 5,
		ascending: false,
	}]);
	assert!(
		title.entries[0]
			.title
			.starts_with(|c: char| c.is_ascii_digit())
	);
}

#[buny_test]
fn test_filters() {
	let wuxia = || FilterValue::MultiSelect {
		id: "genres".into(),
		included: vec!["31".into()],
		excluded: Vec::new(),
	};
	let with_genre = |name: &str, novels: &NovelPageResult| {
		novels
			.entries
			.iter()
			.all(|n| n.tags.as_ref().unwrap().iter().any(|t| t == name))
	};

	let result = search(vec![wuxia()]);
	assert!(with_genre("Wuxia", &result));

	// Two genres match any of them unless "All" is picked.
	let both = |condition: &str| {
		search(vec![
			FilterValue::MultiSelect {
				id: "genres".into(),
				included: vec!["31".into(), "18".into()],
				excluded: Vec::new(),
			},
			FilterValue::Select {
				id: "genre_condition".into(),
				value: condition.into(),
			},
		])
	};
	let all = both("and");
	assert!(with_genre("Wuxia", &all) && with_genre("Romance", &all));
	assert!(!both("or").entries.is_empty());

	let excluded = search(vec![FilterValue::MultiSelect {
		id: "genres".into(),
		included: Vec::new(),
		excluded: vec!["7".into()],
	}]);
	assert!(!excluded.entries.is_empty());
	assert!(
		excluded
			.entries
			.iter()
			.all(|n| !n.tags.as_ref().unwrap().iter().any(|t| t == "Fantasy"))
	);

	let completed = search(vec![FilterValue::MultiSelect {
		id: "status".into(),
		included: vec!["completed".into()],
		excluded: Vec::new(),
	}]);
	assert!(!completed.entries.is_empty());
	assert!(
		completed
			.entries
			.iter()
			.all(|n| n.status == NovelStatus::Completed)
	);

	let not_ongoing = search(vec![FilterValue::MultiSelect {
		id: "status".into(),
		included: Vec::new(),
		excluded: vec!["on-going".into()],
	}]);
	assert!(
		not_ongoing
			.entries
			.iter()
			.all(|n| n.status != NovelStatus::Ongoing)
	);

	// Tag 721 is "Transmigration".
	let tagged = search(vec![FilterValue::MultiSelect {
		id: "tags".into(),
		included: vec!["721".into()],
		excluded: Vec::new(),
	}]);
	assert!(with_genre("Transmigration", &tagged));

	// Types aren't shown on entries; "Original" has ~30 series, so one page
	// with no next page.
	let original = search(vec![FilterValue::Select {
		id: "type".into(),
		value: "original_novel".into(),
	}]);
	let all_types = search(vec![FilterValue::Select {
		id: "type".into(),
		value: "any".into(),
	}]);
	println!("original: {}", original.entries.len());
	assert!(!original.entries.is_empty());
	assert_ne!(original.entries[0].key, all_types.entries[0].key);
}

#[buny_test]
fn test_search_no_results() {
	let result = FenrirRealm::new()
		.get_search_novel_list(Some("zzqxv no such novel".into()), 1, Vec::new())
		.unwrap();
	assert!(result.entries.is_empty());
	assert!(!result.has_next_page);
}

#[buny_test]
fn test_unknown_novel() {
	let err = FenrirRealm::new()
		.get_novel_update(
			Novel {
				key: "no-such-series-zzz".into(),
				..Default::default()
			},
			true,
			false,
			1,
		)
		.expect_err("unknown series");
	assert!(format!("{err:?}").contains("Not found"));
}

#[buny_test]
fn test_home() {
	let layout = FenrirRealm::new().get_home().unwrap();
	assert_eq!(layout.components.len(), 4);
	for component in &layout.components {
		let (entries, listing) = match &component.value {
			HomeComponentValue::Details {
				entries, listing, ..
			}
			| HomeComponentValue::Stack {
				entries, listing, ..
			} => (entries, listing),
			HomeComponentValue::Scroller {
				entries,
				listing,
				size,
				..
			} => {
				assert_eq!(*size, 0);
				(entries, listing)
			}
			HomeComponentValue::Vertical { entries, listing } => {
				assert!(entries.is_empty());
				assert!(listing.is_some());
				continue;
			}
			_ => panic!("unexpected component"),
		};
		println!("{:?}: {}", component.title, entries.len());
		assert!(!entries.is_empty());
		assert!(listing.is_some());
	}
}

#[buny_test]
fn test_deep_links() {
	let source = FenrirRealm::new();
	assert!(matches!(
		source
			.handle_deep_link("https://fenrirealm.com/series/absolute-regression".into())
			.unwrap(),
		Some(DeepLinkResult::Novel { key }) if key == REGRESSION
	));
	assert!(matches!(
		source
			.handle_deep_link("https://fenrirealm.com/series/absolute-regression/1?x=1".into())
			.unwrap(),
		Some(DeepLinkResult::Chapter { novel_key, key }) if novel_key == REGRESSION && key == "12471"
	));
	// A chapter inside a volume.
	assert!(matches!(
		source
			.handle_deep_link(
				"https://fenrirealm.com/series/empress-call-me-by-my-title-at-work/vol-1/1".into()
			)
			.unwrap(),
		Some(DeepLinkResult::Chapter { key, .. }) if key == "93625"
	));
	assert!(
		source
			.handle_deep_link("https://fenrirealm.com/store".into())
			.unwrap()
			.is_none()
	);
	assert!(
		source
			.handle_deep_link("https://example.com/series/x".into())
			.unwrap()
			.is_none()
	);
}

// ---- offline ----

#[buny_test]
fn test_split_label() {
	assert_eq!(split_label("Chapter 12"), Some((12.0, "")));
	assert_eq!(split_label(" Chapter 21.5"), Some((21.5, "")));
	assert_eq!(
		split_label("Chapter 765. Reversal."),
		Some((765.0, "Reversal."))
	);
	assert_eq!(
		split_label("Chapter 93: Name (13)"),
		Some((93.0, "Name (13)"))
	);
	assert_eq!(
		split_label("Chapter 1 — Regression"),
		Some((1.0, "Regression"))
	);
	assert_eq!(
		split_label("43. Abnormal Weather (3)"),
		Some((43.0, "Abnormal Weather (3)"))
	);
	assert_eq!(
		split_label("1. Prologue 1: The Great War."),
		Some((1.0, "Prologue 1: The Great War."))
	);
	assert_eq!(split_label("100,000 Spirit Stones"), None);
	assert_eq!(split_label("1st Circle"), None);
	assert_eq!(split_label("Chapters of Life"), None);
	assert_eq!(split_label("Prologue"), None);
	assert_eq!(split_label("Chapter . 5"), Some((5.0, "")));
}

#[buny_test]
fn test_chapter_title() {
	assert_eq!(
		chapter_title(" Chapter 1", Some("Regression"), Some(1.0)).as_deref(),
		Some("Regression")
	);
	assert_eq!(chapter_title(" Chapter 1", None, Some(1.0)), None);
	assert_eq!(
		chapter_title(" Chapter 1", Some("Chapter 01"), Some(1.0)),
		None
	);
	assert_eq!(chapter_title(" Chapter 17", Some(""), Some(17.0)), None);
	assert_eq!(
		chapter_title("", Some("Chapter 765. Reversal."), Some(765.0)).as_deref(),
		Some("Reversal.")
	);
	assert_eq!(
		chapter_title("Chapter 1 - Interview", Some("Interview"), Some(1.0)).as_deref(),
		Some("Interview")
	);
	assert_eq!(
		chapter_title("Chapter 71 - The Ceremony Begins!", None, Some(71.0)).as_deref(),
		Some("The Ceremony Begins!")
	);
	// The label stays when it isn't the number sent.
	assert_eq!(
		chapter_title(" Chapter 820", Some("Chapter 830. Core Room."), Some(830.0)).as_deref(),
		Some("Core Room.")
	);
	assert_eq!(
		chapter_title(" Chapter 820", Some("Chapter 830. Core Room."), None).as_deref(),
		Some("Chapter 830. Core Room.")
	);
	assert_eq!(
		chapter_title("", Some("Prologue"), Some(0.0)).as_deref(),
		Some("Prologue")
	);
}

fn chapter_json(id: i64, number: f64, part: Option<i64>, group: Option<i64>) -> Value {
	serde_json::json!({
		"id": id,
		"slug": format!("{number}"),
		"name": format!(" Chapter {number}"),
		"title": null,
		"number": number,
		"part": part,
		"index": id - 1,
		"group": group.map(|g| serde_json::json!({"index": g, "slug": format!("vol-{g}")})),
		"locked": {"price": if id > 3 { 15 } else { 0 }},
		"created_at": "2025-01-17T01:11:56.000000Z",
	})
}

#[buny_test]
fn test_parse_chapters() {
	// Parts become decimals.
	let items = [
		chapter_json(1, 17.0, None, None),
		chapter_json(2, 17.0, Some(1), None),
		chapter_json(3, 17.0, Some(5), None),
		chapter_json(4, 18.0, None, None),
	];
	let chapters = parse_chapters(&items, "s");
	let numbers: Vec<_> = chapters.iter().map(|c| c.chapter_number.unwrap()).collect();
	assert_eq!(numbers, [17.0, 17.1, 17.5, 18.0]);
	assert_eq!(chapters[0].key, "1");
	assert!(!chapters[2].locked && chapters[3].locked);

	// A repeated number (a site typo) numbers the whole series by position.
	let items = [
		chapter_json(1, 1.0, None, None),
		chapter_json(2, 2.0, None, None),
		chapter_json(3, 2.0, None, None),
	];
	let numbers: Vec<_> = parse_chapters(&items, "s")
		.iter()
		.map(|c| c.chapter_number.unwrap())
		.collect();
	assert_eq!(numbers, [1.0, 2.0, 3.0]);

	// Volumes come interleaved (deathmage, miss-demon-king): the list starts
	// with volume 2's first chapter, and `index` restarts in each volume.
	// Numbers that restart per volume are kept, with the volume.
	let mut items = [
		chapter_json(3, 1.0, None, Some(2)),
		chapter_json(1, 1.0, None, Some(1)),
		chapter_json(4, 2.0, None, Some(2)),
		chapter_json(2, 2.0, None, Some(1)),
	];
	for (item, index) in items.iter_mut().zip([0, 0, 1, 1]) {
		item["index"] = index.into();
	}
	let chapters = parse_chapters(&items, "s");
	let order: Vec<_> = chapters
		.iter()
		.map(|c| (c.key.as_str(), c.volume_number, c.chapter_number))
		.collect();
	assert_eq!(
		order,
		[
			("1", Some(1.0), Some(1.0)),
			("2", Some(1.0), Some(2.0)),
			("3", Some(2.0), Some(1.0)),
			("4", Some(2.0), Some(2.0)),
		]
	);
}

#[buny_test]
fn test_html_watermarks() {
	let html = "<style>.cb6e4a2dbdd5b{position:absolute;width:1px;height:1px;clip:rect(0,0,0,0);clip-path:inset(50%)}</style>\
		<div class=\"cb6e4a2dbdd5b\" aria-hidden=\"true\">e4y3J1pbdeidIb68IekcFcE5cem7h1E6a5=2=8</div>\
		<p>An unbelievable event has occurred.</p>\
		<div class=\"cb6e4a2dbdd5b\" aria-hidden=\"true\">e4y3J1pbdeidIb68</div>\
		<p>Where \u{200d}\u{200b}\u{200c}\u{200c}am <strong>I </strong>now?</p>\
		<span class=\"cb6e4a2dbdd5b\">hidden by class only</span>\
		<div data-variation=\"blue-inline\"><div class=\"content\"><p>【Status Window】</p></div></div>\
		<p>&lt;Dark Knight&gt; &amp; <em>friends</em>&nbsp;left.</p>\
		<p>***</p><hr><p>Prev I TOC I Next</p>";
	let blocks = content::html_blocks(html, &[" Chapter 1", ""]);
	let text = paragraphs(&blocks);
	assert_eq!(
		text,
		[
			"An unbelievable event has occurred.",
			"Where am **I** now?",
			"【Status Window】",
			"<Dark Knight> & *friends* left.",
		]
	);
	assert!(!matches!(blocks.last(), Some(ContentBlock::Divider)));
}

#[buny_test]
fn test_heading_and_credits() {
	let html = "<p>༺ 𓆩  Chapter 1 — Regression  𓆪 ༻</p>\
		<p>「Translator: Someone」</p><p>᠃</p>\
		<p>Chapter 1 is where it began.</p><p>……</p><p>(End of this chapter)</p><p>᠃ ⚘᠂ ⚘ ᠃</p>";
	let text: Vec<String> = paragraphs(&content::html_blocks(html, &[" Chapter 1", "Regression"]))
		.into_iter()
		.map(String::from)
		.collect();
	assert_eq!(text, ["Chapter 1 is where it began.", "……"]);

	// The first chapter of a volume names the volume above the heading.
	let html =
		"<p>Volume 1: The Grand Scholar of Dongsa</p><p>Chapter 1. Smoking Ice</p><p>Story.</p>";
	let blocks = content::html_blocks(html, &[" Chapter 1", ""]);
	assert_eq!(paragraphs(&blocks), ["Story."]);

	// The title alone as the heading; a story line repeating it later stays.
	let html = "<p>Father and Son (1)</p><p>Story.</p><p>Father and Son (1)</p>";
	let blocks = content::html_blocks(html, &[" Chapter 1", "Father and Son (1)"]);
	assert_eq!(paragraphs(&blocks), ["Story.", "Father and Son (1)"]);
}

#[buny_test]
fn test_doc_body() {
	let doc: Value = serde_json::from_str(
		r#"{"type":"systemWindow","content":[
		{"type":"paragraph","content":[{"type":"text","text":"Translator: FenrirTL "}]},
		{"type":"paragraph","content":[{"type":"text","text":"======================== "}]},
		{"type":"paragraph","content":[]},
		{"type":"paragraph","content":[{"type":"text","text":"< Chapter 1: Send Me to the Past > "}]},
		{"type":"paragraph","content":[{"type":"text","text":"He said "},{"type":"text","text":"no","marks":[{"type":"bold"}]},{"type":"text","text":" twice."}]},
		{"type":"horizontalRule"},
		{"type":"paragraph","content":[{"type":"text","text":"Line one"},{"type":"hardBreak"},{"type":"text","text":"Line two","marks":[{"type":"italic"}]}]}
		]}"#,
	)
	.unwrap();
	let blocks = content::doc_blocks(&doc, &[" Chapter 1", "Send Me to the Past"]);
	assert_eq!(
		paragraphs(&blocks),
		["He said **no** twice.", "Line one", "*Line two*"]
	);
	assert!(matches!(blocks[1], ContentBlock::Divider));
}

#[buny_test]
fn test_plain_text() {
	assert_eq!(
		content::plain_text(
			"<p style=\"text-align: left;\">One &amp; two</p><p></p><hr /><p>Three</p>"
		),
		"One & two\n\nThree"
	);
}
