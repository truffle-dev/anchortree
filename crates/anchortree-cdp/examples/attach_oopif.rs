//! Phase 3.2b live proof: a cross-origin OOPIF is reachable as its own CDP
//! session, and that session joins to a durable frame key.
//!
//! Same-origin frames (the [`observe_frames`](observe_frames) demo) are pierced
//! into the root target: their nodes arrive in one `getDocument { pierce: true }`
//! pass and the frame walk keys them off the inline `contentDocument`. A
//! *cross-origin* iframe is different. Under site isolation it is an
//! out-of-process iframe (OOPIF): a separate CDP target, with its own
//! `backendNodeId` space, that `getDocument { pierce: true }` never reaches. The
//! only way in is `Target.setAutoAttach { flatten: true }`, which hands back a
//! fresh `sessionId` per child.
//!
//! This example proves the join that makes that child's identity durable
//! (`DECISIONS.md` D22 step 3): an OOPIF child target's id *equals* the page
//! `frameId` the root frame tree already keyed, so
//! [`child_frame_keys`](anchortree_cdp::child_frame_keys) maps the child session
//! straight onto the same structural [`FrameKey`] an agent would use to namespace
//! its elements. No new frame-id round-trip, no guable identity.
//!
//! ## Running it
//!
//! It needs a *site-isolated* Chrome and a page with a genuinely cross-origin
//! iframe (same host, different port is same-site and stays in-process). Bring up
//! a two-origin static server and a `--site-per-process` browser on one network:
//!
//! ```text
//! docker run -d --name oopif-web --network <net> \
//!     --network-alias origin-a --network-alias origin-b \
//!     python:3.12-alpine sh -c "mkdir -p /srv && cd /srv && python -m http.server 8080"
//! # parent.html on origin-a embeds <iframe src="http://origin-b:8080/child.html">
//! docker run -d --name chrome --network <net> \
//!     chromedp/headless-shell:latest --site-per-process
//! ```
//!
//! Then point the demo at the browser's HTTP endpoint and the parent URL:
//!
//! ```text
//! ANCHORTREE_CDP_HTTP=http://<chrome-ip>:9222 \
//!     ANCHORTREE_OOPIF_URL=http://origin-a:8080/parent.html \
//!     cargo run -p anchortree-cdp --example attach_oopif
//! ```

use std::error::Error;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use anchortree_cdp::{child_frame_keys, connect_hosted};
use anchortree_core::FrameKey;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let ws_url = resolve_ws_url()?;
    let parent_url = std::env::var("ANCHORTREE_OOPIF_URL")
        .map_err(|_| "set ANCHORTREE_OOPIF_URL to a page embedding a cross-origin iframe")?;

    println!("connecting to {ws_url}");
    let session = connect_hosted(ws_url).await?;

    // Load the parent and give the cross-origin child a beat to spin up its own
    // process and attach.
    session.navigate(&parent_url).await?;
    tokio::time::sleep(Duration::from_millis(800)).await;

    // The root frame tree keys every frame structurally. The root document is
    // FrameKey::root (""), the first child frame is "0", and so on - durable
    // across reloads because it is an ordinal path, not the volatile frameId.
    let frame_keys = session.frame_keys().await?;
    println!("\nframe tree -> structural keys:");
    let mut keyed: Vec<_> = frame_keys.iter().collect();
    keyed.sort_by_key(|(id, _)| (*id).clone());
    for (frame_id, key) in &keyed {
        let shown = if key.is_root() {
            "<root>".to_string()
        } else {
            key.0.clone()
        };
        println!("  frame {frame_id} -> {shown}");
    }

    // Auto-attach to the OOPIF: it announces itself as a child session whose
    // target id is its frame id.
    let children = session.auto_attach_children().await?;
    println!("\nattached child sessions:");
    for c in &children {
        println!(
            "  session {} -> target {} (type {})",
            c.session_id, c.target_id, c.target_type
        );
    }

    // The join: child session id -> durable frame key, via target_id == frame_id.
    let pairs = children
        .iter()
        .map(|c| (c.session_id.as_str(), c.target_id.as_str()));
    let joined = child_frame_keys(pairs, &frame_keys);
    println!("\nchild session -> durable frame key:");
    for (session_id, key) in &joined {
        println!("  session {session_id} -> frame key {}", key.0);
    }

    // Assert the proof: at least one cross-origin child joined to a *non-root*
    // structural frame key. That is D22 step 3 confirmed against real Chrome -
    // the OOPIF's separate CDP target carries the same durable identity the
    // engine would namespace its in-frame elements under.
    let oopif_join = joined.values().find(|k| !k.is_root());
    let joined_key = oopif_join.ok_or(
        "no cross-origin child joined to a frame key; is Chrome running with \
         --site-per-process and is the iframe genuinely cross-origin?",
    )?;
    assert_ne!(*joined_key, FrameKey::root());
    println!(
        "\nOK: a cross-origin OOPIF attached as its own CDP session and joined to \
         the durable frame key {} (target id == frame id).",
        joined_key.0
    );
    Ok(())
}

/// Resolve a `ws://` CDP URL from the environment.
///
/// `ANCHORTREE_CDP_WS` wins if set. Otherwise `ANCHORTREE_CDP_HTTP` is treated
/// as a Chrome HTTP endpoint and queried for its `webSocketDebuggerUrl`.
fn resolve_ws_url() -> Result<String, Box<dyn Error>> {
    if let Ok(ws) = std::env::var("ANCHORTREE_CDP_WS")
        && !ws.is_empty()
    {
        return Ok(ws);
    }
    let http = std::env::var("ANCHORTREE_CDP_HTTP").map_err(|_| {
        "set ANCHORTREE_CDP_WS=ws://<ip>:9222/devtools/browser/<id> or \
         ANCHORTREE_CDP_HTTP=http://<chrome-ip>:9222"
    })?;
    fetch_ws_debugger_url(&http)
}

/// Issue a minimal blocking `GET /json/version` and pull out
/// `webSocketDebuggerUrl`. Dependency-free on purpose (see `observe_rerender`).
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
