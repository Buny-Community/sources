use super::*;

#[test]
fn test_split_label() {
	assert_eq!(
		split_label("Chapter 1 – So it begins"),
		Some((1.0, "So it begins"))
	);
	assert_eq!(split_label("Chapter 01 : Reborn"), Some((1.0, "Reborn")));
	assert_eq!(split_label("Chapter 2. Welcome"), Some((2.0, "Welcome")));
	assert_eq!(split_label("1: After Death"), Some((1.0, "After Death")));
	assert_eq!(
		split_label("77. Immortal Blue Dragon"),
		Some((77.0, "Immortal Blue Dragon"))
	);
	assert_eq!(split_label("Chapter 389 Mission"), Some((389.0, "Mission")));
	assert_eq!(split_label("Chapter 12"), Some((12.0, "")));
	assert_eq!(split_label("Ch. 5 - Name"), Some((5.0, "Name")));
	assert_eq!(split_label("100 Days of Rain"), None);
	assert_eq!(split_label("1.01 Loading In"), None);
	assert_eq!(split_label("Chapters 354"), None);
	assert_eq!(split_label("Prologue"), None);
	assert_eq!(split_label("3"), Some((3.0, "")));
	assert_eq!(split_label("1 | Waking up"), Some((1.0, "Waking up")));
	assert_eq!(
		split_label("Chapter no.1: I Choose You"),
		Some((1.0, "I Choose You"))
	);
	assert_eq!(split_label("01 Long Ago"), None);
	assert_eq!(
		split_label_with("01 Long Ago", true),
		Some((1.0, "Long Ago"))
	);
	// Novel Archive, "A Knight Who Eternally Regresses".
	assert_eq!(split_label("Chapter 0: Prologue"), Some((0.0, "Prologue")));
	assert_eq!(split_label("Chapter 0 - Prolog"), Some((0.0, "Prolog")));
	assert_eq!(
		split_label("Chapter 745.1: Glossary of translation"),
		Some((745.1, "Glossary of translation"))
	);
	// Royal Road: part numbers and book-chapter numbers.
	assert_eq!(
		split_label("Chapter 23-2: Favors Across Time"),
		Some((23.0, "2: Favors Across Time"))
	);
	assert_eq!(split_label("1-01 Isekai"), None);
	assert_eq!(split_label("Book 2, Chapter 34: Res Ipsa Loquitur"), None);
	// NovelFull.
	assert_eq!(
		split_label("Chapter 1632-2 – Ruthlessness"),
		Some((1632.0, "2 – Ruthlessness"))
	);
	assert_eq!(split_label("Chapter -1 Volume 2 Glossary"), None);
}

#[test]
fn test_chapter_numbers() {
	// Labels, with a sheet before chapter 1, an interlude and an epilogue.
	assert_eq!(
		chapter_numbers(&[
			"Skills",
			"Chapter 1",
			"Chapter 2",
			"Chapter 3",
			"Interlude",
			"Chapter 4",
			"Chapter 5",
			"Chapter 6",
			"Chapter 7",
			"Chapter 8",
			"Chapter 9",
			"Chapter 10",
			"Chapter 11",
			"Chapter 12",
			"Epilogue",
		]),
		vec![
			0.5, 1.0, 2.0, 3.0, 3.5, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0, 12.5
		]
	);
	// Two unlabeled chapters in a row share the gap.
	assert_eq!(
		chapter_numbers(&[
			"1: A", "2: B", "Side A", "Side B", "3: C", "4: D", "5: E", "6: F", "7: G", "8: H"
		]),
		vec![1.0, 2.0, 2.33, 2.67, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]
	);
	// A repeated number is a second part of that chapter.
	assert_eq!(
		chapter_numbers(&["Chapter 1", "Chapter 2", "Chapter 2", "Chapter 3"]),
		vec![1.0, 2.0, 2.5, 3.0]
	);
	// Numbering that restarts (per book): positions.
	assert_eq!(
		chapter_numbers(&[
			"Chapter 1",
			"Chapter 2",
			"Chapter 3",
			"Chapter 1",
			"Chapter 2"
		]),
		vec![1.0, 2.0, 3.0, 4.0, 5.0]
	);
	// "Book-chapter" numbers: positions.
	assert_eq!(
		chapter_numbers(&["1-01 Isekai", "1-02 Headache", "1-03 Villainess"]),
		vec![1.0, 2.0, 3.0]
	);
	// Mostly unlabeled: positions.
	assert_eq!(
		chapter_numbers(&["Reborn?!", "Kurama", "Chapter 3"]),
		vec![1.0, 2.0, 3.0]
	);
	// Every name is "NN Title": numbered from the names, labels stripped.
	let names = [
		"01 Long Ago",
		"02 Not Long Ago",
		"03 Recently",
		"04 Now",
		"05 Later",
	];
	assert_eq!(chapter_numbers(&names), vec![1.0, 2.0, 3.0, 4.0, 5.0]);
	assert_eq!(chapter_title(names[2], 3.0), Some("Recently".into()));
	assert!(chapter_numbers(&[]).is_empty());
}

#[test]
fn test_chapter_zero() {
	// Novel Archive's "A Knight Who Eternally Regresses" starts at "Chapter 0".
	// Numbered by position it showed as chapter 1, and every later chapter one
	// too high.
	let names = [
		"Chapter 0 - Prolog",
		"Chapter 1 - My Dream was to be a Knight",
		"Chapter 2 - Zoetrope",
		"Chapter 3 - A Day",
		"Chapter 4 - The Heart of the Beast",
	];
	assert_eq!(chapter_numbers(&names), vec![0.0, 1.0, 2.0, 3.0, 4.0]);
	assert_eq!(chapter_title(names[0], 0.0), Some("Prolog".into()));
	assert_eq!(chapter_title(names[2], 2.0), Some("Zoetrope".into()));
}

#[test]
fn test_one_typo_in_a_long_list() {
	// The same novel, live: 958 names from "Chapter 0: Prologue" to "Chapter
	// 956", where the site labels chapter 119 "Chapter 118: Leap" and adds a
	// "Chapter 745.1" glossary. A strict check would number all of it by
	// position.
	let mut names: Vec<String> = (0..=956)
		.map(|n| match n {
			119 => "Chapter 118: Leap".to_string(),
			_ => format!("Chapter {n}: Name"),
		})
		.collect();
	names.insert(746, "Chapter 745.1: Glossary of translation".into());
	let refs: Vec<&str> = names.iter().map(String::as_str).collect();
	let numbers = chapter_numbers(&refs);
	assert_eq!(numbers.len(), 958);
	assert_eq!(numbers[0], 0.0);
	assert_eq!(numbers[118], 118.0);
	assert_eq!(numbers[119], 119.0);
	assert_eq!(numbers[120], 120.0);
	assert_eq!(numbers[746], 745.1);
	assert_eq!(numbers[957], 956.0);
	assert!(numbers.windows(2).all(|w| w[0] < w[1]));
	// The mislabeled one keeps its whole name.
	assert_eq!(
		chapter_title(refs[119], numbers[119]),
		Some("Chapter 118: Leap".into())
	);
}

#[test]
fn test_many_out_of_order() {
	// More than one label in 50 out of order: positions.
	let mut names: Vec<String> = (1..=50).map(|n| format!("Chapter {n}")).collect();
	names[10] = "Chapter 3".into();
	names[30] = "Chapter 5".into();
	let refs: Vec<&str> = names.iter().map(String::as_str).collect();
	let numbers = chapter_numbers(&refs);
	assert_eq!(numbers[0], 1.0);
	assert_eq!(numbers[49], 50.0);
	assert_eq!(numbers[10], 11.0);
}

#[test]
fn test_chapter_title() {
	assert_eq!(
		chapter_title("Chapter 3 – Geography", 3.0),
		Some("Geography".into())
	);
	assert_eq!(
		chapter_title("Chapter 3 – Geography", 4.0),
		Some("Chapter 3 – Geography".into())
	);
	assert_eq!(chapter_title("Chapter 5", 5.0), None);
	assert_eq!(chapter_title("Prologue", 0.5), Some("Prologue".into()));
	// The label repeated (NovelFull, Novel Archive).
	assert_eq!(
		chapter_title("Chapter 1147 - 1147: Vice Sect Leader Chen Yi", 1147.0),
		Some("Vice Sect Leader Chen Yi".into())
	);
	assert_eq!(
		chapter_title("Chapter 1 - Chapter 1: Reincarnated as… What?!", 1.0),
		Some("Reincarnated as… What?!".into())
	);
	assert_eq!(
		chapter_title("Chapter 1568 - 1566: Past and Present Lives 4", 1568.0),
		Some("1566: Past and Present Lives 4".into())
	);
	assert_eq!(chapter_title("12 chapter", 12.0), None);
}

#[test]
fn test_round2() {
	assert_eq!(round2(0.499_99), 0.5);
	assert_eq!(round2(2.333_333), 2.33);
	assert_eq!(round2(-0.5), -0.5);
}
