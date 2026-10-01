//! A `net.send` replacement that routes through the fingerprint-impersonating
//! fetch gateway (`fetcher.py`) when one is configured.
//!
//! Cloudflare-fronted sources 403 every ordinary HTTP client from a datacenter
//! IP, whatever headers it sends, because the TLS ClientHello and HTTP/2
//! settings are fingerprinted too. The gateway performs the real request with a
//! browser fingerprint and hands back the response; see fetcher.py.
//!
//! With `BUNY_FETCH_GATEWAY` unset the host's own networking is left in place,
//! which is all that's needed from a residential connection.
//!
//! Only `send`/`send_all` are replaced. `html`, `get_status_code`, `read_data`
//! and friends read the `NetResponse` we store, so the rest of the host is
//! untouched.

use base64::Engine;
use buny_test_runner::{
    libs::{HttpMethod, NetResponse, StoreItem},
    Ptr, Rid, WasmEnv,
};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use wasmer::FunctionEnvMut;

// Mirrors buny-test-runner's net result codes; the wasm side matches on these.
const SUCCESS: i32 = 0;
const INVALID_DESCRIPTOR: i32 = -1;
const INVALID_URL: i32 = -4;
const REQUEST_ERROR: i32 = -10;
const FAILED_MEMORY_WRITE: i32 = -11;

/// Set when any response came back carrying `cf-mitigated`.
///
/// Cloudflare sets that header only on requests it actually mitigated, so it is
/// an unambiguous "we blocked this" marker: failures downstream of one say
/// nothing about whether the source still parses the site correctly.
static SAW_MITIGATION: AtomicBool = AtomicBool::new(false);

pub fn saw_mitigation() -> bool {
    SAW_MITIGATION.load(Ordering::Relaxed)
}

/// Gateway base URL, e.g. `http://127.0.0.1:8099`.
pub fn gateway() -> Option<&'static str> {
    static GATEWAY: OnceLock<Option<String>> = OnceLock::new();
    GATEWAY
        .get_or_init(|| std::env::var("BUNY_FETCH_GATEWAY").ok().filter(|v| !v.is_empty()))
        .as_deref()
}

fn client() -> &'static reqwest::blocking::Client {
    static CLIENT: OnceLock<reqwest::blocking::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            // Generous: the gateway owns the real per-request timeout.
            .timeout(std::time::Duration::from_secs(180))
            .build()
            .expect("building the gateway client")
    })
}

#[derive(Serialize)]
struct FetchRequest {
    url: String,
    method: &'static str,
    headers: std::collections::HashMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    body_b64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timeout: Option<f64>,
}

#[derive(Deserialize)]
struct FetchResponse {
    status: u16,
    url: String,
    headers: std::collections::HashMap<String, String>,
    body_b64: String,
}

fn method_name(method: &HttpMethod) -> &'static str {
    match method {
        HttpMethod::Get => "GET",
        HttpMethod::Post => "POST",
        HttpMethod::Put => "PUT",
        HttpMethod::Head => "HEAD",
        HttpMethod::Delete => "DELETE",
        HttpMethod::Patch => "PATCH",
        HttpMethod::Options => "OPTIONS",
        HttpMethod::Connect => "CONNECT",
        HttpMethod::Trace => "TRACE",
    }
}

fn common_send(env: &mut FunctionEnvMut<WasmEnv>, rid: Rid) -> i32 {
    // The overrides are only installed when a gateway is configured, so this
    // cannot be reached without one.
    let Some(gateway) = gateway() else {
        return REQUEST_ERROR;
    };

    let Some(request) = env
        .data_mut()
        .store
        .get_mut(rid)
        .and_then(|item| item.as_request())
    else {
        return INVALID_DESCRIPTOR;
    };
    let Some(url) = request.url.as_ref().map(|u| u.to_string()) else {
        return INVALID_URL;
    };

    // The impersonation profile supplies its own User-Agent and header order: a
    // UA that disagrees with the TLS fingerprint is itself a block signal.
    let mut headers = std::collections::HashMap::new();
    for (name, value) in request.headers.iter() {
        if name == reqwest::header::USER_AGENT {
            continue;
        }
        if let Ok(value) = value.to_str() {
            headers.insert(name.as_str().to_string(), value.to_string());
        }
    }

    let payload = FetchRequest {
        url,
        method: method_name(&request.method),
        headers,
        body_b64: request
            .body
            .take()
            .map(|b| base64::engine::general_purpose::STANDARD.encode(b)),
        timeout: request.timeout.take(),
    };

    let response = client()
        .post(format!("{gateway}/fetch"))
        .json(&payload)
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.json::<FetchResponse>());

    let Ok(fetched) = response else {
        return REQUEST_ERROR;
    };
    let Ok(body) = base64::engine::general_purpose::STANDARD.decode(&fetched.body_b64) else {
        return REQUEST_ERROR;
    };
    let Ok(url) = url::Url::parse(&fetched.url) else {
        return INVALID_URL;
    };
    let Ok(status) = reqwest::StatusCode::from_u16(fetched.status) else {
        return REQUEST_ERROR;
    };
    if fetched
        .headers
        .keys()
        .any(|k| k.eq_ignore_ascii_case("cf-mitigated"))
    {
        SAW_MITIGATION.store(true, Ordering::Relaxed);
    }
    let mut header_map = HeaderMap::new();
    for (name, value) in fetched.headers.iter() {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value.as_str()),
        ) {
            header_map.insert(name, value);
        }
    }

    let Some(request) = env
        .data_mut()
        .store
        .get_mut(rid)
        .and_then(|item| item.as_request())
    else {
        return INVALID_DESCRIPTOR;
    };
    request.response = Some(NetResponse {
        url,
        status,
        headers: header_map,
        data: body,
    });
    SUCCESS
}

/// Fetches a URL through the gateway, for the reachability probe.
///
/// The probe must use the same path as the source's own requests, or it would
/// report a Cloudflare-fronted site as blocked while the gateway reaches it fine.
pub fn fetch_via_gateway(url: &str) -> Result<(u16, String), String> {
    let gateway = gateway().ok_or_else(|| "no gateway configured".to_string())?;
    let payload = FetchRequest {
        url: url.to_string(),
        method: "GET",
        headers: Default::default(),
        body_b64: None,
        timeout: Some(30.0),
    };
    let response = client()
        .post(format!("{gateway}/fetch"))
        .json(&payload)
        .send()
        .map_err(|e| format!("gateway unreachable: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("gateway reported failure ({})", response.status()));
    }
    let fetched: FetchResponse = response
        .json()
        .map_err(|e| format!("malformed gateway response: {e}"))?;
    let body = base64::engine::general_purpose::STANDARD
        .decode(&fetched.body_b64)
        .map_err(|e| format!("malformed body: {e}"))?;
    Ok((fetched.status, String::from_utf8_lossy(&body).to_string()))
}

pub fn send(mut env: FunctionEnvMut<WasmEnv>, rid: Rid) -> i32 {
    common_send(&mut env, rid)
}

pub fn send_all(mut env: FunctionEnvMut<WasmEnv>, rid_ptr: Ptr, len: u32) -> i32 {
    let Ok(rids) = env.data().read_values::<Rid>(&env, rid_ptr, len) else {
        return INVALID_DESCRIPTOR;
    };
    let mut results = Vec::new();
    let mut had_error = false;
    for rid in rids {
        let result = common_send(&mut env, rid);
        if result != SUCCESS {
            had_error = true;
        }
        results.push(result);
    }
    if env.data().write_values(&env, rid_ptr, results).is_err() {
        FAILED_MEMORY_WRITE
    } else if had_error {
        REQUEST_ERROR
    } else {
        SUCCESS
    }
}

#[allow(dead_code)]
fn _assert_store_item(_: &StoreItem) {}
