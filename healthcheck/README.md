# buny-healthcheck

Checks whether the sources in this repo still work against their live sites.

It drives each built source's wasm through search, novel details, chapter list,
chapter text, and every declared listing, then reports per-source results. The
sources are unmodified: it calls the same exported entrypoints the app calls
(`get_search_novel_list`, `get_novel_update`, `get_chapter_content_list`,
`get_novel_list`, `get_home`), reusing `buny-test-runner` as a library.

It asserts structural health rather than per-site fixtures, so it catches
selector rot, domain moves and dead sites, but not subtler regressions such as
chapter ordering. An `Ok` result that is empty counts as a failure, since that is
what a scraper returns once its selectors stop matching.

## Usage

```sh
cd sources/en.royalroad && cargo build --release        # produces the wasm
cd ../../healthcheck && cargo build --release

./target/release/buny-healthcheck \
  ../sources/en.royalroad/target/wasm32-unknown-unknown/release/royalroad.wasm \
  --name en.royalroad \
  --source-json ../sources/en.royalroad/res/source.json \
  --query king
```

Exit status is non-zero if any check fails. `--json <path>` writes a
machine-readable report.

Static listings come from `res/source.json` rather than a `get_listings` export,
so `--source-json` is what enables the listing checks.

## Check outcomes

| Status | Meaning |
| --- | --- |
| `PASS` | Worked and returned plausible data. |
| `WARN` | Worked, but something looks thin (e.g. details missing authors). Does not fail the run. |
| `FAIL` | Errored, trapped, or returned an empty result where content was expected. |
| `BLOCKED` | The site is alive but shielded from this network, so the result is inconclusive. Does not fail the run. |
| `SKIP` | Not implemented by this source, or a prerequisite check failed. |

Each run starts with a reachability probe of the manifest's base URL, which
determines how later failures are read. A dead or moved domain (DNS failure,
refused connection, 4xx) is a `FAIL`; a bot wall, or any response carrying
`cf-mitigated`, downgrades the run's failures to `BLOCKED`. The probe goes
through the fetch gateway when one is configured.

## The fetch gateway

Sources behind Cloudflare reject every ordinary HTTP client from a datacenter IP
whatever headers it sends, because the TLS ClientHello and HTTP/2 settings are
fingerprinted alongside them. Measured from a GitHub runner:

| | scribblehub | chikari | fenrirealm | novelfull | novelfire |
| --- | --- | --- | --- | --- | --- |
| host UA (`Buny/1 CFNetwork/...`) | 403 | 403 | 403 | 403 | 403 |
| Chrome UA | 403 | 403 | 403 | 403 | 403 |
| Chrome UA + full browser headers | 403 | 403 | 403 | 403 | 403 |
| browser fingerprint emulation | 200 | 200 | 200 | 200 | 200 |

The same five return 200 from a residential connection with the plain host UA.

`fetcher.py` performs the request with a browser fingerprint and returns the
response over loopback. `src/net.rs` replaces `net.send`/`net.send_all` to route
through it; everything downstream (`html`, `get_status_code`, `read_data`) reads
the stored `NetResponse` unchanged. It is a separate process because the checker
is pinned to Rust 1.88 while the Rust impersonation crates require 1.98.

The override only installs itself when `BUNY_FETCH_GATEWAY` is set.

A stale emulation profile fails where a current one succeeds: from a GitHub
runner, `chrome131` gets 403 on the same sources where `chrome` gets 200.

## Environment

| Variable | Effect |
| --- | --- |
| `BUNY_FETCH_GATEWAY` | Gateway base URL, e.g. `http://127.0.0.1:8099`. Unset leaves the host's own networking in place. |
| `BUNY_FETCH_PORT` | Port for `fetcher.py` (default 8099). |
| `BUNY_IMPERSONATE` | curl_cffi profile (default `chrome`). |
| `DISCORD_WEBHOOK` | Discord webhook URL for notifications. Unset skips the step. |

## Pins

`rust-toolchain.toml` pins 1.88.0, scoped to this directory: wasmer 5.x
references `__rust_probestack`, removed from compiler-builtins in Rust 1.89. The
pin can go once buny-rs moves to wasmer 6.

`runner.rs` redefines `html.parse_fragment` to parse a wrapped document.
buny-rs's `text_with_newlines()` parses a fragment then selects `body`, which
scraper's `Html::parse_fragment` does not produce and the app's SwiftSoup host
does. Without the override the helper returns `None` and sources that unwrap it
panic. It can go if buny-test-runner changes.

`Cargo.toml` pins buny-rs by commit, as this crate uses internals that are not a
published API: `GlobalStore::store_encoded`, the `[len][cap][postcard]` result
header, and `imports::generate_imports`.

## Workflow

`.github/workflows/healthcheck.yaml` runs daily (`17 3 * * *`) and on demand,
taking an optional single source and search term. It uses the built-in
`GITHUB_TOKEN` and no other credentials.

With `DISCORD_WEBHOOK` set as a repository secret, results are posted to that
channel. Scheduled runs post only when something fails; manual runs always post.

Scheduled workflows in a public repo are disabled automatically after 60 days
without repository activity, and are disabled by default in forks.
