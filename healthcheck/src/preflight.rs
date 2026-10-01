//! Independent reachability probe, used to tell "broken" apart from "blocked".
//!
//! Sources that sit behind Cloudflare are unreachable from datacenter IPs: every
//! request gets a 403 JS-challenge interstitial regardless of headers. That is
//! not the source being broken, and reporting it as a failure trains everyone to
//! ignore the checker. A CF 403 does however prove the site is *alive*, so we can
//! still catch the most common real breakage — a dead or moved domain — by
//! probing the base URL directly and classifying the response.

use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum Reachability {
    /// Responded like a normal site; full checks are meaningful.
    Open,
    /// Alive but shielded (bot wall). Deeper failures are inconclusive.
    Blocked(String),
    /// Does not resolve / refuses connections / TLS broken: real breakage.
    Dead(String),
}

const BROWSER_MARKERS: [&str; 4] = [
    "just a moment",
    "challenge-platform",
    "enable javascript and cookies",
    "cf-browser-verification",
];

/// Probes a source's base URL, using the impersonating gateway when one is
/// configured so the probe sees exactly what the source's own requests see.
pub fn probe(base_url: &str, user_agent: &str) -> Reachability {
    if crate::net::gateway().is_some() {
        // One retry: a single transient fetch failure should not decide how the
        // whole run is interpreted.
        for attempt in 0..2 {
            match crate::net::fetch_via_gateway(base_url) {
                Ok((status, body)) => {
                    return classify(base_url, status, &body.to_lowercase())
                }
                Err(e) if attempt == 1 => return Reachability::Dead(format!("{base_url}: {e}")),
                Err(_) => std::thread::sleep(std::time::Duration::from_secs(3)),
            }
        }
    }

    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(user_agent)
        .build()
    {
        Ok(c) => c,
        Err(e) => return Reachability::Dead(format!("could not build client: {e}")),
    };

    let response = match client.get(base_url).send() {
        Ok(r) => r,
        Err(e) => {
            // Connect/DNS/TLS errors mean the site is genuinely unreachable.
            let kind = if e.is_timeout() {
                "timed out"
            } else if e.is_connect() {
                "connection failed"
            } else {
                "request failed"
            };
            return Reachability::Dead(format!("{base_url}: {kind} ({e})"));
        }
    };

    let status = response.status().as_u16();
    let body = response.text().unwrap_or_default().to_lowercase();
    classify(base_url, status, &body)
}

fn classify(base_url: &str, status: u16, lowercase_body: &str) -> Reachability {
    let challenged = BROWSER_MARKERS.iter().any(|m| lowercase_body.contains(m));

    // 403/503 with challenge markers is a bot wall, not a broken source.
    if matches!(status, 403 | 503 | 429) && challenged {
        return Reachability::Blocked(format!("{status} bot wall at {base_url}"));
    }
    if matches!(status, 403 | 429) {
        return Reachability::Blocked(format!("{status} at {base_url}"));
    }
    if (500..600).contains(&status) {
        return Reachability::Dead(format!("{base_url}: server error {status}"));
    }
    if (400..500).contains(&status) {
        return Reachability::Dead(format!("{base_url}: HTTP {status}"));
    }
    Reachability::Open
}
