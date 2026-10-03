use super::*;
use buny::{
	DeepLinkHandler, DeepLinkResult, Home, HomeComponentValue, Listing, ListingProvider, alloc::vec,
};
use buny_test::buny_test;

const RUNESMITH: &str = "117137/the-runesmith";

fn paragraphs(blocks: &[ContentBlock]) -> Vec<&str> {
	blocks
		.iter()
		.filter_map(|b| match b {
			ContentBlock::Paragraph(text, _) => Some(text.as_str()),
			_ => None,
		})
		.collect()
}

fn search(filters: Vec<FilterValue>) -> NovelPageResult {
	ScribbleHub::new()
		.get_search_novel_list(None, 1, filters)
		.unwrap()
}

fn chapter_blocks(novel_key: &str, chapter_key: &str) -> Vec<ContentBlock> {
	ScribbleHub::new()
		.get_chapter_content_list(
			Novel {
				key: novel_key.into(),
				..Default::default()
			},
			Chapter {
				key: chapter_key.into(),
				..Default::default()
			},
		)
		.unwrap()
}

// ---- live ----

#[buny_test]
fn test_search_and_novel() {
	let source = ScribbleHub::new();
	let result = source
		.get_search_novel_list(Some("runesmith".into()), 1, Vec::new())
		.unwrap();
	let entry = result
		.entries
		.iter()
		.find(|n| n.key == RUNESMITH)
		.expect("search result")
		.clone();
	assert_eq!(entry.title, "The Runesmith");
	assert!(entry.cover.as_deref().unwrap().starts_with("https://"));
	assert!(entry.tags.as_ref().unwrap().iter().any(|t| t == "Fantasy"));

	let novel = source.get_novel_update(entry, true, true, 1).unwrap();
	println!(
		"{} {:?} {:?} {:?} {:?} {:?}",
		novel.title, novel.authors, novel.status, novel.content_rating, novel.cover, novel.tags
	);
	assert_eq!(novel.title, "The Runesmith");
	assert_eq!(novel.authors, Some(vec![String::from("Kuropon")]));
	assert_eq!(novel.status, NovelStatus::Ongoing);
	assert!(novel.cover.as_deref().unwrap().starts_with("https://"));
	let description = novel.description.as_deref().unwrap();
	assert!(description.contains("transported into a foreign world"));
	assert!(description.contains("\n\n"));
	assert!(!description.contains('<'));
	let tags = novel.tags.as_ref().unwrap();
	assert!(tags.iter().any(|t| t == "Action"), "genre");
	assert!(tags.iter().any(|t| t == "Blacksmith"), "tag");
	// "Strong Language" is its only warning.
	assert_eq!(novel.content_rating, ContentRating::Suggestive);

	// Every chapter in one reply, in reading order. A character sheet comes
	// before "Chapter 1", so it's numbered between 0 and 1.
	assert_eq!(novel.has_more_chapters, Some(false));
	let chapters = novel.chapters.unwrap();
	assert!(chapters.len() > 700, "{}", chapters.len());
	assert_eq!(chapters[0].key, "183306");
	assert_eq!(chapters[0].chapter_number, Some(0.5));
	assert_eq!(
		chapters[0].title.as_deref(),
		Some("Skills,Titles and other [Spoilers]")
	);
	assert_eq!(chapters[1].chapter_number, Some(1.0));
	assert_eq!(
		chapters[1].title.as_deref(),
		Some("So it begins… with a truck!")
	);
	assert_eq!(
		chapters[1].url.as_deref(),
		Some("https://www.scribblehub.com/read/117137-the-runesmith/chapter/117153/")
	);
	assert!(chapters.iter().all(|c| c.date_uploaded.is_some()));
	assert!(
		chapters
			.windows(2)
			.all(|w| w[0].chapter_number < w[1].chapter_number)
	);
}

#[buny_test]
fn test_chapter() {
	// Chapter 712. Each paragraph is a <p><span style="font-weight:400">.
	let blocks = chapter_blocks(RUNESMITH, "2508376");
	let text = paragraphs(&blocks);
	println!(
		"{} blocks: {:?} .. {:?}",
		blocks.len(),
		text[0],
		text.last()
	);
	assert!(text[0].starts_with("The following morning arrived soon after"));
	assert!(text.last().unwrap().contains("new life was on the way"));
	assert!(text.iter().all(|p| !p.contains('<') && !p.contains("&#")));
	assert!(blocks.len() > 30);
}

#[buny_test]
fn test_chapter_table() {
	// "Skills,Titles and other [Spoilers]": tables of skills with long
	// descriptions, which become one paragraph per row.
	let blocks = chapter_blocks(RUNESMITH, "183306");
	let text = paragraphs(&blocks);
	println!("{} blocks: {:?}", blocks.len(), &text[..3.min(text.len())]);
	assert!(
		text.iter()
			.any(|p| p.starts_with("Debugger | Passive Skill | Allows the user"))
	);
}

#[buny_test]
fn test_chapter_author_note() {
	// Starts with an author's note (avatar, name, body with a link).
	let blocks = chapter_blocks(
		"215867/goddess-of-ice-reborn-as-narutos-twin-sister-completed",
		"363577",
	);
	match &blocks[0] {
		ContentBlock::BlockQuote(text) => {
			println!("{text:?}");
			assert!(text.starts_with("Author's Note\n\nI made a stawpool"));
			assert!(!text.contains("Maerry"));
		}
		other => panic!("{other:?}"),
	}
	assert!(paragraphs(&blocks)[0].starts_with("Hinata, Anko, Naruto, Ino, and Haku"));
}

#[buny_test]
fn test_chapter_announcement_and_heading() {
	// An announcement box, then "The Incubus System Chapter 1. The Interview",
	// which repeats the series title and chapter name.
	let blocks = chapter_blocks("124643/the-incubus-system", "124646");
	match &blocks[0] {
		ContentBlock::BlockQuote(text) => {
			assert!(text.starts_with("Announcement\n\nAll character body description"))
		}
		other => panic!("{other:?}"),
	}
	let text = paragraphs(&blocks);
	assert!(text[0].starts_with("My name is Ethan."), "{:?}", text[0]);
}

#[buny_test]
fn test_listings() {
	let source = ScribbleHub::new();
	for id in [
		"trending",
		"popular",
		"latest",
		"new",
		"completed",
		"monthly",
	] {
		let listing = Listing {
			id: id.into(),
			..Default::default()
		};
		let first = source.get_novel_list(listing.clone(), 1).unwrap();
		let second = source.get_novel_list(listing, 2).unwrap();
		println!(
			"{id}: {} + {} entries, first {:?}",
			first.entries.len(),
			second.entries.len(),
			first.entries.first().map(|n| &n.title)
		);
		assert_eq!(first.entries.len(), 25, "{id}");
		assert!(first.has_next_page, "{id}");
		assert!(!second.entries.is_empty(), "{id}");
		assert_ne!(first.entries[0].key, second.entries[0].key, "{id}");
	}
	assert!(
		source
			.get_novel_list(
				Listing {
					id: "nope".into(),
					..Default::default()
				},
				1
			)
			.is_err()
	);
}

#[buny_test]
fn test_last_page() {
	// The trending ranking has a handful of pages; past the end is empty.
	let source = ScribbleHub::new();
	let past = ScribbleHub::listing("trending", 500).unwrap();
	assert!(past.entries.is_empty());
	assert!(!past.has_next_page);
	let search = source
		.get_search_novel_list(Some("runesmith".into()), 1, Vec::new())
		.unwrap();
	assert!(!search.has_next_page);
}

#[buny_test]
fn test_filters() {
	let all = search(Vec::new());
	assert_eq!(all.entries.len(), 25);

	let genre = |included: Vec<&str>, excluded: Vec<&str>| FilterValue::MultiSelect {
		id: "genres".into(),
		included: included.into_iter().map(String::from).collect(),
		excluded: excluded.into_iter().map(String::from).collect(),
	};
	// Genres "and": LitRPG (1180) and Isekai (37).
	let both = search(vec![
		genre(vec!["1180", "37"], vec![]),
		FilterValue::Select {
			id: "genres_mode".into(),
			value: "and".into(),
		},
	]);
	assert!(!both.entries.is_empty());
	for novel in &both.entries {
		let tags = novel.tags.as_ref().unwrap();
		assert!(
			tags.iter().any(|t| t == "LitRPG") && tags.iter().any(|t| t == "Isekai"),
			"{}: {tags:?}",
			novel.title
		);
	}
	// Excluding Adult (902).
	let no_adult = search(vec![genre(vec![], vec!["902"])]);
	assert!(!no_adult.entries.is_empty());
	assert!(
		no_adult
			.entries
			.iter()
			.all(|n| !n.tags.as_ref().unwrap().iter().any(|t| t == "Adult"))
	);

	// Completed, sorted by chapters, fewest first.
	let completed = search(vec![
		FilterValue::Select {
			id: "status".into(),
			value: "completed".into(),
		},
		FilterValue::Sort {
			id: "sort".into(),
			index: 8,
			ascending: true,
		},
	]);
	assert!(!completed.entries.is_empty());
	let top = ScribbleHub::new()
		.get_novel_update(completed.entries[0].clone(), true, false, 1)
		.unwrap();
	assert_eq!(top.status, NovelStatus::Completed, "{}", top.title);

	// Tag "Academy" (123), and the "Sexual Content" warning (50) excluded.
	let tagged = search(vec![
		FilterValue::MultiSelect {
			id: "tags".into(),
			included: vec!["123".into()],
			excluded: Vec::new(),
		},
		FilterValue::MultiSelect {
			id: "warnings".into(),
			included: Vec::new(),
			excluded: vec!["50".into()],
		},
	]);
	assert!(!tagged.entries.is_empty());
	let first = ScribbleHub::new()
		.get_novel_update(tagged.entries[0].clone(), true, false, 1)
		.unwrap();
	let tags = first.tags.unwrap();
	assert!(tags.iter().any(|t| t == "Academy"), "{tags:?}");
	assert_ne!(first.content_rating, ContentRating::NSFW);
}

#[buny_test]
fn test_home() {
	let home = ScribbleHub::new().get_home().unwrap();
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
	assert_eq!(home.components.len(), 5);
}

// ---- pure ----

#[buny_test]
fn test_deep_links() {
	let source = ScribbleHub::new();
	assert_eq!(
		source
			.handle_deep_link("https://www.scribblehub.com/series/117137/the-runesmith/".into())
			.unwrap(),
		Some(DeepLinkResult::Novel {
			key: RUNESMITH.into()
		})
	);
	assert_eq!(
		source
			.handle_deep_link(
				"https://www.scribblehub.com/read/117137-the-runesmith/chapter/2508376/".into()
			)
			.unwrap(),
		Some(DeepLinkResult::Chapter {
			novel_key: RUNESMITH.into(),
			key: "2508376".into(),
		})
	);
	assert_eq!(
		source
			.handle_deep_link("https://www.scribblehub.com/series-finder/".into())
			.unwrap(),
		None
	);
	assert_eq!(
		source
			.handle_deep_link("https://example.com/series/1/x/".into())
			.unwrap(),
		None
	);
}

#[buny_test]
fn test_page_helpers() {
	let text = "$('#pagination-profile-recent').pagination({ items: 1908, itemsOnPage: 1, \
		prevText: '«', displayedPages: '4', currentPage: '12', hrefTextPrefix: \"?pg=\" });";
	assert_eq!(pagination(text), Some((12, 1908)));
	assert_eq!(pagination("<div></div>"), None);

	let status = |html: &str| parse_status(&Html::parse(html).unwrap());
	assert_eq!(
		status(
			r#"<ul><li><span class="rnd_stats"><i class="fa fa-question status" aria-hidden="true"></i></span><span>Ongoing - Updated <span title="Last updated: 6 hours ago">6 hours ago</span></span></li></ul>"#
		),
		NovelStatus::Ongoing
	);
	assert_eq!(
		status(
			r#"<ul><li><span class="rnd_stats"><i class="fa fa-question status" aria-hidden="true"></i></span><span title="Last updated: Dec 29, 2018 06:21 PM">Completed</span></li></ul>"#
		),
		NovelStatus::Completed
	);
	assert_eq!(status("<p>nothing</p>"), NovelStatus::Unknown);

	assert_eq!(
		chapter_id("https://www.scribblehub.com/read/1-x/chapter/42/"),
		Some("42")
	);
	assert_eq!(
		novel_key("https://www.scribblehub.com/series/117137/the-runesmith/"),
		Some(RUNESMITH.into())
	);
	assert_eq!(
		chapter_url(RUNESMITH, "42"),
		"https://www.scribblehub.com/read/117137-the-runesmith/chapter/42/"
	);
	assert!(parse_chapter_date("Sep 27, 2026 09:00 AM").is_some());
	let now = current_date();
	let ago = parse_chapter_date("7 hours ago").unwrap();
	assert!((now - 7 * 3600 - ago).abs() < 60);
	assert!(parse_chapter_date("1 day ago").is_some());
}

#[buny_test]
fn test_content_markup() {
	let headings = content::Headings {
		name: "Chapter 1. The Interview",
		series: "The Incubus System",
	};
	// Real markup: an announcement opened inside a <p>, the heading with the
	// series title, emphasis next to spaces, an ornament and a scene break.
	let html = r#"<p dir="ltr"> <div class="wi_news"> <div class="wi_news_title"><i class="fa fa-exclamation-triangle" aria-hidden="true"></i> Announcement</div> <div class="wi_news_body"> All character images are in Glossary.</p> <p dir="ltr"></div> </div> <p><span><br /></span></p> <p dir="ltr"><span>The Incubus System Chapter 1. The Interview </span></p> <p dir="ltr">My name is Ethan.&nbsp; I&#8217;m <strong>poor </strong>and <em> tired</em>.</p><p>* * *</p><hr /><p>&lt;Skill: Charm&gt; activated.</p><p><img src="https://cdn.scribblehub.com/x.jpg"></p><p>***</p>"#;
	let blocks = content::html_blocks(html, &headings);
	assert_eq!(
		blocks,
		vec![
			ContentBlock::BlockQuote(
				"Announcement\n\nAll character images are in Glossary.".into()
			),
			ContentBlock::paragraph("My name is Ethan. I’m **poor** and *tired*.", None),
			ContentBlock::Divider,
			ContentBlock::paragraph("<Skill: Charm> activated.", None),
			ContentBlock::paragraph("[Illustration](https://cdn.scribblehub.com/x.jpg)", None),
		]
	);

	// An author's note: avatar and name dropped.
	let note = r#"<div class="wi_authornotes"> <div> <div class="p-avatar-wrap"><a class="avatar has-link" href="/profile/4591/maerry/"><img src="https://cdn.scribblehub.com/avatar/s/1/a.jpg"></a></div> <p><span class="an_username"><a href="/profile/4591/maerry/">Maerry</a></span></div> <div style="clear: both;"></div> <div class="wi_authornotes_body"> I made a poll.</p> <p><a href="https://example.com">example.com</a></p> </div></div> </p> <p>Story.</p>"#;
	assert_eq!(
		content::html_blocks(note, &headings),
		vec![
			ContentBlock::BlockQuote("Author's Note\n\nI made a poll.\n\nexample.com".into()),
			ContentBlock::paragraph("Story.", None),
		]
	);

	// Tables: short cells stay a table (spacer column dropped), long ones
	// become rows of text, and a one-cell frame becomes paragraphs.
	let table = r#"<p>Story.</p><table><tbody><tr><td><p>STR</p></td><td>10</td><td> </td></tr><tr><td>AGI</td><td><strong>12</strong></td><td></td></tr></tbody></table><table><tr><td>Debugger</td><td>Allows the user to find and resolve defects. Bonus to Intelligence +5.</td></tr></table><table><tr><td><p>[System]</p><p>You <em>leveled</em> up!</p></td></tr></table>"#;
	assert_eq!(
		content::html_blocks(table, &headings),
		vec![
			ContentBlock::paragraph("Story.", None),
			ContentBlock::Table(vec![
				vec!["STR".into(), "10".into()],
				vec!["AGI".into(), "12".into()],
			]),
			ContentBlock::paragraph(
				"Debugger | Allows the user to find and resolve defects. Bonus to Intelligence +5.",
				None
			),
			ContentBlock::paragraph("[System]", None),
			ContentBlock::paragraph("You *leveled* up!", None),
		]
	);

	assert_eq!(
		content::plain_text("<p>What happens?</p> <p>Discord: x</p>"),
		"What happens?\n\nDiscord: x"
	);
	// A collapsed description: the "more>>" link goes, the hidden rest stays.
	let collapsed = r##"<div style="padding-bottom:15px;">Gaining access to the System.<span class="dots">... </span><span class="morelink list" href="#" onclick="showtext(this); return false;">more>></span><span class="testhide" style="display:none"><p style="margin-top:-5px;"></p> That’s the only goal.<p style="margin-top:-5px;"></p> Emilia never imagined. <span class="morelink list" href="#" onclick="hidetext(this); return false;"> &lt;&lt;less</span></span></div>"##;
	assert_eq!(
		content::plain_text(collapsed),
		"Gaining access to the System.\n\nThat’s the only goal.\n\nEmilia never imagined."
	);

	// Plugs at either edge and the spoiler plugin's "[collapse]" toggle go;
	// the spoiler's title and content stay.
	let plugs = r##"<p><a href="https://www.scribblehub.com/out/?url=x">discord.gg/q5KWmtQARF</a></p> <p>Join my Discord!</p> <hr /> <p>Story.</p> <div class="sp-wrap sp-wrap-default"> <div class="sp-head" title="Expand"> Spoiler </div> <div class="sp-body folded"> <p>Hidden scene.</p> <div class="spdiv">[collapse]</div> </div> </div> <p>End.</p><p>Donate | Discord |</p><p>d iscord.gg/dY5UApw</p>"##;
	assert_eq!(
		paragraphs(&content::html_blocks(plugs, &headings)),
		vec!["Story.", "Spoiler", "Hidden scene.", "End."]
	);
}
