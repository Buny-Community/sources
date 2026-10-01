//! Health checker for Buny sources.
//!
//! Drives a built source's wasm against the live site and reports whether the
//! reader-facing path still works: search, details, chapter list, chapter text,
//! plus whichever optional entrypoints the source exports.
//!
//! The sources themselves are untouched; this calls the same ABI the app calls.

mod checks;
mod net;
mod preflight;
mod runner;

use anyhow::{Context, Result};
use checks::{Status, DEFAULT_MIN_CONTENT_CHARS};
use clap::Parser;
use runner::SourceRunner;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(about, version)]
struct Args {
    /// Path to a built source .wasm.
    wasm: PathBuf,

    /// Search term to probe with.
    #[arg(long, default_value = "the")]
    query: String,

    /// Minimum characters of chapter text before content counts as parsed.
    #[arg(long, default_value_t = DEFAULT_MIN_CONTENT_CHARS)]
    min_content_chars: usize,

    /// Write the machine-readable report here (for the workflow summary).
    #[arg(long)]
    json: Option<PathBuf>,

    /// Source name to label the report with. Defaults to the wasm file stem.
    #[arg(long)]
    name: Option<String>,

    /// Path to the source's res/source.json, for its declared listings.
    #[arg(long)]
    source_json: Option<PathBuf>,
}

fn main() -> Result<ExitCode> {
    let args = Args::parse();
    let name = args.name.clone().unwrap_or_else(|| {
        args.wasm
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "source".into())
    });

    let listings = match &args.source_json {
        Some(path) => checks::listings_from_manifest(path)
            .with_context(|| format!("reading listings from {}", path.display()))?,
        None => Vec::new(),
    };
    let base_url = match &args.source_json {
        Some(path) => checks::base_url_from_manifest(path)?,
        None => None,
    };
    let opts = checks::Options {
        query: args.query,
        min_content_chars: args.min_content_chars,
        base_url,
        listings,
    };

    // A source that will not even instantiate is a failure report, not a crash:
    // the workflow still wants a result row for it.
    let report = match SourceRunner::load(&args.wasm) {
        Ok(mut runner) => checks::run(&mut runner, &name, &opts)?,
        Err(e) => checks::Report {
            source: name.clone(),
            checks: vec![checks::Check {
                name: "load",
                status: Status::Fail,
                detail: format!("{e:#}"),
            }],
            log: String::new(),
        },
    };

    print_report(&report);

    if let Some(path) = &args.json {
        let json = serde_json::to_string_pretty(&report)?;
        std::fs::write(path, json).with_context(|| format!("writing {}", path.display()))?;
    }

    if report.blocked() {
        println!(
            "\nnote: this network is shielded from the site, so blocked checks are \
             inconclusive rather than failures."
        );
    }
    Ok(if report.failed() { ExitCode::FAILURE } else { ExitCode::SUCCESS })
}

fn print_report(report: &checks::Report) {
    println!("{}", report.source);
    for check in &report.checks {
        let mark = match check.status {
            Status::Pass => "PASS",
            Status::Warn => "WARN",
            Status::Fail => "FAIL",
            Status::Blocked => "BLOK",
            Status::Skip => "SKIP",
        };
        println!("  {mark:<4} {:<9} {}", check.name, check.detail);
    }
    if !report.log.is_empty() {
        println!("\n--- source log ---\n{}", report.log);
    }
}
