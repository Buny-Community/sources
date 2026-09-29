use super::*;
use buny::{
	DeepLinkHandler, DeepLinkResult, Home, HomeComponentValue, Listing, ListingProvider, alloc::vec,
};
use buny_test::buny_test;

const REGRESSOR: &str = "a-regressors-tale-of-cultivation";

fn paragraphs(blocks: &[ContentBlock]) -> Vec<&str> {
	blocks
		.iter()
		.filter_map(|b| match b {
			ContentBlock::Paragraph(text, _) => Some(text.as_str()),
			_ => None,
		})
		.collect()
}

#[buny_test]
fn test_search_and_novel() {
	let source = WeTriedTLS::new();
	let result = source
		.get_search_novel_list(Some("regressor".into()), 1, Vec::new())
		.unwrap();
	let entry = result
		.entries
		.iter()
		.find(|n| n.key == REGRESSOR)
		.expect("search result")
		.clone();
	assert!(
		entry
			.cover
			.as_deref()
			.unwrap()
			.contains("/_next/image?url=")
	);

	let novel = source.get_novel_update(entry, true, true, 1).unwrap();
	println!(
		"{} {:?} {:?} {:?} {:?}",
		novel.title, novel.authors, novel.status, novel.content_rating, novel.tags
	);
	assert_eq!(novel.title, "A Regressor’s Tale of Cultivation");
	assert!(
		novel
			.description
			.as_deref()
			.unwrap()
			.starts_with("On the way")
	);
	assert!(!novel.description.as_deref().unwrap().contains('<'));
	assert_eq!(novel.status, NovelStatus::Ongoing);
	assert!(
		novel
			.tags
			.as_ref()
			.unwrap()
			.iter()
			.any(|t| t == "Cultivation")
	);

	// 880+ free chapters: page 1 is full and in reading order.
	assert_eq!(novel.has_more_chapters, Some(true));
	let chapters = novel.chapters.clone().unwrap();
	assert_eq!(chapters.len(), CHAPTER_PAGE_SIZE as usize);
	assert_eq!(chapters[0].key, "chapter-0");
	assert_eq!(chapters[0].title.as_deref(), Some("Prologue"));
	assert_eq!(chapters[0].chapter_number, Some(0.0));
	assert_eq!(chapters[1].title.as_deref(), Some("Regressor's First Day"));
	assert!(chapters[1].date_uploaded.is_some());
	assert!(chapters.iter().all(|c| !c.locked));

	// The last page ends with the paid chapters, locked, in number order.
	let last = source
		.get_novel_update(novel.clone(), false, true, 2)
		.unwrap();
	assert_eq!(last.has_more_chapters, Some(false));
	let last = last.chapters.unwrap();
	println!(
		"page 2: {} chapters, {} locked, last {:?}",
		last.len(),
		last.iter().filter(|c| c.locked).count(),
		last.last()
	);
	assert!(last.iter().any(|c| c.locked));
	assert!(last.last().unwrap().locked);
	assert!(
		last.windows(2)
			.all(|w| w[0].chapter_number <= w[1].chapter_number)
	);

	let content = source
		.get_chapter_content_list(novel.clone(), chapters[1].clone())
		.unwrap();
	let text = paragraphs(&content);
	println!("{:?}", &text[..3]);
	assert!(text.len() > 50);
	// Credits and the repeated heading are gone; the story starts right away.
	assert!(text[0].starts_with("\"What's happening?"));
	assert!(text.iter().all(|t| !t.to_lowercase().contains("discord")));

	let locked = last.iter().rev().find(|c| c.locked).unwrap().clone();
	let err = source.get_chapter_content_list(novel, locked);
	println!("{err:?}");
	assert!(err.is_err());
}

#[buny_test]
fn test_content_variants() {
	let source = WeTriedTLS::new();
	for (series, chapter) in [
		// Credits + "◈ Title" + "Chapter N: Name" + <hr>, footnotes header at the end.
		("friedrichs-battlefield", "chapter-93"),
		// <span>-wrapped credits and <br dir="auto"> spacers.
		("memoir-of-a-regressor", "chapter-22"),
		// Real footnotes that must survive the tail trim.
		("even-in-a-dark-fantasy-you-still-have-to-eat", "chapter-1"),
	] {
		let novel = Novel {
			key: series.into(),
			..Default::default()
		};
		let ch = Chapter {
			key: chapter.into(),
			..Default::default()
		};
		let blocks = source.get_chapter_content_list(novel, ch).unwrap();
		let text = paragraphs(&blocks);
		println!(
			"{series}/{chapter}: {} blocks, first {:?}, last {:?}",
			blocks.len(),
			text.first(),
			text.last()
		);
		assert!(text.len() > 50, "{series}");
		assert!(!matches!(blocks.first(), Some(ContentBlock::Divider)));
		assert!(!matches!(blocks.last(), Some(ContentBlock::Divider)));
		assert!(
			text.iter()
				.all(|t| !t.contains("dsc.gg") && !t.starts_with('◈'))
		);
		assert!(
			text.iter()
				.all(|t| !t.contains("** **") && !t.contains("****"))
		);
		assert!(
			blocks
				.windows(2)
				.all(|w| !(matches!(w[0], ContentBlock::Divider)
					&& matches!(w[1], ContentBlock::Divider)))
		);
		if series.starts_with("even-in") {
			assert!(text.iter().any(|t| t.starts_with("“Rice farming”")));
		}
	}
}

#[buny_test]
fn test_filters() {
	let source = WeTriedTLS::new();
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
	assert!(!completed.entries.is_empty());
	assert!(
		completed
			.entries
			.iter()
			.all(|n| n.status == NovelStatus::Completed)
	);

	// Cultivation (58) + Regression (21): series tagged with both.
	let tagged = source
		.get_search_novel_list(
			None,
			1,
			vec![FilterValue::MultiSelect {
				id: "tags".into(),
				included: vec!["58".into(), "21".into()],
				excluded: Vec::new(),
			}],
		)
		.unwrap();
	println!(
		"tags: {:?}",
		tagged.entries.iter().map(|n| &n.key).collect::<Vec<_>>()
	);
	assert!(!tagged.entries.is_empty());
	assert!(tagged.entries.iter().all(|n| {
		let tags = n.tags.as_ref().unwrap();
		tags.iter().any(|t| t == "Cultivation") && tags.iter().any(|t| t == "Regression")
	}));

	let by_title = source
		.get_search_novel_list(
			None,
			1,
			vec![FilterValue::Sort {
				id: "sort".into(),
				index: 4,
				ascending: true,
			}],
		)
		.unwrap();
	let titles: Vec<_> = by_title
		.entries
		.iter()
		.map(|n| n.title.to_lowercase())
		.collect();
	println!("title asc: {:?}", &titles[..3]);
	assert!(by_title.has_next_page);
	// The database collation doesn't match Rust's string order ("30 Years",
	// "Academy's", "A Knight"), so check the ends rather than every pair.
	assert!(titles[0].starts_with(|c: char| c.is_ascii_digit() || c == 'a'));
	assert!(titles[0] < titles[titles.len() - 1]);
}

#[buny_test]
fn test_listings() {
	let source = WeTriedTLS::new();
	for id in ["popular", "latest", "newest", "top_rated"] {
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
		assert_eq!(result.entries.len(), NOVEL_PAGE_SIZE as usize, "{id}");
		assert!(result.has_next_page, "{id}");
		let page2 = source.get_novel_list(listing, 2).unwrap();
		assert!(!page2.entries.is_empty(), "{id}");
		assert_ne!(page2.entries[0].key, result.entries[0].key, "{id}");
	}
}

#[buny_test]
fn test_home() {
	let home = WeTriedTLS::new().get_home().unwrap();
	assert_eq!(home.components.len(), 4);
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
		&home.components[3].value,
		HomeComponentValue::Vertical { entries, listing: Some(l) } if entries.is_empty() && l.id == "latest"
	));
}

#[buny_test]
fn test_deep_links() {
	let source = WeTriedTLS::new();
	assert_eq!(
		source
			.handle_deep_link(format!("{BASE_URL}/series/{REGRESSOR}"))
			.unwrap(),
		Some(DeepLinkResult::Novel {
			key: REGRESSOR.into()
		})
	);
	assert_eq!(
		source
			.handle_deep_link(format!("{BASE_URL}/series/{REGRESSOR}/chapter-1?x=1"))
			.unwrap(),
		Some(DeepLinkResult::Chapter {
			novel_key: REGRESSOR.into(),
			key: "chapter-1".into()
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
			.handle_deep_link("https://example.com/series/x".into())
			.unwrap(),
		None
	);
}

#[buny_test]
fn test_chapter_titles() {
	assert_eq!(
		chapter_title("Chapter 1", Some("Regressor's First Day")).as_deref(),
		Some("Regressor's First Day")
	);
	assert_eq!(
		chapter_title("Chapter 0", Some("Prologue")).as_deref(),
		Some("Prologue")
	);
	assert_eq!(chapter_title("Chapter 18", None), None);
	assert_eq!(
		chapter_title(" Chapter 656", Some("Heaven's Key (4)")).as_deref(),
		Some("Heaven's Key (4)")
	);
	assert_eq!(
		chapter_title("Chatper 234", Some("Depth (1)")).as_deref(),
		Some("Depth (1)")
	);
	assert_eq!(
		chapter_title("Chapter 526", Some("Chapter 526: Seo Hweol's Memories (3)")).as_deref(),
		Some("Seo Hweol's Memories (3)")
	);
	assert_eq!(
		chapter_title("Chapter 186 - END", None).as_deref(),
		Some("END")
	);
	assert_eq!(
		chapter_title("Author's Q&A (2)", Some("Author's Q&A (2)")).as_deref(),
		Some("Author's Q&A (2)")
	);
	assert_eq!(
		chapter_title(
			"Author's Tidbit (1)",
			Some("Seo Ran, Buk Hyang-hwa, and Yuan Li")
		)
		.as_deref(),
		Some("Author's Tidbit (1): Seo Ran, Buk Hyang-hwa, and Yuan Li")
	);
	assert_eq!(chapter_title("Prologue", None).as_deref(), Some("Prologue"));
	assert_eq!(
		chapter_title("Chaos 12", Some("X")).as_deref(),
		Some("Chaos 12: X")
	);
}

#[buny_test]
fn test_unescape() {
	assert_eq!(
		unescape("a&nbsp;b &amp;lt; &#39;x&#x27; &rsquo;"),
		"a\u{a0}b &lt; 'x' ’"
	);
	assert_eq!(unescape("Q&A & more;"), "Q&A & more;");
	assert_eq!(unescape("&lt;Dark Knight&gt;"), "<Dark Knight>");
}

#[buny_test]
fn test_content_conversion() {
	let html = concat!(
		r#"<p dir="auto"><strong>WE TRIED TRANSLATIONS</strong></p><p dir="auto"><br></p>"#,
		r#"<p dir="auto"><strong>Translator: Abood</strong></p>"#,
		r#"<p dir="auto"><strong>Discord: </strong><a href="https://dsc.gg/wetried">https://dsc.gg/wetried</a></p>"#,
		r#"<p dir="auto"><strong>◈ Friedrich's Battlefield</strong></p>"#,
		r#"<p dir="auto"><strong>Chapter 93: Stalling for Time (1)</strong></p><div data-type="horizontalRule"><hr></div>"#,
		r#"<p dir="auto"></p><p dir="auto">“I am <em>Chester</em>,<strong> knight </strong>of &lt;Rigald&gt;.”</p>"#,
		r#"<p dir="auto">* * *</p><div data-type="horizontalRule"><hr></div>"#,
		r#"<p dir="auto">Line one<br>Line&nbsp;two</p>"#,
		r#"<p dir="auto"><img src="https://media.reaperscans.net/file/x.jpg" width="500"></p>"#,
		r#"<p dir="auto">Footnotes:&nbsp;</p>"#,
		r#"<p dir="auto">Join our discord at <a href="https://dsc.gg/wetried"><u>https://dsc.gg/wetried</u></a></p><p dir="auto"><br></p>"#,
	);
	let blocks = content::chapter_blocks(
		html,
		&[
			"Chapter 93",
			"Stalling for Time (1)",
			"Friedrich's Battlefield",
		],
	);
	println!("{blocks:?}");
	assert_eq!(
		blocks,
		vec![
			ContentBlock::paragraph("“I am *Chester*, **knight** of <Rigald>.”", None),
			ContentBlock::Divider,
			ContentBlock::paragraph("Line one", None),
			ContentBlock::paragraph("Line two", None),
			ContentBlock::paragraph(
				"[Illustration](https://media.reaperscans.net/file/x.jpg)",
				None
			),
		]
	);

	assert_eq!(
		content::plain_text(
			r#"<p dir="auto">On the way.</p><p dir="auto">Until I <em>regressed</em>.</p>"#
		),
		"On the way.\n\nUntil I regressed."
	);
}
