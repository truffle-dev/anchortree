//! Phase 3.5b Tier 2 widen: the first RETRIEVE score against the external
//! WebArena-Verified evaluator (DECISIONS D45).
//!
//! Where [`webarena_capture`](./webarena_capture.rs) proves the NAVIGATE contract
//! (reach a URL, emit `AgentResponse::completed(Navigate)`), this example proves
//! the typed-data extraction path D44 deferred: log into an authenticated site,
//! navigate to a page whose rendered DOM carries a single answer, read that
//! answer out of the live DOM, and emit
//! `AgentResponse::retrieved(<parsed value>)` — the contract a "how many ..."
//! task is scored against by the WebArena-Verified `AgentResponseEvaluator`.
//!
//! The flow is deliberately site-agnostic. Three optional JS hooks drive it, so
//! the same example serves any login-then-read RETRIEVE task; only the env vars
//! change. `scripts/run-once-retrieve.sh` wires it to shopping_admin task 11
//! ("Get the total number of reviews ... that mention term 'disappointed'",
//! expected `retrieved_data: [6]`).
//!
//! ## The honest mechanism
//!
//! anchortree drives the *authenticated admin session* and reads the count the
//! site itself renders. It does not fabricate the number, query the database, or
//! assert its own answer: it navigates to the filtered review grid and reads
//! `#reviewGrid-total-count`, which Magento server-renders as "6 records found".
//! If the store held a different number, anchortree would report that number and
//! the task would score 0. The evaluator agreeing is the upstream authority's
//! verdict on a real read, not our own.
//!
//! ## Env contract
//!
//! - `ANCHORTREE_CDP_WS` / `ANCHORTREE_CDP_HTTP` — the live Chrome endpoint (as
//!   in `webarena_capture`).
//! - `ANCHORTREE_CAPTURE_URL` — the page to read the answer from (post-login).
//! - `ANCHORTREE_CAPTURE_OUT` — output dir for `agent_response.json` + `network.har`.
//! - `ANCHORTREE_LOGIN_URL` (optional) — if set, navigated first, then
//!   `ANCHORTREE_LOGIN_JS` is evaluated to fill + submit the login form.
//! - `ANCHORTREE_LOGIN_JS` (optional) — JS run on the login page to authenticate.
//! - `ANCHORTREE_READ_JS` — JS evaluated on the capture page; its result is the
//!   raw answer string (defaults to reading `document.title`).
//! - `ANCHORTREE_RETRIEVE_NUMBER` — if `1`, the read string is parsed as an
//!   integer and emitted as a JSON number (so a scalar `6` normalises to the
//!   `[6]` the evaluator expects); otherwise the raw string is emitted.

use std::error::Error;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use anchortree_cdp::{AgentResponse, NetworkCapture, connect, write_task_output};

/// Parse a count out of the raw DOM read. The grid renders the total as bare
/// digits possibly padded with whitespace ("  6  "); pull the first integer run
/// so a stray "records found" suffix or surrounding whitespace cannot corrupt
/// the number. Returns the parsed integer as a JSON number on success.
fn parse_retrieved_number(raw: &str) -> Result<serde_json::Value, String> {
    let digits: String = raw
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if digits.is_empty() {
        return Err(format!("no integer found in DOM read {raw:?}"));
    }
    let n: i64 = digits
        .parse()
        .map_err(|e| format!("could not parse {digits:?} as i64: {e}"))?;
    Ok(serde_json::json!(n))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let ws_url = resolve_ws_url()?;
    let target = std::env::var("ANCHORTREE_CAPTURE_URL")
        .map_err(|_| "set ANCHORTREE_CAPTURE_URL to the page to read the answer from")?;

    println!("connecting to {ws_url}");
    let session = connect(ws_url).await?;
    let page = session.observer.page();

    // Start the capture before any navigation so the whole authenticated session
    // (login POST, redirect, grid document) lands in the HAR. The recording is
    // the evidence the read happened against a real served page.
    let capture = NetworkCapture::start_with_bodies(page).await?;

    // Optional login: navigate to the login URL, run the login JS (fills + submits
    // the form), and wait for the post-login navigation to settle.
    if let Ok(login_url) = std::env::var("ANCHORTREE_LOGIN_URL") {
        if !login_url.is_empty() {
            println!("logging in via {login_url}");
            page.goto(&login_url).await?;
            page.wait_for_navigation().await?;
            if let Ok(login_js) = std::env::var("ANCHORTREE_LOGIN_JS") {
                if !login_js.is_empty() {
                    page.evaluate(login_js.as_str()).await?;
                    page.wait_for_navigation().await?;
                    // Settle the dashboard redirect before navigating onward.
                    tokio::time::sleep(Duration::from_millis(400)).await;
                }
            }
        }
    }

    // Navigate to the answer page and read the answer out of the live DOM.
    println!("navigating to {target}");
    page.goto(&target).await?;
    page.wait_for_navigation().await?;
    tokio::time::sleep(Duration::from_millis(400)).await;

    let read_js =
        std::env::var("ANCHORTREE_READ_JS").unwrap_or_else(|_| "document.title".to_string());
    let raw: String = page
        .evaluate(read_js.as_str())
        .await?
        .into_value()
        .unwrap_or_default();
    println!("read answer = {raw:?}");

    // Close the capture: stop the pump, drain buffered events, build the HAR.
    let har = capture.finish().await?;
    let entry_count = har.log.entries.len();
    println!("captured {entry_count} HAR entries");
    assert!(
        entry_count >= 1,
        "the session should have produced at least one HAR entry; got {entry_count}"
    );

    // Emit the RETRIEVE contract. A "how many ..." task expects a single number;
    // ANCHORTREE_RETRIEVE_NUMBER=1 parses the read into a JSON number so the
    // evaluator's scalar->tuple normalisation matches the expected `[6]`.
    let as_number = std::env::var("ANCHORTREE_RETRIEVE_NUMBER")
        .map(|v| v == "1")
        .unwrap_or(false);
    let data = if as_number {
        parse_retrieved_number(&raw).map_err(|e| -> Box<dyn Error> { e.into() })?
    } else {
        serde_json::json!(raw)
    };
    println!("emitting retrieved_data = {data}");
    let response = AgentResponse::retrieved(data);

    let out_dir = std::env::var_os("ANCHORTREE_CAPTURE_OUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("anchortree-retrieve-out"));
    write_task_output(&out_dir, &response, &har)?;
    println!(
        "wrote {} and {}",
        out_dir.join("agent_response.json").display(),
        out_dir.join("network.har").display()
    );

    // The written HAR must round-trip back to a valid 1.2 log with entries.
    let har_back: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out_dir.join("network.har"))?)?;
    assert_eq!(har_back["log"]["version"], "1.2");
    assert!(
        har_back["log"]["entries"]
            .as_array()
            .is_some_and(|a| !a.is_empty()),
        "the written network.har must carry entries"
    );

    println!("\nOK: authenticated RETRIEVE captured and the agent contract output is written.");
    Ok(())
}

/// Resolve a `ws://` CDP URL from the environment.
///
/// `ANCHORTREE_CDP_WS` wins if set. Otherwise `ANCHORTREE_CDP_HTTP` is treated
/// as a Chrome HTTP endpoint and queried for its `webSocketDebuggerUrl`.
fn resolve_ws_url() -> Result<String, Box<dyn Error>> {
    if let Ok(ws) = std::env::var("ANCHORTREE_CDP_WS") {
        if !ws.is_empty() {
            return Ok(ws);
        }
    }
    let http = std::env::var("ANCHORTREE_CDP_HTTP").map_err(|_| {
        "set ANCHORTREE_CDP_WS=ws://<ip>:9222/devtools/browser/<id> or \
         ANCHORTREE_CDP_HTTP=http://<container-ip>:9222"
    })?;
    fetch_ws_debugger_url(&http)
}

/// Issue a minimal blocking `GET /json/version` and pull out
/// `webSocketDebuggerUrl`. Dependency-free: a single plain-text request so the
/// demo stays inside the `ws://`-only transport envelope.
fn fetch_ws_debugger_url(http_endpoint: &str) -> Result<String, Box<dyn Error>> {
    let host_port = http_endpoint
        .strip_prefix("http://")
        .ok_or("ANCHORTREE_CDP_HTTP must start with http://")?
        .trim_end_matches('/');

    let mut stream = TcpStream::connect(host_port)?;
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let request = format!(
        "GET /json/version HTTP/1.1\r\nHost: {host_port}\r\nAccept: application/json\r\n\r\n"
    );
    stream.write_all(request.as_bytes())?;

    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end = loop {
        if let Some(pos) = find_subslice(&buf, b"\r\n\r\n") {
            break pos + 4;
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Err("CDP endpoint closed before sending full headers".into());
        }
        buf.extend_from_slice(&chunk[..n]);
    };

    let headers = String::from_utf8_lossy(&buf[..header_end]);
    let content_length = headers
        .lines()
        .find_map(|line| {
            let (k, v) = line.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case("content-length")
                .then(|| v.trim().parse::<usize>().ok())
                .flatten()
        })
        .ok_or("CDP /json/version response had no Content-Length")?;

    while buf.len() < header_end + content_length {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = &buf[header_end..(header_end + content_length).min(buf.len())];
    let json: serde_json::Value = serde_json::from_slice(body)?;
    let ws = json
        .get("webSocketDebuggerUrl")
        .and_then(|v| v.as_str())
        .ok_or("CDP /json/version response had no webSocketDebuggerUrl")?;
    Ok(ws.to_string())
}

/// Index of the first occurrence of `needle` in `haystack`, if any.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::parse_retrieved_number;

    #[test]
    fn parses_padded_count() {
        // The grid renders the total as bare digits padded with whitespace.
        assert_eq!(
            parse_retrieved_number("  6  ").unwrap(),
            serde_json::json!(6)
        );
    }

    #[test]
    fn parses_count_with_suffix() {
        // A "6 records found" read still yields the leading integer.
        assert_eq!(
            parse_retrieved_number("6 records found").unwrap(),
            serde_json::json!(6)
        );
    }

    #[test]
    fn parses_multi_digit() {
        assert_eq!(
            parse_retrieved_number("351").unwrap(),
            serde_json::json!(351)
        );
    }

    #[test]
    fn emits_json_number_not_string() {
        // The evaluator's results_schema is {items: {type: number}}; a string
        // "6" would fail validation. Guard that we emit a JSON number.
        let v = parse_retrieved_number("6").unwrap();
        assert!(
            v.is_number(),
            "retrieved value must be a JSON number, got {v}"
        );
        assert!(!v.is_string());
    }

    #[test]
    fn errors_on_no_digits() {
        assert!(parse_retrieved_number("records found").is_err());
        assert!(parse_retrieved_number("").is_err());
    }
}
