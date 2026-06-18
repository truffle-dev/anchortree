//! Phase 3.3b live proof: record a real `network.har` from a live navigation and
//! emit the WebArena-Verified agent contract output for one task.
//!
//! This is the "alive" proof for the runner layer. It connects to a live
//! headless Chrome over a plain `ws://` CDP endpoint, starts a
//! [`NetworkCapture`](anchortree_cdp::NetworkCapture) on the page, navigates to a
//! target URL (which issues the document request plus its sub-resources), reads a
//! tiny answer out of the loaded DOM, then closes the capture and writes
//! `agent_response.json` + `network.har` into an output directory. The headline:
//! the HAR carries real entries assembled from live CDP `Network.*` events, and
//! at least one entry matches the navigated document — proof the pump wired the
//! browser-free recorder to a live event stream end to end.
//!
//! `ANCHORTREE_TASK_TYPE` selects the agent contract written: `RETRIEVE` (the
//! default) emits the read-back `document.title` as the answer; `NAVIGATE` emits
//! `AgentResponse::completed(Navigate)` (status `SUCCESS`, no data), which is the
//! response a reach-a-URL task is scored against by the WebArena-Verified
//! `AgentResponseEvaluator`. `MUTATE` additionally runs `ANCHORTREE_MUTATE_JS` on
//! the loaded page (fill + native full-form submit) and waits for the resulting
//! save POST plus its 302 redirect to land in the HAR, then emits
//! `AgentResponse::completed(Mutate)`; that save request is what the
//! `NetworkEventEvaluator` scores its `post_data` subset against.
//!
//! Two optional env vars let the capture reach a target that lives behind a
//! login (e.g. a Magento admin content page): if `ANCHORTREE_LOGIN_URL` is set,
//! it is navigated first and `ANCHORTREE_LOGIN_JS` (form-fill + submit) is
//! evaluated on it before the real `ANCHORTREE_CAPTURE_URL` navigation. The whole
//! authenticated session lands in the one HAR. When neither is set the flow is
//! unchanged: a single public navigation.
//!
//! ## Running it
//!
//! Bring up a headless Chrome and a static file server on the phantom network:
//!
//! ```text
//! docker run -d --name anchortree-chrome --network phantom_phantom-net \
//!     chromedp/headless-shell:latest
//! docker run -d --name anchortree-www --network phantom_phantom-net \
//!     -v "$PWD/site:/site:ro" -w /site python:3.12-slim \
//!     python -m http.server 8080
//! ```
//!
//! Then point the demo at Chrome and the page to capture:
//!
//! ```text
//! ANCHORTREE_CDP_HTTP=http://<chrome-ip>:9222 \
//!     ANCHORTREE_CAPTURE_URL=http://<www-ip>:8080/index.html \
//!     cargo run -p anchortree-cdp --example webarena_capture
//! ```
//!
//! Connect by container **IP**, not hostname: Chrome's CDP HTTP endpoint rejects
//! a `Host` header it does not recognise, and the `/json/version`
//! `webSocketDebuggerUrl` headless-shell returns is already IP-based.

use std::error::Error;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use anchortree_cdp::{AgentResponse, NetworkCapture, TaskType, connect, write_task_output};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let ws_url = resolve_ws_url()?;
    let target = std::env::var("ANCHORTREE_CAPTURE_URL")
        .map_err(|_| "set ANCHORTREE_CAPTURE_URL to an http(s) page to capture")?;

    println!("connecting to {ws_url}");
    let session = connect(ws_url).await?;
    let page = session.observer.page();

    // Start the capture BEFORE navigating so the document request itself is
    // recorded. The pump runs on a background task from here until `finish`.
    // `start_with_bodies` inlines each response body into the HAR so the
    // recording is self-contained and can be replayed offline (the input the
    // `webarena_replay` example fulfills with no live origin).
    let capture = NetworkCapture::start_with_bodies(page).await?;

    // Optional login: navigate to the login URL, run the login JS (fills + submits
    // the form), and wait for the post-login navigation to settle. This lets the
    // capture reach an authenticated content page (e.g. a Magento admin URL) while
    // keeping the whole authenticated session in the one HAR. Skipped entirely when
    // ANCHORTREE_LOGIN_URL is unset, so public navigations are unchanged.
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

    // Drive the task: navigate, settle.
    println!("navigating to {target}");
    page.goto(&target).await?;
    page.wait_for_navigation().await?;
    // Give late sub-resources a beat to finish so their loadingFinished events
    // land before we stop the capture.
    tokio::time::sleep(Duration::from_millis(400)).await;

    // RETRIEVE (default) reports the read-back title; NAVIGATE reports a
    // data-less SUCCESS; MUTATE runs a form-fill+submit hook on the loaded page
    // and reports a data-less SUCCESS for the save it triggered.
    let task_type = std::env::var("ANCHORTREE_TASK_TYPE").unwrap_or_default();

    // MUTATE: fill and submit the save form (`ANCHORTREE_MUTATE_JS`); the hook
    // does a native full-form POST (every field, including `form_key`), which is
    // what the WebArena-Verified `NetworkEventEvaluator` scores its `post_data`
    // subset against. An AJAX save would return 200 + JSON, not the 302 the task
    // contract requires, and would not be scored. We then read NO DOM answer: the
    // submit replaces the document, so any evaluate against the pre-submit
    // execution context races its teardown ("Cannot find context with specified
    // id"). The capture is network-event driven, so we just let the save POST and
    // its 302 redirect (plus the reloaded edit page) drain into the HAR.
    let title = if task_type.eq_ignore_ascii_case("mutate") {
        let mutate_js = std::env::var("ANCHORTREE_MUTATE_JS").map_err(
            |_| "MUTATE task requires ANCHORTREE_MUTATE_JS to fill and submit the save form",
        )?;
        if mutate_js.is_empty() {
            return Err(
                "ANCHORTREE_MUTATE_JS is empty; it must fill and submit the save form".into(),
            );
        }
        // The admin form fields render asynchronously (Magento UI components +
        // PageBuilder), so poll the hook until it reports a submit. The hook
        // returns a `waiting:*` sentinel while the title field / save control are
        // not yet present, and a string starting with `submitted` once it has
        // fired the save. We stop the instant it submits (any later evaluate would
        // race the navigation teardown), or give up after the readiness budget.
        println!("running mutate hook (polling for form readiness)");
        let mut last = String::new();
        let mut submitted = false;
        for attempt in 1..=80u32 {
            last = page
                .evaluate(mutate_js.as_str())
                .await?
                .into_value()
                .unwrap_or_default();
            if last.starts_with("submitted") {
                println!("mutate hook submitted on attempt {attempt}: {last:?}");
                submitted = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        if !submitted {
            return Err(format!(
                "mutate hook never reported a submit within the readiness budget; \
                 last status {last:?}"
            )
            .into());
        }
        // Let the save POST + its 302 redirect (and the reloaded edit page) drain
        // into the capture. No wait_for_navigation / DOM read: the old execution
        // context is gone the moment the save replaces the document. The window is
        // generous because the admin save runs client-side validation and (on a
        // PageBuilder page) serializes the editor content asynchronously before it
        // POSTs, so the request can fire a couple of seconds after the click.
        tokio::time::sleep(Duration::from_millis(7000)).await;
        String::new()
    } else {
        let title: String = page
            .evaluate("document.title")
            .await?
            .into_value()
            .unwrap_or_default();
        println!("read document.title = {title:?}");
        title
    };

    // Close the capture: stop the pump, drain buffered events, build the HAR.
    let har = capture.finish().await?;
    let entry_count = har.log.entries.len();
    println!("captured {entry_count} HAR entries");

    assert!(
        entry_count >= 1,
        "the navigation should have produced at least the document request as a \
         HAR entry; got {entry_count}. Is ANCHORTREE_CAPTURE_URL a real http page?"
    );
    // At least one entry must be a real request URL (the live pump assembled it
    // from EventRequestWillBeSent), not an empty placeholder.
    let any_real_url = har
        .log
        .entries
        .iter()
        .any(|e| e.request.url.starts_with("http"));
    assert!(
        any_real_url,
        "no HAR entry carried an http(s) request URL; the live event pump did not \
         capture the navigation"
    );

    // Emit the WebArena-Verified task output. `ANCHORTREE_CAPTURE_OUT` lets a
    // caller (e.g. scripts/run-once-m1.sh) pin where the HAR lands so a later
    // replay reads the same path; otherwise it defaults under the temp dir.
    let out_dir = std::env::var_os("ANCHORTREE_CAPTURE_OUT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("anchortree-capture-out"));
    // RETRIEVE (default) reports the read-back title as the answer; NAVIGATE and
    // MUTATE report a data-less SUCCESS, the contract those task types are scored
    // on (MUTATE is additionally scored on the save POST captured in the HAR).
    let response = if task_type.eq_ignore_ascii_case("navigate") {
        AgentResponse::completed(TaskType::Navigate)
    } else if task_type.eq_ignore_ascii_case("mutate") {
        AgentResponse::completed(TaskType::Mutate)
    } else {
        AgentResponse::retrieved(serde_json::json!(title))
    };
    write_task_output(&out_dir, &response, &har)?;
    println!(
        "wrote {} and {}",
        out_dir.join("agent_response.json").display(),
        out_dir.join("network.har").display()
    );

    // The written HAR must round-trip back to a valid 1.2 log.
    let har_back: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out_dir.join("network.har"))?)?;
    assert_eq!(har_back["log"]["version"], "1.2");
    assert!(
        har_back["log"]["entries"]
            .as_array()
            .is_some_and(|a| !a.is_empty()),
        "the written network.har must carry entries"
    );

    println!("\nOK: live network.har captured and the agent contract output is written.");
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
