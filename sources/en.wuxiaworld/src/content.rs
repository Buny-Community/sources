// Chapter bodies, translator notes and synopses are rich-text editor HTML
// (adapted from en.wetriedtls): `<p dir="ltr">` blocks of styled `<span>`s with
// inline `<em>`, `<i>`, `<strong>`, `<u>`, `<a>` and `<br>`, the odd list,
// `<h3>` or `<img>`, and scene breaks as `<hr>` or as text ("***", "* * *",
// "---"). In-story angle brackets arrive escaped (`&lt;Skill&gt;`), so every
// real `<` that starts a tag is markup and can be dropped once interpreted.

use buny::{
	ContentBlock,
	alloc::{String, Vec, string::ToString},
	prelude::*,
};

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

impl Builder {
	fn text(&mut self, raw: &str) {
		if raw.is_empty() {
			return;
		}
		// Newlines in the source HTML are plain whitespace; &nbsp; is only used
		// as spacing, so it becomes a normal space.
		let text = unescape(raw).replace(['\r', '\n', '\u{a0}'], " ");
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

	fn tag(&mut self, tag: &str) {
		let closing = tag.starts_with('/');
		let name_part = tag.trim_start_matches('/');
		let name_end = name_part
			.find(|c: char| !c.is_ascii_alphanumeric())
			.unwrap_or(name_part.len());
		let name = name_part[..name_end].to_ascii_lowercase();
		match name.as_str() {
			"strong" | "b" => {
				self.bold = if closing {
					self.bold.saturating_sub(1)
				} else {
					self.bold + 1
				}
			}
			"em" | "i" => {
				self.italic = if closing {
					self.italic.saturating_sub(1)
				} else {
					self.italic + 1
				}
			}
			"hr" => {
				self.break_line();
				self.lines.push(Line::Divider);
			}
			"img" if !closing => {
				self.break_line();
				if let Some(src) = attribute(tag, "src") {
					self.lines.push(Line::Image(unescape(src)));
				}
			}
			"p" | "br" | "div" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "li" | "ul" | "ol"
			| "blockquote" | "figure" | "figcaption" | "table" | "tr" => self.break_line(),
			// td/th, span, a, u, s, sup... carry no structure the reader can show.
			"td" | "th" => self.text(" "),
			_ => {}
		}
	}
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
			value.split(|c: char| c.is_whitespace()).next()
		};
	}
	None
}

fn parse(html: &str) -> Vec<Line> {
	let mut builder = Builder::default();
	let mut rest = html;
	while let Some(start) = rest.find('<') {
		builder.text(&rest[..start]);
		let after = &rest[start + 1..];
		let is_tag = after.starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!');
		match after.find('>') {
			Some(end) if is_tag => {
				builder.tag(&after[..end]);
				rest = &after[end + 1..];
			}
			// A bare '<' in text; keep it.
			_ => {
				builder.text("<");
				rest = after;
			}
		}
	}
	builder.text(rest);
	builder.break_line();
	builder.lines
}

// A line made only of break characters: "* * *", "***", "──────", "# # #", "____".
fn is_scene_break(plain: &str) -> bool {
	let mut count = 0;
	for c in plain.chars().filter(|c| !c.is_whitespace()) {
		if !matches!(
			c,
			'*' | '~' | '=' | '#' | '─' | '━' | '-' | '—' | '_' | '◇' | '◆'
		) {
			return false;
		}
		count += 1;
	}
	count >= 3
}

// Credit, plug and navigation lines. Wuxiaworld keeps credits in the novel
// page and the translator's note, so these are rare in bodies; they are only
// trimmed at the edges. Chapters imported from the old site still carry its
// "Previous Chapter | Next Chapter" links.
fn is_credit(plain: &str) -> bool {
	let lower = plain.to_lowercase();
	let nav = lower
		.split(|c: char| !c.is_alphabetic())
		.filter(|w| !w.is_empty())
		.all(|w| {
			matches!(
				w,
				"previous" | "next" | "chapter" | "table" | "of" | "contents" | "index"
			)
		});
	(nav && ["previous", "next", "contents"]
		.iter()
		.any(|w| lower.contains(w)))
		|| lower.contains("discord.gg")
		|| lower.contains("ko-fi.com")
		|| lower.contains("patreon.com")
		|| [
			"translator:",
			"translated by",
			"editor:",
			"edited by",
			"proofreader:",
			"tl:",
			"ed:",
			"pr:",
		]
		.iter()
		.any(|p| lower.trim_start_matches(['[', '(']).starts_with(p))
}

// Lowercase letters and digits only, so "Prologue - Name" matches "Prologue" + "Name".
fn normalize(s: &str) -> String {
	s.chars()
		.filter(|c| c.is_alphanumeric())
		.flat_map(char::to_lowercase)
		.collect()
}

// The heading block repeats the chapter ("Chapter 50: Help?", "WDQK Chapter
// 26: The Hunt", "Prologue") and sometimes the series ("Killer Nights").
// `headings` are the chapter name, the series name, and the chapter name
// without its "Chapter N" label.
fn is_heading(plain: &str, markdown: &str, headings: &[&str; 3]) -> bool {
	let plain = plain.trim();
	// A label with a word before the number ("Chapter 5", "WDQK 26"), so an
	// opening line such as "474 people..." isn't taken for a bare "474 - Name".
	let word_first = plain
		.find(|c: char| c.is_ascii_digit())
		.is_some_and(|i| plain[..i].chars().any(char::is_alphabetic));
	if word_first && crate::split_label(plain).is_some() {
		return true;
	}
	let line = normalize(plain);
	if line.is_empty() {
		return false;
	}
	let [name, series, title] = headings.map(normalize);
	if [&name, &series, &title]
		.iter()
		.any(|h| !h.is_empty() && **h == line)
	{
		return true;
	}
	// Looser containment only for a line that is bold as a whole, as headings
	// are, so an opening sentence that happens to contain a name stays.
	let bold = markdown.starts_with("**") && markdown.ends_with("**");
	bold && [&name, &series, &title]
		.iter()
		.any(|h| h.chars().count() >= 4 && line.contains(h.as_str()))
}

/// Converts a chapter body to reader blocks, dropping a heading or credit line
/// that opens the chapter and a plug that closes it.
/// `headings` are the chapter name, series name and bare title (see `is_heading`).
pub fn chapter_blocks(html: &str, headings: &[&str; 3]) -> Vec<ContentBlock> {
	let lines = parse(html);

	let is_break = |line: &Line| match line {
		Line::Divider => true,
		Line::Text { plain, .. } => is_scene_break(plain),
		Line::Image(_) => false,
	};
	let start = lines
		.iter()
		.position(|line| match line {
			Line::Text { plain, markdown } => {
				!is_credit(plain)
					&& !is_heading(plain, markdown, headings)
					&& !is_scene_break(plain)
			}
			Line::Divider => false,
			Line::Image(_) => true,
		})
		.unwrap_or(lines.len());
	let mut end = lines.len();
	while end > start {
		let junk = match &lines[end - 1] {
			Line::Divider => true,
			// A "Footnotes:" header with nothing under it once the plug is gone.
			Line::Text { plain, .. } => {
				is_credit(plain)
					|| is_scene_break(plain)
					|| plain
						.trim_end_matches(':')
						.trim()
						.eq_ignore_ascii_case("footnotes")
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
			// "<hr>" right after "──────" is one break, not two.
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

/// Plain text with paragraphs separated by blank lines (for descriptions).
pub fn plain_text(html: &str) -> String {
	parse(html)
		.into_iter()
		.filter_map(|line| match line {
			Line::Text { plain, .. } => Some(plain),
			_ => None,
		})
		.collect::<Vec<_>>()
		.join("\n\n")
}
