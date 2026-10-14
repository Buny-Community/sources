use buny::{
	Novel, Result, Review, ReviewPageResult, ReviewProvider,
	alloc::Vec,
	imports::{html::Html, net::Request},
	prelude::*,
};

use crate::{BASE_URL, RoyalRoad};

impl ReviewProvider for RoyalRoad {
	fn get_novel_reviews(&self, novel: Novel, page: i32) -> Result<ReviewPageResult> {
		let url = format!("{}/fiction/{}?reviews={page}", BASE_URL, novel.key);
		let html = Request::get(&url)?.html()?;

		let entries = html
			.select(".reviews-container .review")
			.map(|els| {
				els.map(|el| {
					let avatar = el
						.select_first("img[data-type=avatar]")
						.and_then(|img| img.attr("src"))
						.map(|src| {
							if src.starts_with("https://") {
								src
							} else {
								format!("{}{src}", BASE_URL)
							}
						});
					let author = el
						.select_first(".review-meta a[href^='/profile/']")
						.and_then(|a| a.text())
						.unwrap_or_default();
					let title = el.select_first(".review-header h4").and_then(|h| h.text());
					// Literal `\n` markers survive the app's whitespace-collapsing text().
					// Wrapped in our own div because the test runner's fragments have no <body>,
					// which is why ElementHelpers::text_with_newlines returns None there.
					let content = el
						.select_first(".review-inner")
						.and_then(|body| body.html())
						.and_then(|raw| {
							let marked = ["<br>", "<br/>", "<br />"]
								.iter()
								.fold(raw, |acc, br| acc.replace(br, "\\n"))
								.replace("</p>", "\\n</p>")
								.replace("</div>", "\\n</div>")
								.replace("</li>", "\\n</li>");
							Html::parse_fragment(format!("<div id=\"rr-review\">{marked}</div>"))
								.ok()?
								.select_first("#rr-review")?
								.text()
						})
						// One paragraph per non-empty line; drops the `<p>&nbsp;</p>` spacers.
						.map(|text| {
							text.split("\\n")
								.map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
								.filter(|line| !line.is_empty())
								.collect::<Vec<_>>()
								.join("\n\n")
						})
						.filter(|text| !text.is_empty());
					// aria-label is "4.5 stars"
					let rating = el
						.select_first(".overall-score-container [aria-label$=' stars']")
						.and_then(|s| s.attr("aria-label"))
						.and_then(|label| label.trim_end_matches(" stars").parse::<f32>().ok())
						.map(|stars| (stars / 5.0).clamp(0.0, 1.0));
					let date = el
						.select_first(".review-meta time[unixtime]")
						.and_then(|t| t.attr("unixtime"))
						.and_then(|s| s.trim().parse::<i64>().ok());
					Review {
						author,
						avatar,
						title,
						content,
						rating,
						date,
					}
				})
				.collect::<Vec<Review>>()
			})
			.unwrap_or_default();

		// Past the last page there is no active page; on the last page nothing follows it.
		let has_next_page = !entries.is_empty()
			&& html
				.select_first(".reviews-container .pagination .page-active + li")
				.is_some();

		Ok(ReviewPageResult {
			entries,
			has_next_page,
		})
	}
}
