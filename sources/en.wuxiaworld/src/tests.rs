use super::*;
use buny::{
	DeepLinkHandler, DeepLinkResult, Home, HomeComponentValue, Listing, ListingProvider, alloc::vec,
};
use buny_test::buny_test;

// 7,300+ chapters, extras numbered 0.1-0.75 before chapter 1, free chapters
// first and the rest locked.
const NSHBA: &str = "nine-star-hegemon";
// "Book 1, Chapter 1": the API's `number` is a sort key (1.001), so chapters
// are numbered by position.
const CITY_OF_SIN: &str = "city-of-sin";

fn paragraphs(blocks: &[ContentBlock]) -> Vec<&str> {
	blocks
		.iter()
		.filter_map(|b| match b {
			ContentBlock::Paragraph(text, _) => Some(text.as_str()),
			_ => None,
		})
		.collect()
}

fn novel(key: &str) -> Novel {
	Novel {
		key: key.into(),
		..Default::default()
	}
}

#[buny_test]
fn test_search_and_novel() {
	let source = Wuxiaworld::new();
	let result = source
		.get_search_novel_list(Some("nine star".into()), 1, Vec::new())
		.unwrap();
	let entry = result
		.entries
		.iter()
		.find(|n| n.key == NSHBA)
		.expect("search result")
		.clone();
	assert!(
		entry
			.cover
			.as_deref()
			.unwrap()
			.starts_with("https://cdn.wuxiaworld.com/")
	);
	assert!(entry.description.is_some());

	let novel = source.get_novel_update(entry, true, true, 1).unwrap();
	println!(
		"{} {:?} {:?} {:?} {:?}",
		novel.title, novel.authors, novel.status, novel.content_rating, novel.tags
	);
	assert_eq!(novel.title, "Nine Star Hegemon Body Art");
	assert_eq!(
		novel.authors.as_deref(),
		Some(&["Ordinary Magician (平凡魔术师)".to_string()][..])
	);
	let description = novel.description.as_deref().unwrap();
	assert!(description.starts_with("Long Chen, a crippled youth"));
	assert!(!description.contains('<') && !description.contains("&#39;"));
	assert_eq!(novel.status, NovelStatus::Ongoing);
	assert!(novel.tags.as_ref().unwrap().iter().any(|t| t == "Xianxia"));
	assert_eq!(
		novel.url.as_deref(),
		Some("https://www.wuxiaworld.com/novel/nine-star-hegemon")
	);

	// One page with everything, in reading order, numbered as the site names them.
	assert_eq!(novel.has_more_chapters, Some(false));
	let chapters = novel.chapters.clone().unwrap();
	println!("{} chapters, first {:?}", chapters.len(), chapters.first());
	assert!(chapters.len() > 7000);
	assert!(
		chapters
			.windows(2)
			.all(|w| w[0].chapter_number < w[1].chapter_number)
	);
	let extra = &chapters[0];
	assert_eq!(extra.key, "nshba-chapter-0-1");
	assert_eq!(extra.chapter_number, Some(0.1));
	assert_eq!(
		extra.title.as_deref(),
		Some("Leng Yueyan Chapter 1 The Peace of the Countryside is Broken")
	);
	let first = chapters
		.iter()
		.find(|c| c.key == "nshba-chapter-1")
		.unwrap();
	assert_eq!(first.chapter_number, Some(1.0));
	assert_eq!(first.title.as_deref(), Some("Memories of a Pill Sovereign"));
	assert!(!first.locked);
	assert!(first.date_uploaded.is_some_and(|d| d > 1_500_000_000));
	assert_eq!(
		first.url.as_deref(),
		Some("https://www.wuxiaworld.com/novel/nine-star-hegemon/nshba-chapter-1")
	);
	// "Chapter 7320" with nothing after it: the number is the whole title.
	let last = chapters.last().unwrap();
	assert!(last.locked);
	assert!(last.title.is_none() || !last.title.as_deref().unwrap().starts_with("Chapter"));

	let content = source
		.get_chapter_content_list(novel.clone(), first.clone())
		.unwrap();
	let text = paragraphs(&content);
	println!("{:?}", &text[..3]);
	assert!(text.len() > 50);
	assert_eq!(text[0], "“Who am I? I am… Long Chen!”");
	assert!(text.iter().all(|t| !t.contains('<') || t.contains("&lt;")));
	assert!(!matches!(content.last(), Some(ContentBlock::Divider)));

	let err = source
		.get_chapter_content_list(novel, last.clone())
		.unwrap_err();
	println!("{err:?}");
	assert!(format!("{err:?}").contains("Karma"));
}

#[buny_test]
fn test_position_numbered_series() {
	let source = Wuxiaworld::new();
	let novel = source
		.get_novel_update(novel(CITY_OF_SIN), false, true, 1)
		.unwrap();
	let chapters = novel.chapters.unwrap();
	println!("{:?}", &chapters[..3]);
	assert!(
		chapters
			.windows(2)
			.all(|w| w[0].chapter_number < w[1].chapter_number)
	);
	assert!(
		chapters
			.iter()
			.all(|c| c.chapter_number.unwrap() as i64 as f32 == c.chapter_number.unwrap())
	);
	// The book and chapter stay in the title, since the app's number is the position.
	assert!(chapters.iter().any(|c| {
		c.title
			.as_deref()
			.is_some_and(|t| t.starts_with("Book 2, Chapter"))
	}));
}

#[buny_test]
fn test_missing() {
	let source = Wuxiaworld::new();
	assert!(
		source
			.get_novel_update(novel("no-such-novel-xyz"), true, true, 1)
			.is_err()
	);
	let chapter = Chapter {
		key: "bogus".into(),
		..Default::default()
	};
	assert!(
		source
			.get_chapter_content_list(novel(NSHBA), chapter)
			.is_err()
	);
}

#[buny_test]
fn test_translator_thoughts() {
	let source = Wuxiaworld::new();
	let chapter = Chapter {
		key: "ihas-chapter-1".into(),
		..Default::default()
	};
	let blocks = source
		.get_chapter_content_list(novel("i-have-a-sword"), chapter)
		.unwrap();
	let text = paragraphs(&blocks);
	let at = text
		.iter()
		.position(|t| *t == "**Translator's thoughts**")
		.expect("translator's thoughts");
	println!("{:?}", &text[at..]);
	assert!(text[at + 1].starts_with("Hi, we are Coca"));
	assert!(text[at - 1].starts_with("Ye Guan was rendered speechless"));
}

// Past the free chapters the API still sends ~200 words of the chapter, flagged
// as a teaser. That must fail rather than read as the whole chapter.
#[buny_test]
fn test_teasers() {
	let source = Wuxiaworld::new();
	let open = |novel_key: &str, key: &str| {
		let chapter = Chapter {
			key: key.into(),
			..Default::default()
		};
		source.get_chapter_content_list(novel(novel_key), chapter)
	};
	let err = open(NSHBA, "nshba-chapter-3711").unwrap_err();
	println!("{err:?}");
	assert!(format!("{err:?}").contains("Only a preview is free"));

	// Keyboard Immortal flags no chapter free, but serves the first 50 in full.
	let ki = source
		.get_novel_update(novel("keyboard-immortal"), false, true, 1)
		.unwrap()
		.chapters
		.unwrap();
	assert!(ki.iter().all(|c| c.locked));
	assert!(paragraphs(&open("keyboard-immortal", &ki[0].key).unwrap()).len() > 50);
	assert!(open("keyboard-immortal", &ki[50].key).is_err());
}

#[buny_test]
fn test_old_site_navigation() {
	let chapter = Chapter {
		key: "wdqk-chapter-1".into(),
		..Default::default()
	};
	let blocks = Wuxiaworld::new()
		.get_chapter_content_list(novel("wu-dong-qian-kun"), chapter)
		.unwrap();
	let text = paragraphs(&blocks);
	println!("{:?} .. {:?}", text.first(), text.last());
	assert!(!text[0].contains("Chapter"));
	assert!(!text.last().unwrap().contains("Previous Chapter"));
}

#[buny_test]
fn test_filters() {
	let source = Wuxiaworld::new();
	let completed = source
		.get_search_novel_list(
			None,
			1,
			vec![FilterValue::Select {
				id: "status".into(),
				value: "Completed".into(),
			}],
		)
		.unwrap();
	assert_eq!(completed.entries.len(), PAGE_SIZE as usize);
	assert!(
		completed
			.entries
			.iter()
			.all(|n| n.status == NovelStatus::Completed)
	);

	let hiatus = source
		.get_search_novel_list(
			None,
			1,
			vec![FilterValue::Select {
				id: "status".into(),
				value: "Hiatus".into(),
			}],
		)
		.unwrap();
	assert!(!hiatus.entries.is_empty());
	assert!(
		hiatus
			.entries
			.iter()
			.all(|n| n.status == NovelStatus::Hiatus)
	);

	let korean = source
		.get_search_novel_list(
			None,
			1,
			vec![FilterValue::Select {
				id: "language".into(),
				value: "Korean".into(),
			}],
		)
		.unwrap();
	println!(
		"korean: {:?}",
		korean.entries.iter().map(|n| &n.key).collect::<Vec<_>>()
	);
	assert!(korean.entries.iter().any(|n| n.key == "overgeared"));
	assert!(korean.entries.iter().all(|n| n.key != NSHBA));

	// Genres match novels that have all of them.
	let genres = source
		.get_search_novel_list(
			None,
			1,
			vec![FilterValue::MultiSelect {
				id: "genres".into(),
				included: vec!["Comedy".into(), "Romance".into()],
				excluded: Vec::new(),
			}],
		)
		.unwrap();
	assert!(!genres.entries.is_empty());
	assert!(genres.entries.iter().all(|n| {
		let tags = n.tags.as_ref().unwrap();
		tags.iter().any(|t| t == "Comedy") && tags.iter().any(|t| t == "Romance")
	}));

	let by_name = source
		.get_search_novel_list(
			None,
			1,
			vec![FilterValue::Sort {
				id: "sort".into(),
				index: 5,
				ascending: true,
			}],
		)
		.unwrap();
	let titles: Vec<_> = by_name
		.entries
		.iter()
		.map(|n| n.title.to_lowercase())
		.collect();
	println!("name asc: {:?}", &titles[..3]);
	assert!(titles[0] < titles[titles.len() - 1]);
}

// Pages come from cursor paging; walking all of them must visit every novel once.
#[buny_test]
fn test_paging() {
	let source = Wuxiaworld::new();
	let listing = Listing {
		id: "popular".into(),
		..Default::default()
	};
	let mut keys = Vec::new();
	let mut page = 1;
	loop {
		let result = source.get_novel_list(listing.clone(), page).unwrap();
		keys.extend(result.entries.into_iter().map(|n| n.key));
		if !result.has_next_page {
			break;
		}
		page += 1;
		assert!(page < 30, "no end");
	}
	let count = keys.len();
	keys.sort();
	keys.dedup();
	println!("{page} pages, {count} novels");
	assert_eq!(keys.len(), count);
	assert!(count > 200);
	assert!(
		source
			.get_novel_list(listing, page + 1)
			.unwrap()
			.entries
			.is_empty()
	);
}

#[buny_test]
fn test_listings() {
	let source = Wuxiaworld::new();
	for id in ["popular", "trending", "new", "top_rated", "completed"] {
		let listing = Listing {
			id: id.into(),
			..Default::default()
		};
		let result = source.get_novel_list(listing.clone(), 1).unwrap();
		println!(
			"{id}: {} entries, next={}, first={}",
			result.entries.len(),
			result.has_next_page,
			result.entries[0].key
		);
		assert_eq!(result.entries.len(), PAGE_SIZE as usize, "{id}");
		assert!(result.has_next_page, "{id}");
		let page2 = source.get_novel_list(listing, 2).unwrap();
		assert!(!page2.entries.is_empty(), "{id}");
		assert!(
			page2
				.entries
				.iter()
				.all(|n| result.entries.iter().all(|m| m.key != n.key)),
			"{id}"
		);
	}
}

#[buny_test]
fn test_home() {
	let home = Wuxiaworld::new().get_home().unwrap();
	assert_eq!(home.components.len(), 5);
	for c in &home.components {
		let count = match &c.value {
			HomeComponentValue::Details { entries, .. }
			| HomeComponentValue::Scroller { entries, .. }
			| HomeComponentValue::Stack { entries, .. }
			| HomeComponentValue::Vertical { entries, .. } => entries.len(),
			_ => 0,
		};
		println!("{:?}: {count}", c.title);
		if !matches!(c.value, HomeComponentValue::Vertical { .. }) {
			assert!(count > 0, "{:?}", c.title);
		}
	}
	assert!(matches!(
		&home.components[4].value,
		HomeComponentValue::Vertical { entries, listing: Some(l) } if entries.is_empty() && l.id == "popular"
	));
}

#[buny_test]
fn test_deep_links() {
	let source = Wuxiaworld::new();
	assert_eq!(
		source
			.handle_deep_link(format!("{BASE_URL}/novel/{NSHBA}"))
			.unwrap(),
		Some(DeepLinkResult::Novel { key: NSHBA.into() })
	);
	assert_eq!(
		source
			.handle_deep_link(format!(
				"https://wuxiaworld.com/novel/{NSHBA}/nshba-chapter-1?x=1"
			))
			.unwrap(),
		Some(DeepLinkResult::Chapter {
			novel_key: NSHBA.into(),
			key: "nshba-chapter-1".into()
		})
	);
	assert_eq!(
		source
			.handle_deep_link(format!("{BASE_URL}/novels"))
			.unwrap(),
		None
	);
	assert_eq!(
		source
			.handle_deep_link("https://example.com/novel/x".into())
			.unwrap(),
		None
	);
}

#[buny_test]
fn test_chapter_titles() {
	let t = |name: &str, number: f64| chapter_title(name, number);
	assert_eq!(
		t("Chapter 1 Memories of a Pill Sovereign", 1.0).as_deref(),
		Some("Memories of a Pill Sovereign")
	);
	assert_eq!(
		t("Chapter 0283: Glossary", 283.0).as_deref(),
		Some("Glossary")
	);
	assert_eq!(t("Chapter 7320", 7320.0), None);
	assert_eq!(t("Chapter: 846", 846.0), None);
	assert_eq!(
		t("AST: Chapter 573 - Stellar Horse", 573.0).as_deref(),
		Some("Stellar Horse")
	);
	assert_eq!(
		t("AST 2194 - Strange Feeling", 2194.0).as_deref(),
		Some("Strange Feeling")
	);
	assert_eq!(
		t("WDQK Chapter 26: The Hunt", 26.0).as_deref(),
		Some("The Hunt")
	);
	assert_eq!(
		t("CHapter 180 -  Shock and suspicions", 180.0).as_deref(),
		Some("Shock and suspicions")
	);
	assert_eq!(
		t("Ch 483 - Taking Off The Mask (2)", 483.0).as_deref(),
		Some("Taking Off The Mask (2)")
	);
	assert_eq!(t("Chpater 444", 444.0), None);
	assert_eq!(t("Episode 1. Prologue", 1.0).as_deref(), Some("Prologue"));
	assert_eq!(
		t("1180 - Abandonment", 1180.0).as_deref(),
		Some("Abandonment")
	);
	assert_eq!(
		t("Chapter 245: From The Ashes (2) Part 1", 245.1).as_deref(),
		Some("From The Ashes (2) Part 1")
	);
	// The number doesn't match the one the app shows: keep the name whole.
	assert_eq!(t("Chapter 7320", 7331.0).as_deref(), Some("Chapter 7320"));
	assert_eq!(
		t(" Book 7, Chapter 144 ", 1084.0).as_deref(),
		Some("Book 7, Chapter 144")
	);
	assert_eq!(
		t("Vol 2. Chapter 1: A Transcendent", 120.0).as_deref(),
		Some("Vol 2. Chapter 1: A Transcendent")
	);
	// Suffixes glued to the number are part of it.
	assert_eq!(
		t("MW Chapter 1929A", 1929.1).as_deref(),
		Some("MW Chapter 1929A")
	);
	assert_eq!(
		t("273-2 - Naming targets", 273.0).as_deref(),
		Some("273-2 - Naming targets")
	);
	assert_eq!(
		t("Chapter 197.1: Snow", 197.1).as_deref(),
		Some("Chapter 197.1: Snow")
	);
	assert_eq!(t("Prologue", 0.0).as_deref(), Some("Prologue"));
	assert_eq!(t("I Am 3", 3.0).as_deref(), Some("I Am 3"));
	assert_eq!(t("", 3.0), None);
}

#[buny_test]
fn test_numbering_choice() {
	let raw = |name: &'static str, number: f64, offset: i32| RawChapter {
		slug: "x",
		name,
		number: Some(number),
		offset,
		published: None,
		free: true,
	};
	assert!(numbers_are_labels(&[
		raw("Side Story 1", 0.1, 1),
		raw("Chapter 1", 1.0, 2),
		raw("Chapter 2", 2.0, 3),
		raw("Chapter 2 Part 2", 2.5, 4),
		raw("Chapter 3", 3.0, 5),
		raw("Chapter 4", 4.0, 6),
		raw("Chapter 5", 5.0, 7),
		raw("Chapter 6", 6.0, 8),
		raw("Chapter 7", 7.0, 9),
		raw("Chapter 8", 8.0, 10),
	]));
	// Book-style sort keys.
	assert!(!numbers_are_labels(&[
		raw("Book 1, Chapter 1", 1.001, 1),
		raw("Book 1, Chapter 2", 1.002, 2),
		raw("Book 2, Chapter 1", 2.001, 3),
	]));
	// Numbers that go backwards can't be used for sorting.
	assert!(!numbers_are_labels(&[
		raw("Chapter 2", 2.0, 1),
		raw("Chapter 1", 1.0, 2),
	]));
}

#[buny_test]
fn test_proto() {
	let mut w = Writer::default();
	w.int32(3, -1).string_value(1, "martial").int32(7, 300);
	// Negative enums take 10 bytes (sign-extended), as protobuf requires.
	assert_eq!(
		&w.as_bytes()[..11],
		&[
			0x18, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01
		]
	);
	let back = Message::new(w.as_bytes());
	assert_eq!(back.int32(3), Some(-1));
	assert_eq!(back.string_value(1), Some("martial"));
	assert_eq!(back.int32(7), Some(300));
	// DecimalValue { units: 0, nanos: 100000000 } is 0.1.
	let decimal = Message::new(&[0x22, 0x05, 0x15, 0x00, 0xe1, 0xf5, 0x05]);
	assert!((decimal.decimal(4).unwrap() - 0.1).abs() < 1e-9);
	// A truncated field ends iteration instead of panicking.
	assert_eq!(Message::new(&[0x0a, 0x10, 0x61]).fields().count(), 0);
}

#[buny_test]
fn test_unescape() {
	assert_eq!(
		unescape("a&nbsp;b &amp;lt; &#39;x&#x27; &rsquo; caf&eacute;"),
		"a\u{a0}b &lt; 'x' ’ café"
	);
	assert_eq!(unescape("Q&A & more;"), "Q&A & more;");
	assert_eq!(unescape("&lt;Dark Knight&gt;"), "<Dark Knight>");
}

#[buny_test]
fn test_content_conversion() {
	// wu-dong-qian-kun/wdqk-chapter-26 and killer-nights/kn-chapter-8 (trimmed).
	let wdqk = concat!(
		"<p>WDQK Chapter 26: The Hunt</p>",
		"<p>Ever since the Family Competition ended, it became much more lively.</p>",
		"<p>“Yes.”</p><p><hr></p>",
	);
	assert_eq!(
		content::chapter_blocks(wdqk, &["WDQK Chapter 26: The Hunt", "Wu Dong Qian Kun", ""]),
		vec![
			ContentBlock::paragraph(
				"Ever since the Family Competition ended, it became much more lively.",
				None
			),
			ContentBlock::paragraph("“Yes.”", None),
		]
	);

	let kn = concat!(
		r#"<p><h3><span style="text-decoration: underline;">Killer Nights</span></h3></p>"#,
		r#"<p><h3><span style="text-decoration: underline;">Chapter 8: Professionalism and Dignity</span></h3></p>"#,
		r#"<p><span style="">Jiang Zhengkai lingered. &nbsp;He didn&rsquo;t turn on the <em>lights</em>,<strong> just </strong>sat.</span></p>"#,
		r#"<p dir="ltr"><span>***</span></p><hr>"#,
		r#"<p dir="ltr"><span>&lt;Skill: Dash&gt; activated.</span><br>Line two</p>"#,
		r#"<p><img src="https://cdn.wuxiaworld.com/x.jpg"></p>"#,
		"<p><hr></p>",
	);
	let blocks = content::chapter_blocks(
		kn,
		&[
			"Chapter 8: Professionalism and Dignity",
			"Killer Nights",
			"Killer Nights",
		],
	);
	println!("{blocks:?}");
	assert_eq!(
		blocks,
		vec![
			ContentBlock::paragraph(
				"Jiang Zhengkai lingered.  He didn’t turn on the *lights*, **just** sat.",
				None
			),
			ContentBlock::Divider,
			ContentBlock::paragraph("<Skill: Dash> activated.", None),
			ContentBlock::paragraph("Line two", None),
			ContentBlock::paragraph("[Illustration](https://cdn.wuxiaworld.com/x.jpg)", None),
		]
	);

	// Old-site navigation at the edges goes; "contents" in the story stays.
	assert_eq!(
		content::chapter_blocks(
			"<p>Previous Chapter | Next Chapter</p><p>Chapter 1 - Prologue</p><p>The contents of the box.</p><p>Table of Contents</p>",
			&["Chapter 1 - Prologue", "", "Prologue"]
		),
		vec![ContentBlock::paragraph("The contents of the box.", None)]
	);
	// A list of chapter names (who-let-him-cultivate's extra-chapter index) is content.
	assert_eq!(
		content::chapter_blocks(
			"<p>This is not the final list.</p><p>Chapter 124.1</p><p>Chapter 989.1</p>",
			&["Index of Extra Chapters", "", ""]
		)
		.len(),
		3
	);
	// The bare title as a heading ("Chapter 1 - Prologue" opens with "Prologue").
	assert_eq!(
		content::chapter_blocks(
			"<p><strong>Prologue</strong></p><p>Kung!</p>",
			&["Chapter 1 - Prologue", "Ranker's Return", "Prologue"]
		),
		vec![ContentBlock::paragraph("Kung!", None)]
	);

	// An opening line that merely starts with a number is story text.
	assert_eq!(
		content::chapter_blocks("<p>474 people died.</p>", &["Chapter 474", "", ""]),
		vec![ContentBlock::paragraph("474 people died.", None)]
	);

	assert_eq!(
		content::plain_text("<p>Long Chen&#39;s path.</p><p>He <em>rose</em>.</p>"),
		"Long Chen's path.\n\nHe rose."
	);
}
