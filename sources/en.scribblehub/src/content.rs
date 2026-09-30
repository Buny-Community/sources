// Chapter bodies are the `#chp_raw` element: rich-editor HTML, mostly `<p>`
// (often wrapping `<span style=...>`), with inline `<strong>`/`<em>`/`<br>`,
// `<hr>` and images. Besides story text it can hold:
// - author's notes, `<div class="wi_authornotes">` with the author's avatar and
//   name, then `<div class="wi_authornotes_body">`, placed before or after the
//   chapter as the author chose. Sent as a banner (`BlockQuote`), like
//   en.royalroad does.
// - announcements, `<div class="wi_news">` with a `wi_news_title` and a
//   `wi_news_body`. Also a banner.
// - tables, used for status screens and skill lists. The app draws `Table`
//   rows at most 100pt high and shrinks text to fit, so only tables of short
//   cells become a `Table`; the rest become one paragraph per row.
// The editor's HTML is not always well formed: an announcement can open inside
// a `<p>` and close in the next one. Only `<div>` nesting is tracked, which
// that doesn't break.

use buny::{
	ContentBlock,
	alloc::{String, Vec},
	prelude::*,
};

use crate::unescape;

enum Line {
	Text { plain: String, markdown: String },
	Divider,
	Image(String),
	Banner(String),
	Table(Vec<Vec<Cell>>),
}

struct Cell {
	// The cell's lines joined with spaces.
	plain: String,
	markdown: Vec<String>,
}

#[derive(Default)]
struct Run {
	text: String,
	bold: bool,
	italic: bool,
}

#[derive(Default)]
struct Builder {
	lines: Vec<Line>,
	runs: Vec<Run>,
	bold: u32,
	italic: u32,
}

// Zero-width and direction marks, never visible.
fn is_invisible(c: char) -> bool {
	matches!(c, '\u{200b}'..='\u{200f}' | '\u{2060}'..='\u{2064}' | '\u{feff}' | '\u{ad}')
}

impl Builder {
	// `text` is already decoded (no entities).
	fn text(&mut self, text: &str) {
		if text.is_empty() {
			return;
		}
		// Newlines inside a paragraph are plain whitespace; &nbsp; is only used
		// as spacing, so it becomes a normal space.
		let text: String = text
			.chars()
			.filter(|&c| !is_invisible(c))
			.map(|c| {
				if matches!(c, '\r' | '\n' | '\t' | '\u{a0}') {
					' '
				} else {
					c
				}
			})
			.collect();
		let (bold, italic) = (self.bold > 0, self.italic > 0);
		match self.runs.last_mut() {
			Some(run) if run.bold == bold && run.italic == italic => run.text.push_str(&text),
			_ => self.runs.push(Run { text, bold, italic }),
		}
	}

	fn break_line(&mut self) {
		let runs = core::mem::take(&mut self.runs);
		let plain: String = runs.iter().map(|r| r.text.as_str()).collect();
		let plain = collapse_spaces(plain.trim());
		if plain.is_empty() {
			return;
		}
		let mut markdown = String::with_capacity(plain.len() + 8);
		for run in &runs {
			let marker = match (run.bold, run.italic) {
				(true, true) => "***",
				(true, false) => "**",
				(false, true) => "*",
				(false, false) => "",
			};
			let core = run.text.trim();
			if marker.is_empty() || core.is_empty() {
				markdown.push_str(&run.text);
				continue;
			}
			// Markdown emphasis can't open or close next to whitespace
			// ("**Discord: **" renders the asterisks), so keep it outside.
			let lead = &run.text[..run.text.len() - run.text.trim_start().len()];
			let trail = &run.text[run.text.trim_end().len()..];
			markdown.push_str(lead);
			markdown.push_str(marker);
			markdown.push_str(core);
			markdown.push_str(marker);
			markdown.push_str(trail);
		}
		self.lines.push(Line::Text {
			plain,
			markdown: collapse_spaces(markdown.trim()),
		});
	}

	fn push(&mut self, line: Line) {
		self.break_line();
		self.lines.push(line);
	}

	fn set_bold(&mut self, on: bool) {
		self.bold = if on {
			self.bold + 1
		} else {
			self.bold.saturating_sub(1)
		};
	}

	fn set_italic(&mut self, on: bool) {
		self.italic = if on {
			self.italic + 1
		} else {
			self.italic.saturating_sub(1)
		};
	}

	fn finish(mut self) -> Vec<Line> {
		self.break_line();
		self.lines
	}
}

// Editors pad with runs of spaces and &nbsp;.
fn collapse_spaces(s: &str) -> String {
	let mut out = String::with_capacity(s.len());
	let mut space = false;
	for c in s.chars() {
		if c == ' ' {
			if !space {
				out.push(c);
			}
			space = true;
		} else {
			out.push(c);
			space = false;
		}
	}
	out
}

// ---- HTML ----

fn tag_name(tag: &str) -> String {
	let name_part = tag.trim_start_matches('/');
	let end = name_part
		.find(|c: char| !c.is_ascii_alphanumeric())
		.unwrap_or(name_part.len());
	name_part[..end].to_ascii_lowercase()
}

pub(crate) fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
	let mut rest = tag;
	while let Some(pos) = rest.find(name) {
		let before_ok = rest[..pos].ends_with(|c: char| c.is_whitespace());
		let after = rest[pos + name.len()..].trim_start();
		rest = &rest[pos + name.len()..];
		if !before_ok {
			continue;
		}
		let Some(value) = after.strip_prefix('=') else {
			continue;
		};
		let value = value.trim_start();
		let quote = value.chars().next()?;
		return if quote == '"' || quote == '\'' {
			value[1..].split(quote).next()
		} else {
			value.split(|c: char| c.is_whitespace() || c == '/').next()
		};
	}
	None
}

fn has_class(tag: &str, class: &str) -> bool {
	attribute(tag, "class").is_some_and(|c| c.split_whitespace().any(|c| c == class))
}

// Elements skipped with everything inside them.
fn is_skipped(tag: &str, name: &str) -> bool {
	// A long description is collapsed: "... " and a "more>>" link, then the
	// rest in a `display:none` `testhide` span that the link reveals.
	if has_class(tag, "testhide") {
		return false;
	}
	matches!(name, "style" | "script" | "noscript" | "button")
		|| has_class(tag, "dots")
		|| has_class(tag, "morelink")
		// The spoiler plugin's "[collapse]" toggle, at the end of each spoiler.
		|| has_class(tag, "spdiv")
		|| attribute(tag, "aria-hidden") == Some("true")
		|| attribute(tag, "style").is_some_and(|s| {
			let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
			s.contains("display:none")
		})
		// The author's avatar and name above an author's note.
		|| has_class(tag, "p-avatar-wrap")
		|| has_class(tag, "an_username")
}

#[derive(Clone, Copy, PartialEq)]
enum BannerKind {
	AuthorNote,
	Announcement,
}

struct Banner {
	kind: BannerKind,
	// `<div>` depth of the banner's own element.
	depth: u32,
	builder: Builder,
}

#[derive(Default)]
struct Table {
	// Tables inside this one, whose cells are read as text of the outer cell.
	nested: u32,
	rows: Vec<Vec<Cell>>,
	row: Vec<Cell>,
	cell: Option<Builder>,
}

impl Table {
	fn end_cell(&mut self) {
		if let Some(cell) = self.cell.take() {
			let markdown: Vec<String> = cell
				.finish()
				.into_iter()
				.filter_map(|l| match l {
					Line::Text { markdown, .. } => Some(markdown),
					_ => None,
				})
				.collect();
			let plain = markdown
				.iter()
				.map(|m| strip_markers(m))
				.collect::<Vec<_>>()
				.join(" ");
			self.row.push(Cell { plain, markdown });
		}
	}

	fn end_row(&mut self) {
		self.end_cell();
		let row = core::mem::take(&mut self.row);
		if !row.is_empty() {
			self.rows.push(row);
		}
	}
}

fn strip_markers(markdown: &str) -> String {
	markdown
		.replace("***", "")
		.replace("**", "")
		.replace('*', "")
}

#[derive(Default)]
struct Parser {
	main: Builder,
	banner: Option<Banner>,
	table: Option<Table>,
	div_depth: u32,
}

impl Parser {
	fn builder(&mut self) -> &mut Builder {
		if let Some(table) = self.table.as_mut() {
			// Text between cells is only markup whitespace.
			return table.cell.get_or_insert_with(Builder::default);
		}
		match self.banner.as_mut() {
			Some(banner) => &mut banner.builder,
			None => &mut self.main,
		}
	}

	// Where a finished table or image goes.
	fn outer(&mut self) -> &mut Builder {
		match self.banner.as_mut() {
			Some(banner) => &mut banner.builder,
			None => &mut self.main,
		}
	}

	fn text(&mut self, text: &str) {
		if self
			.table
			.as_ref()
			.is_some_and(|t| t.cell.is_none() && text.trim().is_empty())
		{
			return;
		}
		self.builder().text(text);
	}

	fn end_banner(&mut self) {
		let Some(banner) = self.banner.take() else {
			return;
		};
		let lines: Vec<String> = banner
			.builder
			.finish()
			.into_iter()
			.filter_map(|l| match l {
				Line::Text { plain, .. } => Some(plain),
				Line::Image(src) => Some(src),
				_ => None,
			})
			.collect();
		if lines.is_empty() {
			return;
		}
		let mut text = String::new();
		// An announcement's title ("Announcement") is its first line already.
		if banner.kind == BannerKind::AuthorNote {
			text.push_str("Author's Note\n\n");
		}
		text.push_str(&lines.join("\n\n"));
		self.main.push(Line::Banner(text));
	}

	fn tag(&mut self, tag: &str) {
		let closing = tag.starts_with('/');
		let name = tag_name(tag);

		if name == "table" {
			match (self.table.as_mut(), closing) {
				(None, false) => {
					self.builder().break_line();
					self.table = Some(Table::default());
				}
				(Some(t), false) => t.nested += 1,
				(Some(t), true) if t.nested > 0 => t.nested -= 1,
				(Some(_), true) => {
					let mut table = self.table.take().unwrap_or_default();
					table.end_row();
					if !table.rows.is_empty() {
						self.outer().push(Line::Table(table.rows));
					}
				}
				(None, true) => {}
			}
			return;
		}
		if let Some(table) = self.table.as_mut()
			&& table.nested == 0
		{
			match name.as_str() {
				"tr" => {
					table.end_row();
					return;
				}
				"td" | "th" => {
					table.end_cell();
					if !closing {
						table.cell = Some(Builder::default());
					}
					return;
				}
				_ => {}
			}
		}

		if name == "div" {
			if closing {
				if self
					.banner
					.as_ref()
					.is_some_and(|b| b.depth == self.div_depth)
				{
					self.end_banner();
				}
				self.div_depth = self.div_depth.saturating_sub(1);
			} else {
				self.div_depth += 1;
				let kind = if has_class(tag, "wi_authornotes") {
					Some(BannerKind::AuthorNote)
				} else if has_class(tag, "wi_news") {
					Some(BannerKind::Announcement)
				} else {
					None
				};
				if let Some(kind) = kind
					&& self.banner.is_none()
					&& self.table.is_none()
				{
					self.main.break_line();
					self.banner = Some(Banner {
						kind,
						depth: self.div_depth,
						builder: Builder::default(),
					});
					return;
				}
			}
			self.builder().break_line();
			return;
		}

		let builder = self.builder();
		match name.as_str() {
			"strong" | "b" => builder.set_bold(!closing),
			"em" | "i" => builder.set_italic(!closing),
			"hr" => builder.push(Line::Divider),
			"img" if !closing => {
				if let Some(src) = attribute(tag, "src") {
					let src = unescape(src);
					let src = if src.starts_with("//") {
						format!("https:{src}")
					} else if src.starts_with('/') {
						format!("{}{src}", crate::BASE_URL)
					} else {
						src
					};
					if src.starts_with("http") {
						builder.push(Line::Image(src));
					}
				}
			}
			"p" | "br" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "ul" | "ol"
			| "blockquote" | "figure" | "figcaption" | "section" | "article" | "pre" => builder.break_line(),
			// span, a, u, s, sup, font... carry nothing the reader can show.
			_ => {}
		}
	}

	fn finish(mut self) -> Vec<Line> {
		if let Some(mut table) = self.table.take() {
			table.end_row();
			if !table.rows.is_empty() {
				self.outer().push(Line::Table(table.rows));
			}
		}
		self.end_banner();
		self.main.finish()
	}
}

fn parse_html(html: &str) -> Vec<Line> {
	let mut parser = Parser::default();
	let mut rest = html;
	// While inside a skipped element: its tag name and how many same-named
	// elements are open inside it.
	let mut skipping: Option<(String, u32)> = None;
	while let Some(start) = rest.find('<') {
		if skipping.is_none() {
			parser.text(&unescape(&rest[..start]));
		}
		let after = &rest[start + 1..];
		let is_tag = after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
		let Some(end) = after.find('>').filter(|_| is_tag) else {
			// A bare '<' in text ("<Skill Name>" typed by hand); keep it.
			if skipping.is_none() {
				parser.text("<");
			}
			rest = after;
			continue;
		};
		let tag = &after[..end];
		rest = &after[end + 1..];
		if tag.starts_with('!') {
			continue;
		}
		let name = tag_name(tag);
		let closing = tag.starts_with('/');
		let self_closing = tag.ends_with('/') || matches!(name.as_str(), "br" | "hr" | "img");

		if let Some((skip_name, depth)) = skipping.as_mut() {
			if name == *skip_name {
				if closing {
					if *depth == 0 {
						skipping = None;
					} else {
						*depth -= 1;
					}
				} else if !self_closing {
					*depth += 1;
				}
			}
			continue;
		}
		if !closing && !self_closing && is_skipped(tag, &name) {
			skipping = Some((name, 0));
			continue;
		}
		parser.tag(tag);
	}
	if skipping.is_none() {
		parser.text(&unescape(rest));
	}
	parser.finish()
}

// ---- edges ----

// A line made only of break characters: "* * *", "***", "──────", "# # #", "____".
fn is_scene_break(plain: &str) -> bool {
	let mut count = 0;
	for c in plain.chars().filter(|c| !c.is_whitespace()) {
		if !matches!(
			c,
			'*' | '~'
				| '=' | '#' | '─'
				| '━' | '-' | '—'
				| '_' | '◇' | '◆'
				| '•' | '·' | '+'
				| '§' | '❖' | '✦'
				| '☆' | '★'
		) {
			return false;
		}
		count += 1;
	}
	count >= 3
}

// ASCII letters and digits only, lowercased, so "Chapter 1 – Regression"
// matches "chapter1regression" and curly quotes don't matter.
pub(crate) fn normalize(s: &str) -> String {
	s.chars()
		.filter(|c| c.is_ascii_alphanumeric())
		.map(|c| c.to_ascii_lowercase())
		.collect()
}

// A line with no letters or digits ("᠃ ⚘᠂ ⚘ ᠃"): decoration. A line of speech
// punctuation ("……", "…!", "“...”") is a wordless reply, which is story, so
// it stays.
fn is_ornament(plain: &str) -> bool {
	!plain.chars().any(|c| {
		c.is_alphanumeric()
			|| matches!(
				c,
				'.' | '!' | '?' | '…' | '"' | '“' | '”' | '‘' | '’' | '\''
			)
	})
}

/// What a chapter's opening heading can repeat: the chapter's name as the
/// list shows it and the series title.
pub struct Headings<'a> {
	pub name: &'a str,
	pub series: &'a str,
}

// The chapter heading repeated at the top: the list's name ("Chapter 3 –
// Geography"), the name without its label, or the series title before it
// ("The Incubus System Chapter 1. The Interview").
fn is_heading(plain: &str, headings: &Headings) -> bool {
	let line = normalize(plain);
	if line.is_empty() || plain.chars().count() > 200 {
		return false;
	}
	let name = normalize(headings.name);
	let series = normalize(headings.series);
	let bare = crate::split_label(headings.name)
		.map(|(_, rest)| normalize(rest))
		.unwrap_or_default();
	if [&name, &bare].iter().any(|h| !h.is_empty() && **h == line) {
		return true;
	}
	if !series.is_empty()
		&& let Some(rest) = line.strip_prefix(&series)
	{
		return rest.is_empty() || rest == name || (!bare.is_empty() && rest == bare);
	}
	// "Chapter 3" alone, or "Chapter 3: <anything short>" when the list names it
	// differently. Only a short line, so a sentence starting "Chapter" stays.
	crate::split_label(plain).is_some_and(|(_, rest)| rest.chars().count() <= 80)
		&& normalize(plain).starts_with("chapter")
}

// A plug for the author's Discord, Patreon or Ko-fi: a link ("discord.gg/x",
// and "d iscord.gg/x" where the site's filter was dodged) or just the link's
// text ("My Discord", "Donate | Discord |"). The site blanks Patreon links in
// the text (".com/name"), so those are left alone.
fn is_plug(plain: &str) -> bool {
	let lower = plain.trim().to_lowercase();
	let compact: String = lower.chars().filter(|c| c.is_alphanumeric()).collect();
	lower.contains("discord.gg/")
		|| compact.contains("discordgg")
		|| lower.contains("discord.com/invite")
		|| lower.contains("ko-fi.com/")
		|| lower.contains("patreon.com/")
		|| matches!(
			compact.as_str(),
			"discord"
				| "mydiscord"
				| "joindiscord"
				| "joinmydiscord"
				| "patreon" | "mypatreon"
				| "kofi" | "donate"
				| "donatediscord"
				| "discorddonate"
				| "patreondiscord"
				| "discordpatreon"
		)
}

// Lines that close a chapter: end markers and plugs.
fn is_footer(plain: &str) -> bool {
	let lower = plain.trim().to_lowercase();
	let compact: String = lower.chars().filter(|c| c.is_alphanumeric()).collect();
	matches!(
		compact.as_str(),
		"endofchapter"
			| "endofthischapter"
			| "endofthechapter"
			| "tobecontinued"
			| "previoustocnext"
			| "prevtocnext"
			| "previousnext"
			| "prevnext"
	) || is_plug(plain)
}

fn text_line(line: &Line) -> Option<&str> {
	match line {
		Line::Text { plain, .. } => Some(plain),
		_ => None,
	}
}

fn table_blocks(rows: Vec<Vec<Cell>>, blocks: &mut Vec<ContentBlock>) {
	// Drop columns that are empty in every row (editors add a spacer column).
	let width = rows.iter().map(Vec::len).max().unwrap_or(0);
	let keep: Vec<bool> = (0..width)
		.map(|col| {
			rows.iter()
				.any(|r| r.get(col).is_some_and(|c| !c.plain.is_empty()))
		})
		.collect();
	let rows: Vec<Vec<Cell>> = rows
		.into_iter()
		.map(|r| {
			r.into_iter()
				.enumerate()
				.filter(|(i, _)| keep.get(*i).copied().unwrap_or(false))
				.map(|(_, c)| c)
				.collect::<Vec<_>>()
		})
		.filter(|r: &Vec<Cell>| r.iter().any(|c| !c.plain.is_empty()))
		.collect();
	let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
	if columns == 0 {
		return;
	}
	// A one-column table is a frame around text (a system message): its lines
	// read as paragraphs.
	if columns == 1 {
		for cell in rows.into_iter().flatten() {
			for md in cell.markdown {
				blocks.push(ContentBlock::paragraph(md, None));
			}
		}
		return;
	}
	let short = rows.iter().flatten().all(|c| c.plain.chars().count() <= 40);
	if short {
		blocks.push(ContentBlock::Table(
			rows.into_iter()
				.map(|r| {
					let mut cells: Vec<String> = r.into_iter().map(|c| c.plain).collect();
					cells.resize(columns, String::new());
					cells
				})
				.collect(),
		));
		return;
	}
	for row in rows {
		let cells: Vec<String> = row
			.into_iter()
			.filter(|c| !c.plain.is_empty())
			.map(|c| c.markdown.join(" "))
			.collect();
		blocks.push(ContentBlock::paragraph(cells.join(" | "), None));
	}
}

fn to_blocks(lines: Vec<Line>, headings: &Headings) -> Vec<ContentBlock> {
	let is_junk_edge = |line: &Line| match line {
		Line::Divider => true,
		Line::Text { plain, .. } => is_scene_break(plain) || is_ornament(plain),
		_ => false,
	};

	// The repeated heading and decoration sit in the first few lines; stop at
	// the first line of story. Banners (announcements, author's notes) before
	// the heading are kept.
	let mut banners: Vec<usize> = Vec::new();
	let mut start = 0;
	let mut seen_heading = false;
	while let Some(line) = lines.get(start) {
		if matches!(line, Line::Banner(_)) {
			banners.push(start);
			start += 1;
			continue;
		}
		let skip = is_junk_edge(line)
			|| text_line(line).is_some_and(|p| p.chars().count() <= 120 && is_plug(p))
			|| (!seen_heading && text_line(line).is_some_and(|p| is_heading(p, headings)) && {
				seen_heading = true;
				true
			});
		if !skip {
			break;
		}
		start += 1;
	}
	// Leading banners keep their place before the story.
	let lead: Vec<usize> = banners.into_iter().filter(|&i| i < start).collect();

	let mut end = lines.len();
	while end > start {
		let line = &lines[end - 1];
		if !(is_junk_edge(line) || text_line(line).is_some_and(is_footer)) {
			break;
		}
		end -= 1;
	}

	let mut blocks: Vec<ContentBlock> = Vec::with_capacity(end - start + lead.len());
	let keep = |i: usize| (i >= start && i < end) || lead.contains(&i);
	for line in lines
		.into_iter()
		.enumerate()
		.filter(|(i, _)| keep(*i))
		.map(|(_, l)| l)
	{
		let is_break = match &line {
			Line::Divider => true,
			Line::Text { plain, .. } => is_scene_break(plain),
			_ => false,
		};
		if is_break {
			// "<hr>" right after "***" is one break, not two.
			if !matches!(blocks.last(), Some(ContentBlock::Divider)) {
				blocks.push(ContentBlock::Divider);
			}
			continue;
		}
		match line {
			Line::Text { markdown, .. } => blocks.push(ContentBlock::paragraph(markdown, None)),
			// ContentBlock has no image variant, so an illustration becomes a
			// tappable link rather than being dropped.
			Line::Image(src) => blocks.push(ContentBlock::paragraph(
				format!("[Illustration]({src})"),
				None,
			)),
			Line::Banner(text) => blocks.push(ContentBlock::BlockQuote(text)),
			Line::Table(rows) => table_blocks(rows, &mut blocks),
			Line::Divider => {}
		}
	}
	while matches!(blocks.last(), Some(ContentBlock::Divider)) {
		blocks.pop();
	}
	blocks
}

/// Converts the inner HTML of `#chp_raw`.
pub fn html_blocks(html: &str, headings: &Headings) -> Vec<ContentBlock> {
	to_blocks(parse_html(html), headings)
}

/// Plain text with paragraphs separated by blank lines (for descriptions).
pub fn plain_text(html: &str) -> String {
	parse_html(html)
		.into_iter()
		.filter_map(|line| match line {
			Line::Text { plain, .. } => Some(plain),
			Line::Banner(text) => Some(text),
			_ => None,
		})
		.collect::<Vec<_>>()
		.join("\n\n")
}
