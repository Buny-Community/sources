//! Chapter numbers from chapter names, for sites that only give a list of
//! names in reading order.
//!
//! The app identifies a chapter by its key, so numbers don't have to be the
//! site's. They still have to be unique and rising: the app sorts chapters on
//! (volume, number), and its source migration matches chapters on
//! "chapterNumber_volumeNumber". Numbering by position does that, but a
//! prologue then shifts every chapter up by one, so the names' own labels
//! ("Chapter 12 - Name") are used when they can be trusted.
#![cfg_attr(not(test), no_std)]

extern crate alloc;

use alloc::{string::String, string::ToString, vec, vec::Vec};

/// Splits a chapter label off the start of `s`: "Chapter 12", "Chapter 01 :",
/// "Ch. 12", "Chapter no.1:", "Episode 3", or a bare "12." / "12:" / "12 -" /
/// "12 |" / "12". Returns the number and the rest, without its leading
/// separator.
pub fn split_label(s: &str) -> Option<(f32, &str)> {
	split_label_with(s, false)
}

/// `loose` also takes a bare number followed by a space ("01 Long Ago"), which
/// is only trusted when most of a series' chapters are named that way.
pub fn split_label_with(s: &str, loose: bool) -> Option<(f32, &str)> {
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
	if word.is_empty() {
		// A bare number must be the whole name or be followed by a separator
		// ("12. Name", "12: Name", "12 - Name"), so a title that starts with a
		// number ("100 Days") stays.
		let separated = after.trim().is_empty()
			|| after.starts_with(['.', ':'])
			|| after.trim_start().starts_with(['-', '–', '—', ':', '|'])
			|| (loose && after.starts_with(' '));
		// "1-01 Isekai" is book 1, chapter 1, not chapter 1 named "01 Isekai".
		let compound = after
			.trim_start()
			.strip_prefix(['-', '–', '—'])
			.is_some_and(|r| r.trim_start().starts_with(|c: char| c.is_ascii_digit()));
		if !separated || compound {
			return None;
		}
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

/// One number per name, in reading order: unique and strictly rising.
///
/// Authors name chapters freely: most label them ("Chapter 12 – Name",
/// "12: Name"), and many add unlabeled ones (a prologue, character sheets,
/// interludes, an epilogue) or split one into parts ("Chapter 13 (Part 2)").
///
/// When at least 80% of chapters carry a label and the labels rise in reading
/// order, the labels are used, and each other chapter gets a number between its
/// labeled neighbours (a prologue before "Chapter 1" becomes 0.5, a second
/// "Chapter 13" becomes 13.5). A few labels out of order (a typo, at most one
/// in 50) are treated the same way. Otherwise every chapter is numbered by
/// position, from 1.
pub fn chapter_numbers(names: &[&str]) -> Vec<f32> {
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

/// The app shows the chapter number separately, so a label is dropped from the
/// title when its number is the one sent, and so is a repeat of it ("Chapter
/// 12 - 12: Name"). A name that is only a label ("Chapter 5") has no title.
pub fn chapter_title(name: &str, number: f32) -> Option<String> {
	fn strip(name: &str, number: f32) -> Option<&str> {
		[false, true]
			.iter()
			.find_map(|&loose| match split_label_with(name, loose) {
				Some((n, rest)) if n == number => Some(rest.trim()),
				_ => None,
			})
	}
	let title = match strip(name.trim(), number) {
		Some(rest) => strip(rest, number).unwrap_or(rest),
		None => name.trim(),
	};
	// "1 chapter", "2 chapter": a label written backwards.
	(!title.is_empty() && !title.eq_ignore_ascii_case("chapter")).then(|| title.to_string())
}

fn label_numbers(labels: &[Option<f32>]) -> Option<Vec<f32>> {
	let labeled = labels.iter().flatten().count();
	if labeled == 0 || labeled * 5 < labels.len() * 4 {
		return None;
	}

	let keep = rising_run(labels);
	let kept: Vec<Option<f32>> = labels
		.iter()
		.zip(&keep)
		.map(|(label, &k)| label.filter(|_| k))
		.collect();
	let kept_count = keep.iter().filter(|&&k| k).count();

	// A label left out of the run that repeats a neighbour's number is a part of
	// that chapter. Any other is out of order: a few are typos, many mean the
	// numbering restarts (per book) and the labels can't be used.
	let mut next = vec![None; labels.len() + 1];
	for i in (0..labels.len()).rev() {
		next[i] = kept[i].or(next[i + 1]);
	}
	let mut prev = None;
	let mut out_of_order = 0;
	for (i, label) in labels.iter().enumerate() {
		if kept[i].is_some() {
			prev = kept[i];
		} else if label.is_some() && *label != prev && *label != next[i] {
			out_of_order += 1;
		}
	}
	if out_of_order * 50 > labeled || kept_count * 2 < labels.len() {
		return None;
	}

	let mut out = Vec::with_capacity(kept.len());
	let mut i = 0;
	while i < kept.len() {
		if let Some(n) = kept[i] {
			out.push(n);
			i += 1;
			continue;
		}
		// A run of chapters without a usable label between two labels (or an end).
		let run_end = (i..kept.len())
			.find(|&j| kept[j].is_some())
			.unwrap_or(kept.len());
		let prev = if i == 0 { None } else { kept[i - 1] };
		let next = kept.get(run_end).copied().flatten();
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
	// Rounding can collapse numbers in a long run of chapters between close labels.
	out.windows(2).all(|w| w[0] < w[1]).then_some(out)
}

/// Marks the longest strictly rising run of labels (keeping the earliest of
/// equal ones), in O(n log n): lists reach 15,000 chapters.
fn rising_run(labels: &[Option<f32>]) -> Vec<bool> {
	// `tails[k]`: the smallest last label of a rising run of length k + 1, and
	// the index of that label.
	let mut tails: Vec<(f32, usize)> = Vec::new();
	let mut prev: Vec<Option<usize>> = vec![None; labels.len()];
	for (i, label) in labels.iter().enumerate() {
		let Some(x) = *label else { continue };
		let p = tails.partition_point(|&(t, _)| t < x);
		if tails.get(p).is_some_and(|&(t, _)| t == x) {
			continue;
		}
		prev[i] = p.checked_sub(1).map(|q| tails[q].1);
		if p == tails.len() {
			tails.push((x, i));
		} else {
			tails[p] = (x, i);
		}
	}
	let mut keep = vec![false; labels.len()];
	let mut cur = tails.last().map(|&(_, i)| i);
	while let Some(i) = cur {
		keep[i] = true;
		cur = prev[i];
	}
	keep
}

/// Two decimals, so a prologue shows as 0.5 rather than 0.49999.
fn round2(n: f32) -> f32 {
	let scaled = n * 100.0;
	let rounded = if scaled >= 0.0 {
		(scaled + 0.5) as i64
	} else {
		(scaled - 0.5) as i64
	};
	rounded as f32 / 100.0
}

#[cfg(test)]
mod tests;
