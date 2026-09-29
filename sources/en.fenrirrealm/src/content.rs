// Chapter bodies come in three formats (`content_format`):
// - "html" (nearly every chapter): rich-editor HTML, a flat run of `<p>` with
//   inline `<strong>`/`<em>`/`<br>`, and "system window" boxes as
//   `<div data-variation=...><div class="content">...</div></div>`. Every body
//   also carries two watermarks: a `<style>` rule for a visually hidden class
//   plus `<div class="<that class>" aria-hidden="true">` junk strings between
//   paragraphs, and runs of zero-width characters after the first word of
//   paragraphs (a per-reader fingerprint). Both are dropped.
// - "json" (older chapters): a TipTap document stored as a string.
// - "text": plain lines. Paid chapters' teasers come in this format.

use buny::{
	ContentBlock,
	alloc::{String, Vec, string::ToString},
	prelude::*,
};
use serde_json::Value;

use crate::unescape;

enum Line {
	Text { plain: String, markdown: String },
	Divider,
	Image(String),
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

// Zero-width and direction marks: the fingerprint, and never visible anyway.
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
				if matches!(c, '\r' | '\n' | '\u{a0}') {
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
		let plain = plain.trim();
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
			plain: plain.to_string(),
			markdown: markdown.trim().to_string(),
		});
	}

	fn divider(&mut self) {
		self.break_line();
		self.lines.push(Line::Divider);
	}

	fn image(&mut self, src: String) {
		self.break_line();
		self.lines.push(Line::Image(src));
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
}

// ---- HTML ----

fn tag_name(tag: &str) -> String {
	let name_part = tag.trim_start_matches('/');
	let end = name_part
		.find(|c: char| !c.is_ascii_alphanumeric())
		.unwrap_or(name_part.len());
	name_part[..end].to_ascii_lowercase()
}

fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
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

// Class names that a `<style>` block hides (`clip`, 1px boxes, display:none).
fn hidden_classes(html: &str) -> Vec<String> {
	let mut classes = Vec::new();
	let mut rest = html;
	while let Some(start) = rest.find("<style") {
		let body_start = rest[start..]
			.find('>')
			.map_or(rest.len(), |e| start + e + 1);
		let end = rest[body_start..]
			.find("</style")
			.map_or(rest.len(), |e| body_start + e);
		let mut css = &rest[body_start..end];
		while let Some(open) = css.find('{') {
			let selector = css[..open].trim();
			let close = css[open..].find('}').map_or(css.len(), |c| open + c);
			let rule = &css[open + 1..close];
			let hides = rule.contains("clip")
				|| rule.contains("display:none")
				|| rule.contains("display: none")
				|| rule.contains("visibility:hidden");
			if hides {
				for sel in selector.split(',') {
					if let Some(class) = sel.trim().strip_prefix('.')
						&& class
							.bytes()
							.all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
					{
						classes.push(class.to_string());
					}
				}
			}
			css = &css[(close + 1).min(css.len())..];
		}
		rest = &rest[end..];
	}
	classes
}

fn is_hidden(tag: &str, hidden: &[String]) -> bool {
	if attribute(tag, "aria-hidden") == Some("true") || attribute(tag, "hidden").is_some() {
		return true;
	}
	attribute(tag, "class")
		.is_some_and(|c| c.split_whitespace().any(|c| hidden.iter().any(|h| h == c)))
}

fn html_tag(builder: &mut Builder, tag: &str) {
	let closing = tag.starts_with('/');
	match tag_name(tag).as_str() {
		"strong" | "b" => builder.set_bold(!closing),
		"em" | "i" => builder.set_italic(!closing),
		"hr" => builder.divider(),
		"img" if !closing => {
			if let Some(src) = attribute(tag, "src") {
				let src = unescape(src);
				let src = if src.starts_with('/') {
					format!("{}{src}", crate::BASE_URL)
				} else {
					src
				};
				builder.image(src);
			}
		}
		"p" | "br" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "ul" | "ol"
		| "blockquote" | "figure" | "figcaption" | "table" | "tr" => builder.break_line(),
		// td/th, span, a, u, s, sup... carry no structure the reader can show.
		"td" | "th" => builder.text(" "),
		_ => {}
	}
}

fn parse_html(html: &str) -> Vec<Line> {
	let hidden = hidden_classes(html);
	let mut builder = Builder::default();
	let mut rest = html;
	// While inside a hidden element or a <style>/<script>: its tag name and
	// how many same-named elements are open inside it.
	let mut skipping: Option<(String, u32)> = None;
	while let Some(start) = rest.find('<') {
		if skipping.is_none() {
			builder.text(&unescape(&rest[..start]));
		}
		let after = &rest[start + 1..];
		let is_tag = after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
		let Some(end) = after.find('>').filter(|_| is_tag) else {
			// A bare '<' in text; keep it.
			if skipping.is_none() {
				builder.text("<");
			}
			rest = after;
			continue;
		};
		let tag = &after[..end];
		rest = &after[end + 1..];
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
		if !closing
			&& !self_closing
			&& (matches!(name.as_str(), "style" | "script") || is_hidden(tag, &hidden))
		{
			skipping = Some((name, 0));
			continue;
		}
		html_tag(&mut builder, tag);
	}
	if skipping.is_none() {
		builder.text(&unescape(rest));
	}
	builder.break_line();
	builder.lines
}

// ---- TipTap JSON ----

fn doc_node(builder: &mut Builder, node: &Value) {
	match node["type"].as_str().unwrap_or("") {
		"text" => {
			let marks = node["marks"].as_array();
			let has = |kind: &str| marks.is_some_and(|m| m.iter().any(|m| m["type"] == kind));
			let (bold, italic) = (has("bold"), has("italic"));
			if bold {
				builder.set_bold(true);
			}
			if italic {
				builder.set_italic(true);
			}
			builder.text(node["text"].as_str().unwrap_or(""));
			if bold {
				builder.set_bold(false);
			}
			if italic {
				builder.set_italic(false);
			}
		}
		"hardBreak" => builder.break_line(),
		"horizontalRule" => builder.divider(),
		"image" => {
			if let Some(src) = node["attrs"]["src"].as_str() {
				builder.image(src.to_string());
			}
		}
		// paragraph, heading, blockquote, lists, the "systemWindow" box and the
		// doc itself: blocks of other nodes.
		_ => {
			for child in node["content"].as_array().into_iter().flatten() {
				doc_node(builder, child);
			}
			builder.break_line();
		}
	}
}

fn parse_doc(doc: &Value) -> Vec<Line> {
	let mut builder = Builder::default();
	doc_node(&mut builder, doc);
	builder.break_line();
	builder.lines
}

// ---- plain text ----

fn parse_text(text: &str) -> Vec<Line> {
	let mut builder = Builder::default();
	for line in text.lines() {
		builder.text(line);
		builder.break_line();
	}
	builder.lines
}

// ---- edges ----

// A line made only of break characters: "* * *", "***", "──────", "# # #", "____".
fn is_scene_break(plain: &str) -> bool {
	let mut count = 0;
	for c in plain.chars().filter(|c| !c.is_whitespace()) {
		if !matches!(
			c,
			'*' | '~' | '=' | '#' | '─' | '━' | '-' | '—' | '_' | '◇' | '◆' | '•' | '·'
		) {
			return false;
		}
		count += 1;
	}
	count >= 3
}

// Lowercase letters and digits only, so "Chapter 1 — Regression" matches
// "chapter1regression".
fn normalize(s: &str) -> String {
	s.chars()
		.filter(|c| c.is_alphanumeric())
		.flat_map(char::to_lowercase)
		.collect()
}

// Credit lines that open a chapter: "「Translator: ...」", "Editor : ...",
// "TL: ...". Only matched at the start of a chapter.
fn is_credit(plain: &str) -> bool {
	let lower = plain
		.trim_start_matches(|c: char| !c.is_alphanumeric())
		.to_lowercase();
	[
		"translator",
		"translated by",
		"editor",
		"edited by",
		"proofread",
		"tl:",
		"tl :",
		"ed:",
		"pr:",
		"quality checker",
		"murim term consultant",
	]
	.iter()
	.any(|p| lower.starts_with(p))
}

// A line with no letters or digits ("᠃ ⚘᠂ ⚘ ᠃", "༺ 𓆩 𓆪 ༻"): decoration. A
// line of speech punctuation ("……", "…!", "“...”") is a wordless reply, which
// is story, so it stays.
fn is_ornament(plain: &str) -> bool {
	!plain.chars().any(|c| {
		c.is_alphanumeric()
			|| matches!(
				c,
				'.' | '!' | '?' | '…' | '"' | '“' | '”' | '‘' | '’' | '\''
			)
	})
}

// The chapter heading repeated at the top: "Chapter 93: Name (13)",
// "༺ 𓆩 Chapter 1 — Regression 𓆪 ༻", "43. Abnormal Weather (3)", or the
// title alone. `headings` are the chapter's name and title.
fn is_heading(plain: &str, headings: &[&str; 2]) -> bool {
	// ASCII only: the ornaments include hieroglyphs (𓆩), which count as letters.
	let trimmed = plain.trim_matches(|c: char| !c.is_ascii_alphanumeric());
	if trimmed.is_empty() || trimmed.chars().count() > 200 {
		return false;
	}
	if crate::split_label(trimmed).is_some_and(|(_, rest)| rest.chars().count() <= 150) {
		return true;
	}
	let line = normalize(trimmed);
	let [name, title] = headings.map(normalize);
	let title_rest = crate::split_label(headings[1]).map(|(_, rest)| normalize(rest));
	[Some(&name), Some(&title), title_rest.as_ref()]
		.into_iter()
		.flatten()
		.any(|h| !h.is_empty() && *h == line)
		|| (!name.is_empty() && !title.is_empty() && line == format!("{name}{title}"))
}

// "Volume 1: The Grand Scholar of Dongsa", "Vol. 2", above the chapter heading
// in the first chapter of a volume.
fn is_volume_heading(plain: &str) -> bool {
	let lower = plain.trim().to_lowercase();
	let Some(rest) = ["volume", "vol."]
		.iter()
		.find_map(|p| lower.strip_prefix(p))
	else {
		return false;
	};
	rest.trim_start().starts_with(|c: char| c.is_ascii_digit()) && plain.chars().count() <= 120
}

// Lines that close a chapter: navigation, end markers and the translators'
// schedule and review plugs.
fn is_footer(plain: &str) -> bool {
	let lower = plain.trim().to_lowercase();
	let compact: String = lower.chars().filter(|c| c.is_alphanumeric()).collect();
	matches!(
		compact.as_str(),
		"endofchapter"
			| "endofthischapter"
			| "endofthechapter"
			| "theend"
			| "end" | "tobecontinued"
			| "previtocinext"
			| "previoustocnext"
			| "prevtocnext"
			| "novelupdate"
			| "novelupdates"
			| "reviewnovelupdate"
			| "reviewnovelupdates"
	) || lower == "єη∂ σƒ ¢нαρƭєя"
		|| lower.starts_with("review @")
		|| lower.starts_with("schedule:")
		|| lower.starts_with("friendly reminder: if you encounter ads")
		|| lower.starts_with("check out my other novel")
		|| lower.contains("discord.gg/")
		|| lower.contains("ko-fi.com/")
		|| lower.contains("patreon.com/")
}

fn to_blocks(lines: Vec<Line>, headings: &[&str; 2]) -> Vec<ContentBlock> {
	let is_break = |line: &Line| match line {
		Line::Divider => true,
		Line::Text { plain, .. } => is_scene_break(plain),
		Line::Image(_) => false,
	};

	// The heading, credits and decoration sit in the first few lines; stop at
	// the first line of story.
	let mut start = 0;
	let mut seen_heading = false;
	while let Some(line) = lines.get(start) {
		let skip = match line {
			Line::Divider => true,
			Line::Image(_) => false,
			Line::Text { plain, .. } => {
				is_scene_break(plain)
					|| is_ornament(plain)
					|| is_credit(plain)
					|| is_volume_heading(plain)
					// Only one heading, so a story line that repeats the title
					// after it stays.
					|| (!seen_heading && is_heading(plain, headings) && {
						seen_heading = true;
						true
					})
			}
		};
		if !skip {
			break;
		}
		start += 1;
	}

	let mut end = lines.len();
	while end > start {
		let junk = match &lines[end - 1] {
			Line::Divider => true,
			Line::Text { plain, .. } => {
				is_scene_break(plain) || is_ornament(plain) || is_footer(plain)
			}
			Line::Image(_) => false,
		};
		if !junk {
			break;
		}
		end -= 1;
	}

	let mut blocks: Vec<ContentBlock> = Vec::with_capacity(end - start);
	for line in &lines[start..end] {
		if is_break(line) {
			// "<hr>" right after "***" is one break, not two.
			if !matches!(blocks.last(), Some(ContentBlock::Divider)) {
				blocks.push(ContentBlock::Divider);
			}
			continue;
		}
		match line {
			Line::Text { markdown, .. } => {
				blocks.push(ContentBlock::paragraph(markdown.clone(), None))
			}
			// ContentBlock has no image variant, so an illustration becomes a
			// tappable link rather than being dropped.
			Line::Image(src) => blocks.push(ContentBlock::paragraph(
				format!("[Illustration]({src})"),
				None,
			)),
			Line::Divider => {}
		}
	}
	blocks
}

/// Converts an HTML chapter body. `headings` are the chapter's name and title.
pub fn html_blocks(html: &str, headings: &[&str; 2]) -> Vec<ContentBlock> {
	to_blocks(parse_html(html), headings)
}

/// Converts a TipTap document chapter body.
pub fn doc_blocks(doc: &Value, headings: &[&str; 2]) -> Vec<ContentBlock> {
	to_blocks(parse_doc(doc), headings)
}

/// Converts a plain-text chapter body.
pub fn text_blocks(text: &str, headings: &[&str; 2]) -> Vec<ContentBlock> {
	to_blocks(parse_text(text), headings)
}

/// Plain text with paragraphs separated by blank lines (for descriptions).
pub fn plain_text(html: &str) -> String {
	parse_html(html)
		.into_iter()
		.filter_map(|line| match line {
			Line::Text { plain, .. } => Some(plain),
			_ => None,
		})
		.collect::<Vec<_>>()
		.join("\n\n")
}
