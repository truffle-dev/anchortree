//! Phase 3.5b Tier 2: observe durable identity over a REAL WebArena-Verified
//! page reached entirely from a recorded HAR — no live origin, no fixture hooks.
//!
//! This is the general-purpose replay-and-observe rail. Where
//! [`webarena_replay`](../webarena_replay.rs) drives the bespoke `m1-site`
//! fixture (with its `__atRerender`/`__atReorder` hooks) to MEASURE a
//! head-to-head, this example makes no assumption about the page's contents: it
//! replays whatever self-contained HAR it is handed, observes the result once,
//! and mints durable [`Eid`](anchortree_core::Eid)s over a genuine, server-
//! rendered application page. That is exactly what is needed for the live
//! WebArena-Verified sites, whose pages carry no instrumentation of ours.
//!
//! The capture half ([`webarena_capture`](../webarena_capture.rs)) is pointed at
//! a booted WebArena-Verified site (see `scripts/run-once-webarena.sh`, which
//! pulls the smallest per-site image, runs it as a sibling on the phantom
//! network, and reaches it by container DNS). It banks a self-contained inline-
//! body `network.har` plus the `agent_response.json` the WebArena evaluator
//! scores from. This example then replays that HAR with the live site torn down:
//! every request is answered from the recording or honestly failed, the browser
//! never touches the network, and the observe loop proves anchortree mints
//! durable handles on a real application page reached with zero live origin.
//!
//! ## Running it
//!
//! ```text
//! ANCHORTREE_CDP_HTTP=http://127.0.0.1:9222 \
//!     ANCHORTREE_REPLAY_HAR=/tmp/wa-out/network.har \
//!     ANCHORTREE_REPLAY_URL=http://at-wa-map:8080/about \
//!     cargo run -p anchortree-cdp --example webarena_observe
//! ```
//!
//! `ANCHORTREE_REPLAY_URL` must match the document URL the HAR recorded (the
//! fulfiller keys on URL). Exit 0 means the page was reconstructed entirely from
//! the recording and at least one durable eid was minted over it.

use std::error::Error;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use anchortree_cdp::{ReplayFulfiller, ReplayHar, connect};
use anchortree_core::{IdentityMap, ObservationSource as _};
use chromiumoxide::cdp::browser_protocol::page::NavigateParams;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let ws_url = resolve_ws_url()?;
    let har_path = std::env::var("ANCHORTREE_REPLAY_HAR")
        .map_err(|_| "set ANCHORTREE_REPLAY_HAR to a self-contained inline-body network.har")?;
    let target = std::env::var("ANCHORTREE_REPLAY_URL")
        .map_err(|_| "set ANCHORTREE_REPLAY_URL to the document URL the HAR recorded")?;

    let har = ReplayHar::from_json(&std::fs::read_to_string(&har_path)?)?;
    println!("loaded replay HAR from {har_path}");

    println!("connecting to {ws_url}");
    let mut session = connect(ws_url).await?;
    let page = session.observer.page().clone();

    // Start the fulfiller BEFORE navigating: it answers the document request
    // itself from the HAR, so even the first byte comes from the recording.
    let fulfiller = ReplayFulfiller::start(&page, har).await?;

    println!("navigating to {target} (served entirely from the HAR)");
    // A real application page does not settle to network-idle the way a single
    // self-contained fixture does: sub-resources it never recorded (favicons,
    // late XHRs) are intercepted and honestly aborted, so the `load` lifecycle
    // never fires and `goto` (which waits for it) hangs. We issue the raw
    // `Page.navigate` instead: it returns the moment the navigation is committed,
    // not when the page settles. Then we give the replayed document a fixed beat
    // to parse and run its inline scripts before observing. The DOM the agent
    // reasons over is present at commit + parse; full network-idle is not a
    // precondition for minting durable identity over it.
    page.execute(NavigateParams::new(target.clone())).await?;
    tokio::time::sleep(Duration::from_millis(1200)).await;

    let stats = fulfiller.finish().await?;
    println!(
        "replay answered: {} fulfilled, {} failed, {} dispatch errors",
        stats.fulfilled, stats.failed, stats.errors
    );
    assert!(
        stats.fulfilled >= 1,
        "the navigation should have fulfilled at least the document request from \
         the HAR; got {} fulfilled. Does ANCHORTREE_REPLAY_URL match a recorded \
         entry?",
        stats.fulfilled
    );

    // Observe the replayed DOM once and mint durable eids over it. No re-render,
    // no reorder, no fixture hook: a real application page, observed as the
    // agent would see it on the first turn.
    let mut map = IdentityMap::new();
    let nodes = session.observer.observe().await?;
    let observed = nodes.len();
    let diff = map.observe(nodes).diff;

    println!(
        "observe: {observed} accessibility nodes -> {} durable eids minted",
        diff.added.len()
    );
    assert!(
        !diff.added.is_empty(),
        "observing the replayed page minted no eids; the recording reconstructed \
         no observable elements (is ANCHORTREE_REPLAY_URL the recorded document?)"
    );

    // Show a handful of the minted handles so the proof is legible: these are
    // real elements of a real WebArena-Verified page, reached with no live origin.
    println!("sample durable handles on the replayed page:");
    for eid in diff.added.iter().take(8) {
        println!("  {}", eid.0);
    }

    println!(
        "\nOK: a real WebArena-Verified page was reconstructed ENTIRELY from a \
         recorded HAR and anchortree minted {} durable eids over it. No live \
         origin was touched during replay.",
        diff.added.len()
    );
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
/// example stays inside the `ws://`-only transport envelope.
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
