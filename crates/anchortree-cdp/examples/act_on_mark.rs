//! Phase 2.2a end-to-end demo: act on a transient **mark**, not a durable eid.
//!
//! Most controls earn a durable [`Eid`] the agent can remember across turns. But
//! some elements `fuse` keeps have no anchor the engine can promise to keep — an
//! icon-only button with no `id`, no `aria-label`, no text. Minting an eid for
//! one of those would be a lie: the next observation would churn it into a
//! different eid. For those the engine emits a single-turn [`Mark`] instead (see
//! `anchortree_core::observation` and decision D13). This is the textual
//! "set-of-marks": a numbered handle list the agent acts on *this turn* and must
//! not remember.
//!
//! The script:
//!   1. builds a toolbar of two icon-only `<button>`s (an SVG child, no `id`, no
//!      `aria-label`, no text) plus two `role="status"` lines that DO earn eids;
//!   2. observes once and confirms the two icon buttons came back as `marks`
//!      (positional `m0`, `m1`), while the status lines went into the durable
//!      `diff`;
//!   3. dispatches a trusted [`Action::Click`] against mark `m0` via the
//!      observer's `act_mark` — resolved straight from the observation's
//!      captured `backendNodeId`, not through the identity map; and
//!   4. reads the live DOM back to prove the click landed on the right button
//!      and arrived with `isTrusted: true` (a page-script `.click()` could not).
//!
//! ## Running it
//!
//! Same target as the other examples — a headless Chrome on the phantom network,
//! addressed by container IP:
//!
//! ```text
//! docker run -d --name anchortree-chrome --network phantom_phantom-net \
//!     chromedp/headless-shell:latest
//! ANCHORTREE_CDP_HTTP=http://<container-ip>:9222 \
//!     cargo run -p anchortree-cdp --example act_on_mark
//! ```
//!
//! or pass `ANCHORTREE_CDP_WS=ws://<ip>:9222/devtools/browser/<id>` straight
//! through.

use std::error::Error;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use anchortree_cdp::{Action, connect};
use anchortree_core::{IdentityMap, ObservationSource as _};

/// A toolbar with two icon-only buttons and two live status lines.
///
/// Each button's only child is a decorative `<svg>` with no `<title>`, so its
/// computed accessible name is empty; it carries no `id` and no `aria-label`.
/// That is exactly the "kept but unanchorable" shape: `fuse` surfaces it (it is
/// an interactive button) but the rebind ladder has nothing durable to hold, so
/// the engine emits a mark rather than minting a churn-prone eid. The two
/// `role="status"` paragraphs DO have accessible names, so they earn eids and
/// land in the durable diff — the contrast the demo is built to show. Each
/// button records `event.isTrusted` into a distinct global so we can prove which
/// one was clicked and that the dispatch was trusted.
const JS_TOOLBAR: &str = r#"document.body.innerHTML = `<main><h1>Toolbar</h1><button onclick="window.__clicked0 = event.isTrusted; const c = document.getElementById('count'); c.textContent = String((parseInt(c.textContent) || 0) + 1);"><svg width="16" height="16" aria-hidden="true"><rect width="16" height="16"></rect></svg></button><button onclick="window.__clicked1 = event.isTrusted; document.getElementById('state').textContent = 'archived';"><svg width="16" height="16" aria-hidden="true"><circle cx="8" cy="8" r="8"></circle></svg></button><p id="count" role="status" aria-label="Click count">0</p><p id="state" role="status" aria-label="State">live</p></main>`; window.__clicked0 = null; window.__clicked1 = null; true"#;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let ws_url = resolve_ws_url()?;
    println!("connecting to {ws_url}");
    let mut session = connect(ws_url).await?;

    let mut map = IdentityMap::new();

    // --- Observe the toolbar. ---
    session.observer.page().evaluate(JS_TOOLBAR).await?;
    let obs = map.observe(session.observer.observe().await?);

    // The durable side: the two status lines earned eids.
    let durable: Vec<String> = obs.diff.added.iter().map(|e| e.0.clone()).collect();
    println!("durable eids (diff.added): {durable:?}");
    assert!(
        durable.iter().any(|e| e.contains("count")),
        "the named status lines should earn durable eids, got {durable:?}"
    );

    // The transient side: the two icon buttons are marks, in document order.
    println!("marks this turn:");
    for m in &obs.marks {
        println!(
            "  {} role={:?} label={:?} bbox=({:.0},{:.0} {:.0}x{:.0})",
            m.id(),
            m.role,
            m.label_snippet,
            m.geometry.x,
            m.geometry.y,
            m.geometry.w,
            m.geometry.h
        );
    }
    assert_eq!(
        obs.marks.len(),
        2,
        "the two unanchorable icon buttons should each surface as a mark, got {:?}",
        obs.marks
    );
    assert_eq!(obs.marks[0].id(), "m0");
    assert_eq!(obs.marks[1].id(), "m1");

    // --- Act on mark m0: a trusted click resolved straight from the
    //     observation, no eid involved. ---
    session.observer.act_mark(&obs, 0, Action::Click).await?;

    let count: String = session
        .observer
        .page()
        .evaluate("document.getElementById('count').textContent")
        .await?
        .into_value()?;
    let clicked0: bool = session
        .observer
        .page()
        .evaluate("window.__clicked0 === true")
        .await?
        .into_value()?;
    // `window.__clicked1` stays `null` until the second button fires; read it as
    // a normalized boolean so an untouched (null) global comes back as `false`.
    let clicked1: bool = session
        .observer
        .page()
        .evaluate("window.__clicked1 === true")
        .await?
        .into_value()?;

    println!(
        "\nafter act_mark(m0, Click): count={count:?}, __clicked0={clicked0}, __clicked1={clicked1}"
    );
    assert_eq!(count, "1", "clicking m0 should have bumped the count to 1");
    assert!(
        clicked0,
        "the click must arrive on the first button as a trusted event (isTrusted: true)"
    );
    assert!(
        !clicked1,
        "only m0 should have fired; the second button must be untouched"
    );

    // --- Acting on an out-of-range index fails loudly, the single-turn
    //     contract: marks are not a durable handle space. ---
    let stale = session.observer.act_mark(&obs, 99, Action::Click).await;
    assert!(
        stale.is_err(),
        "an index with no mark in this observation must error, not silently no-op"
    );
    println!("act_mark(m99) correctly refused: {}", stale.unwrap_err());

    println!("\nOK: a trusted click landed on an unanchorable icon button via a single-turn mark.");
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
/// `webSocketDebuggerUrl`. Dependency-free (no TLS, no HTTP crate) to stay
/// inside the `ws://`-only transport envelope (see `DECISIONS.md` D8/D10).
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
