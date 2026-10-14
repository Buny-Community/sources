use buny::{
	Novel, Result, Review, ReviewPageResult, ReviewProvider,
	alloc::Vec,
	imports::{net::Request, std::current_date},
	prelude::*,
};

use crate::{BASE_URL, NovelFire};

impl ReviewProvider for NovelFire {
	fn get_novel_reviews(&self, novel: Novel, page: i32) -> Result<ReviewPageResult> {
		let url = format!("{}/book/{}/reviews?page={page}", BASE_URL, novel.key);
		let html = Request::get(&url)?.html()?;
		let now = current_date();

		let entries = html
			.select(".review-item")
			.map(|els| {
				els.map(|el| {
					let avatar = el
						.select_first("img.avatar")
						.and_then(|img| img.attr("src"))
						.filter(|src| !src.ends_with("/default-avatar.jpg"))
						.map(|src| {
							if src.starts_with("https://") {
								src
							} else {
								format!("{}{src}", BASE_URL)
							}
						});
					let author = el
						.select_first(".username")
						.and_then(|a| a.text())
						.unwrap_or_default();
					// One paragraph per <p>; split_whitespace also drops the &nbsp; padding.
					let content = el
						.select("[itemprop=reviewBody] p")
						.map(|ps| {
							ps.filter_map(|p| p.text())
								.map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
								.filter(|t| !t.is_empty())
								.collect::<Vec<_>>()
								.join("\n\n")
						})
						.filter(|text| !text.is_empty());
					// data-rating is 0.5-5 in half stars
					let rating = el
						.select_first(".user-rating")
						.and_then(|r| r.attr("data-rating"))
						.and_then(|s| s.trim().parse::<f32>().ok())
						.map(|stars| (stars / 5.0).clamp(0.0, 1.0));
					let date = el
						.select_first(".review-date")
						.and_then(|d| d.text())
						.and_then(|d| relative_seconds(&d))
						.map(|ago| now - ago);
					Review {
						author,
						avatar,
						title: None,
						content,
						rating,
						date,
					}
				})
				.collect::<Vec<Review>>()
			})
			.unwrap_or_default();

		let has_next_page = html.select_first(".pagination a[rel=next]").is_some();

		Ok(ReviewPageResult {
			entries,
			has_next_page,
		})
	}
}

/// "2 weeks ago" -> seconds. The site only shows relative dates, so months and years are approximate.
fn relative_seconds(text: &str) -> Option<i64> {
	let mut words = text.split_whitespace();
	let n = words.next()?.parse::<i64>().ok()?;
	let unit = match words.next()?.trim_end_matches('s') {
		"second" => 1,
		"minute" => 60,
		"hour" => 3_600,
		"day" => 86_400,
		"week" => 604_800,
		"month" => 2_592_000,
		"year" => 31_536_000,
		_ => return None,
	};
	Some(n * unit)
}

#[cfg(test)]
mod test {
	use super::relative_seconds;
	use buny_test::buny_test;

	#[buny_test]
	fn parses_relative_dates() {
		assert_eq!(relative_seconds("2 weeks ago"), Some(1_209_600));
		assert_eq!(relative_seconds("1 day ago"), Some(86_400));
		assert_eq!(relative_seconds("3 years ago"), Some(94_608_000));
		assert_eq!(relative_seconds("just now"), None);
	}
}
