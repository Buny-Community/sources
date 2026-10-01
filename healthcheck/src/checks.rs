//! The check battery: walks the path a reader actually takes through a source.

use crate::runner::{Outcome, SourceRunner};
use anyhow::Result;
use buny::{Chapter, ContentBlock, HomeLayout, Listing, ListingKind, Novel};
use serde::Deserialize;

/// Mirror of `buny::NovelPageResult`, which is `Serialize`-only upstream
/// (the app never sends one in, so it has no `Deserialize`).
/// Field order must match: postcard encodes structs positionally.
#[derive(Debug, Deserialize)]
pub struct PageResult {
    pub entries: Vec<Novel>,
    pub has_next_page: bool,
}
use serde::Serialize;
use wasmer::Value;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    /// Worked, but returned something suspicious.
    Warn,
    Fail,
    /// Site is alive but shielded from this network; result is inconclusive.
    Blocked,
    /// Entrypoint not implemented, or a prerequisite check failed.
    Skip,
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub name: &'static str,
    pub status: Status,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub source: String,
    pub checks: Vec<Check>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub log: String,
}

impl Report {
    /// Only genuine failures set a non-zero exit; a bot wall must not.
    pub fn failed(&self) -> bool {
        self.checks.iter().any(|c| c.status == Status::Fail)
    }

    pub fn blocked(&self) -> bool {
        self.checks.iter().any(|c| c.status == Status::Blocked)
    }
}

/// Minimum characters of chapter text before we believe the content parsed.
const MIN_CONTENT_CHARS: usize = 200;

pub struct Options {
    pub query: String,
    pub min_content_chars: usize,
    /// Base URL from the manifest, probed to classify blocked vs broken.
    pub base_url: Option<String>,
    /// Listings declared in the source's `res/source.json`.
    ///
    /// Static listings never appear as a `get_listings` export: the app reads
    /// them from the manifest and passes each one to `get_novel_list`.
    pub listings: Vec<Listing>,
}

/// The subset of `res/source.json` we need.
#[derive(Debug, serde::Deserialize)]
struct Manifest {
    #[serde(default)]
    listings: Vec<ManifestListing>,
    info: Option<ManifestInfo>,
}

#[derive(Debug, serde::Deserialize)]
struct ManifestInfo {
    url: Option<String>,
}

/// Reads the base URL a source declares, for the reachability probe.
pub fn base_url_from_manifest(path: &std::path::Path) -> anyhow::Result<Option<String>> {
    let raw = std::fs::read_to_string(path)?;
    let manifest: Manifest = serde_json::from_str(&raw)?;
    Ok(manifest.info.and_then(|i| i.url))
}

#[derive(Debug, serde::Deserialize)]
struct ManifestListing {
    id: String,
    name: String,
    #[serde(default)]
    kind: Option<String>,
}

/// Reads the listings a source declares in its manifest.
pub fn listings_from_manifest(path: &std::path::Path) -> anyhow::Result<Vec<Listing>> {
    let raw = std::fs::read_to_string(path)?;
    let manifest: Manifest = serde_json::from_str(&raw)?;
    Ok(manifest
        .listings
        .into_iter()
        .map(|l| Listing {
            id: l.id,
            name: l.name,
            kind: match l.kind.as_deref() {
                Some("list") => ListingKind::List,
                _ => ListingKind::Default,
            },
        })
        .collect())
}

/// The user agent buny-test-runner's host sends, mirrored so the probe sees
/// what the source itself would see.
const HOST_USER_AGENT: &str = "Buny/1 CFNetwork/3826.500.131 Darwin/24.5.0";

pub fn run(runner: &mut SourceRunner, source: &str, opts: &Options) -> Result<Report> {
    let mut checks = Vec::new();

    // Probe first: its verdict decides whether later failures mean "broken" or
    // merely "we cannot see in from here".
    let reachability = opts
        .base_url
        .as_deref()
        .map(|url| crate::preflight::probe(url, HOST_USER_AGENT));
    match &reachability {
        Some(crate::preflight::Reachability::Open) => {
            checks.push(pass("reachable", "base URL responded normally"))
        }
        Some(crate::preflight::Reachability::Blocked(detail)) => checks.push(Check {
            name: "reachable",
            status: Status::Blocked,
            detail: format!("{detail}; deeper checks cannot be trusted from this network"),
        }),
        Some(crate::preflight::Reachability::Dead(detail)) => {
            checks.push(fail("reachable", detail.clone()))
        }
        None => checks.push(skip("reachable", "no base URL in manifest")),
    }
    let shielded = matches!(reachability, Some(crate::preflight::Reachability::Blocked(_)));

    // --- search -------------------------------------------------------------
    let query = runner.descriptor_str(opts.query.clone());
    let filters = runner.descriptor(&Vec::<buny::FilterValue>::new())?;
    let search: Option<PageResult> = match runner
        .call::<PageResult>(
            "get_search_novel_list",
            &[Value::I32(query), Value::I32(1), Value::I32(filters)],
        )
        .map(|o| o.into_ok("search"))
    {
        Ok(Ok(result)) => {
            if result.entries.is_empty() {
                // The usual shape of selector rot: a clean Ok with nothing in it.
                checks.push(fail("search", format!(
                    "returned 0 results for query {:?} (parsed without error, so selectors likely broke)",
                    opts.query
                )));
                None
            } else {
                let bad_keys = result.entries.iter().filter(|n| n.key.is_empty()).count();
                let bad_titles = result.entries.iter().filter(|n| n.title.is_empty()).count();
                let mut detail = format!("{} results", result.entries.len());
                if result.has_next_page {
                    detail.push_str(", has next page");
                }
                if bad_keys > 0 || bad_titles > 0 {
                    checks.push(warn("search", format!(
                        "{detail}, but {bad_keys} with empty key and {bad_titles} with empty title"
                    )));
                } else {
                    checks.push(pass("search", detail));
                }
                Some(result)
            }
        }
        Ok(Err(e)) | Err(e) => {
            checks.push(fail("search", format!("{e:#}")));
            None
        }
    };

    // Everything downstream needs a novel to work from.
    let seed: Option<Novel> = search.and_then(|r| r.entries.into_iter().next());

    // --- details ------------------------------------------------------------
    let detailed: Option<Novel> = match &seed {
        None => {
            checks.push(skip("details", "no novel from search"));
            None
        }
        Some(novel) => {
            let d = runner.descriptor(novel)?;
            match runner
                .call::<Novel>(
                    "get_novel_update",
                    &[Value::I32(d), Value::I32(1), Value::I32(0), Value::I32(1)],
                )
                .map(|o| o.into_ok("details"))
            {
                Ok(Ok(full)) => {
                    let mut missing = Vec::new();
                    if full.title.is_empty() {
                        missing.push("title");
                    }
                    if full.description.as_deref().unwrap_or("").is_empty() {
                        missing.push("description");
                    }
                    if full.authors.as_ref().map_or(true, |a| a.is_empty()) {
                        missing.push("authors");
                    }
                    if full.cover.as_deref().unwrap_or("").is_empty() {
                        missing.push("cover");
                    }
                    if full.title.is_empty() {
                        checks.push(fail("details", format!("empty title for {:?}", novel.key)));
                    } else if !missing.is_empty() {
                        checks.push(warn("details", format!("{:?}: missing {}", full.title, missing.join(", "))));
                    } else {
                        checks.push(pass("details", format!("{:?} fully populated", full.title)));
                    }
                    Some(full)
                }
                Ok(Err(e)) | Err(e) => {
                    checks.push(fail("details", format!("{e:#}")));
                    None
                }
            }
        }
    };

    // --- chapters -----------------------------------------------------------
    let base = detailed.or(seed);
    let chapters: Option<(Novel, Vec<Chapter>)> = match &base {
        None => {
            checks.push(skip("chapters", "no novel to list chapters for"));
            None
        }
        Some(novel) => {
            let d = runner.descriptor(novel)?;
            match runner
                .call::<Novel>(
                    "get_novel_update",
                    &[Value::I32(d), Value::I32(0), Value::I32(1), Value::I32(1)],
                )
                .map(|o| o.into_ok("chapters"))
            {
                Ok(Ok(with_chapters)) => match with_chapters.chapters.clone() {
                    Some(list) if !list.is_empty() => {
                        let bad = list.iter().filter(|c| c.key.is_empty()).count();
                        let numbered = list.iter().filter(|c| c.chapter_number.is_some()).count();
                        let mut detail = format!("{} chapters", list.len());
                        if numbered < list.len() {
                            detail.push_str(&format!(", {} without a chapter number", list.len() - numbered));
                        }
                        if bad > 0 {
                            checks.push(fail("chapters", format!("{detail}, {bad} with an empty key")));
                            None
                        } else {
                            checks.push(pass("chapters", detail));
                            Some((with_chapters, list))
                        }
                    }
                    _ => {
                        checks.push(fail("chapters", "returned an empty chapter list".to_string()));
                        None
                    }
                },
                Ok(Err(e)) | Err(e) => {
                    checks.push(fail("chapters", format!("{e:#}")));
                    None
                }
            }
        }
    };

    // --- chapter content ----------------------------------------------------
    match chapters {
        None => checks.push(skip("content", "no chapters to read")),
        Some((novel, list)) => {
            // Prefer an unlocked chapter; paywalled ones legitimately return nothing.
            match list.iter().find(|c| !c.locked) {
                None => checks.push(skip("content", "every chapter is locked")),
                Some(chapter) => {
                    // Sites intermittently serve a stub page to datacenter IPs
                    // (novelfire has returned just a "chapter reviews" link),
                    // so a thin result gets one retry before it counts.
                    let mut attempt = 0;
                    let outcome = loop {
                        attempt += 1;
                        let nd = runner.descriptor(&novel)?;
                        let cd = runner.descriptor(chapter)?;
                        let outcome = runner
                            .call::<Vec<ContentBlock>>(
                                "get_chapter_content_list",
                                &[Value::I32(nd), Value::I32(cd)],
                            )
                            .map(|o| o.into_ok("content"));
                        let thin = match &outcome {
                            Ok(Ok(blocks)) => {
                                blocks.iter().map(block_len).sum::<usize>() < opts.min_content_chars
                            }
                            _ => true,
                        };
                        if !thin || attempt == 2 {
                            break outcome;
                        }
                        std::thread::sleep(std::time::Duration::from_secs(5));
                    };
                    match outcome
                    {
                        Ok(Ok(blocks)) => {
                            let chars: usize = blocks.iter().map(block_len).sum();
                            let label = chapter.title.clone().unwrap_or_else(|| chapter.key.clone());
                            if blocks.is_empty() {
                                checks.push(fail("content", format!("{label:?}: 0 content blocks")));
                            } else if chars < opts.min_content_chars {
                                // Quote what came back: a soft block ("enable
                                // JavaScript", "checking your browser") and real
                                // selector rot are indistinguishable from a count.
                                checks.push(fail("content", format!(
                                    "{label:?}: only {chars} characters across {} blocks (below {}); got {:?}",
                                    blocks.len(), opts.min_content_chars, excerpt(&blocks, 160)
                                )));
                            } else {
                                checks.push(pass("content", format!(
                                    "{label:?}: {chars} characters across {} blocks", blocks.len()
                                )));
                            }
                        }
                        Ok(Err(e)) | Err(e) => checks.push(fail("content", format!("{e:#}"))),
                    }
                }
            }
        }
    }

    // --- optional entrypoints ----------------------------------------------
    optional_filters(runner, &mut checks)?;
    optional_listings(runner, &mut checks, opts)?;
    optional_home(runner, &mut checks)?;

    // The probe only exists to interpret other failures. If everything else
    // worked, a failed probe is noise about the probe, not the source.
    if checks.iter().any(|c| c.name == "reachable" && c.status == Status::Fail)
        && !checks
            .iter()
            .any(|c| c.name != "reachable" && c.status == Status::Fail)
    {
        if let Some(check) = checks.iter_mut().find(|c| c.name == "reachable") {
            check.status = Status::Warn;
            check.detail = format!("{} (every other check passed)", check.detail);
        }
    }

    // A `cf-mitigated` response proves the network was blocked, even when the
    // base URL answered normally: Cloudflare often guards one endpoint (here,
    // Scribble Hub's chapter-list POST) far harder than the rest of the site.
    let shielded = shielded || crate::net::saw_mitigation();

    // Behind a bot wall, a failure says nothing about the source itself.
    if shielded {
        for check in checks.iter_mut() {
            if check.status == Status::Fail {
                check.status = Status::Blocked;
            }
        }
    }

    Ok(Report { source: source.to_string(), checks, log: runner.stdout().trim_end().to_string() })
}

fn optional_filters(runner: &mut SourceRunner, checks: &mut Vec<Check>) -> Result<()> {
    if !runner.has_export("get_filters") {
        return Ok(());
    }
    // `Filter` has a hand-written Serialize and no Deserialize, so we verify the
    // call succeeds and returns a payload rather than mirroring that impl.
    match runner.call_raw("get_filters", &[]) {
        Ok(Outcome::Ok(bytes)) if bytes.len() <= 1 => {
            checks.push(warn("filters", "returned an empty filter list".to_string()))
        }
        Ok(Outcome::Ok(bytes)) => checks.push(pass("filters", format!("{} bytes of filter config", bytes.len()))),
        Ok(Outcome::Unimplemented) => {}
        Ok(other) => checks.push(fail("filters", format!("{other:?}"))),
        Err(e) => checks.push(fail("filters", format!("{e:#}"))),
    }
    Ok(())
}

fn optional_listings(runner: &mut SourceRunner, checks: &mut Vec<Check>, opts: &Options) -> Result<()> {
    if !runner.has_export("get_novel_list") {
        return Ok(());
    }

    // Dynamic listings override the manifest when the source provides them.
    let listings = if runner.has_export("get_listings") {
        match runner.call::<Vec<Listing>>("get_listings", &[]) {
            Ok(Outcome::Ok(l)) if !l.is_empty() => l,
            Ok(Outcome::Unimplemented) => opts.listings.clone(),
            Ok(Outcome::Ok(_)) => opts.listings.clone(),
            Ok(other) => {
                checks.push(fail("listings", format!("{other:?}")));
                return Ok(());
            }
            Err(e) => {
                checks.push(fail("listings", format!("{e:#}")));
                return Ok(());
            }
        }
    } else {
        opts.listings.clone()
    };

    if listings.is_empty() {
        checks.push(skip("listings", "source exports get_novel_list but declares no listings"));
        return Ok(());
    }

    // Each listing is its own page in the app, so a broken one is a real
    // failure even when search works.
    for listing in &listings {
        let d = runner.descriptor(listing)?;
        match runner
            .call::<PageResult>("get_novel_list", &[Value::I32(d), Value::I32(1)])
            .map(|o| o.into_ok("listing"))
        {
            Ok(Ok(page)) => {
                if page.entries.is_empty() {
                    checks.push(fail("listing", format!("{:?} returned 0 entries", listing.name)));
                } else {
                    let bad = page.entries.iter().filter(|n| n.key.is_empty() || n.title.is_empty()).count();
                    if bad > 0 {
                        checks.push(warn("listing", format!(
                            "{:?}: {} entries, {bad} malformed", listing.name, page.entries.len()
                        )));
                    } else {
                        checks.push(pass("listing", format!("{:?}: {} entries", listing.name, page.entries.len())));
                    }
                }
            }
            Ok(Err(e)) | Err(e) => checks.push(fail("listing", format!("{:?}: {e:#}", listing.name))),
        }
    }
    Ok(())
}

fn optional_home(runner: &mut SourceRunner, checks: &mut Vec<Check>) -> Result<()> {
    if !runner.has_export("get_home") {
        return Ok(());
    }
    match runner.call::<HomeLayout>("get_home", &[]) {
        Ok(Outcome::Ok(home)) => {
            if home.components.is_empty() {
                checks.push(fail("home", "home layout has no components".to_string()));
            } else {
                checks.push(pass("home", format!("{} components", home.components.len())));
            }
        }
        Ok(Outcome::Unimplemented) => {}
        Ok(other) => checks.push(fail("home", format!("{other:?}"))),
        Err(e) => checks.push(fail("home", format!("{e:#}"))),
    }
    Ok(())
}

/// First `max` characters of a block list, for quoting in a failure.
fn excerpt(blocks: &[ContentBlock], max: usize) -> String {
    let mut text = String::new();
    for block in blocks {
        let part = match block {
            ContentBlock::Paragraph(t, _) | ContentBlock::BlockQuote(t) => t.trim().to_string(),
            ContentBlock::Table(rows) => rows
                .iter()
                .flatten()
                .map(|c| c.trim())
                .collect::<Vec<_>>()
                .join(" "),
            ContentBlock::Divider => String::new(),
        };
        if part.is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&part);
        if text.chars().count() >= max {
            break;
        }
    }
    text.chars().take(max).collect()
}

fn block_len(block: &ContentBlock) -> usize {
    match block {
        ContentBlock::Paragraph(text, _) | ContentBlock::BlockQuote(text) => text.trim().chars().count(),
        ContentBlock::Table(rows) => rows.iter().flatten().map(|c| c.trim().chars().count()).sum(),
        ContentBlock::Divider => 0,
    }
}

fn pass(name: &'static str, detail: impl Into<String>) -> Check {
    Check { name, status: Status::Pass, detail: detail.into() }
}
fn warn(name: &'static str, detail: impl Into<String>) -> Check {
    Check { name, status: Status::Warn, detail: detail.into() }
}
fn fail(name: &'static str, detail: impl Into<String>) -> Check {
    Check { name, status: Status::Fail, detail: detail.into() }
}
fn skip(name: &'static str, detail: impl Into<String>) -> Check {
    Check { name, status: Status::Skip, detail: detail.into() }
}

pub const DEFAULT_MIN_CONTENT_CHARS: usize = MIN_CONTENT_CHARS;
