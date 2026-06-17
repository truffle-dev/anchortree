//! Phase 3.2c live proof: a cross-origin OOPIF's widget surfaces in `observe()`
//! and the identity engine gives it a frame-namespaced eid that *rebinds* when
//! the OOPIF re-renders the widget into a fresh DOM node.
//!
//! The [`attach_oopif`](attach_oopif) demo proved the channel join: a
//! cross-origin child is reachable as its own CDP session and that session keys
//! to a durable [`FrameKey`]. This demo proves the layer above it. A real agent
//! does not want a session id and a frame key; it feeds one flat observation to
//! the identity engine, and the OOPIF's button comes back with the same stable
//! `Eid` shape as every other element, namespaced under its frame.
//!
//! `getDocument { pierce: true }` never reaches an out-of-process iframe, so the
//! observer attaches to each OOPIF child as its own session, fuses that
//! session's accessibility tree *independently* (its `backendNodeId` and
//! `AXNodeId` spaces can collide with the root's under site isolation, so they
//! must never share a fuse pass; `DECISIONS.md` D23), stamps every node with the
//! child's structural [`FrameKey`], and concatenates the result onto the root's.
//! The engine then namespaces the OOPIF button under its frame, e.g. `f0/...`.
//!
//! Then the OOPIF re-renders: `child.html` swaps its widget's `innerHTML` ~1.2s
//! after load, destroying the button and recreating a structurally identical
//! one. The new button is a different DOM node with a different `backendNodeId`,
//! so the engine's fast `(FrameKey, backendNodeId)` path misses and the rebind
//! path (accessible name + structural path) must carry the identity. The proof:
//! the second observation reports the OOPIF button's eid as *rebound*, not as a
//! removed/added churn.
//!
//! ## Running it
//!
//! Same setup as [`attach_oopif`](attach_oopif): a site-isolated Chrome and a
//! two-origin static server whose `child.html` carries the timed `innerHTML`
//! swap.
//!
//! ```text
//! ANCHORTREE_CDP_HTTP=http://<chrome-ip>:9222 \
//!     ANCHORTREE_OOPIF_URL=http://origin-a:8080/parent.html \
//!     cargo run -p anchortree-cdp --example observe_oopif
//! ```

use std::error::Error;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use anchortree_cdp::connect_hosted;
use anchortree_core::{IdentityMap, ObservationSource};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let ws_url = resolve_ws_url()?;
    let parent_url = std::env::var("ANCHORTREE_OOPIF_URL")
        .map_err(|_| "set ANCHORTREE_OOPIF_URL to a page embedding a cross-origin iframe")?;

    println!("connecting to {ws_url}");
    let mut session = connect_hosted(ws_url).await?;

    // Load the parent and give the cross-origin child a beat to spin up its own
    // process, attach, and paint its first render.
    session.navigate(&parent_url).await?;
    tokio::time::sleep(Duration::from_millis(800)).await;

    // The identity engine carries durable eids across both observations.
    let mut map = IdentityMap::new();

    // First observation: every element is added. The OOPIF button must be among
    // them, under a non-root frame key (eid begins "f<key>/").
    let first = map.observe(session.observer.observe().await?);
    let oopif_eid = first
        .diff
        .added
        .iter()
        .find(|eid| {
            map.binding(eid).is_some_and(|b| {
                !b.frame_key.is_root() && b.fingerprint.accessible_name == "Buy now"
            })
        })
        .cloned()
        .ok_or(
            "the cross-origin OOPIF button did not surface in observe(); is Chrome running \
             with --site-per-process and is the iframe genuinely cross-origin?",
        )?;
    let first_backend = map.binding(&oopif_eid).unwrap().backend_node_id;
    // The sole iframe is the first (and only) frame owner in document order, so
    // it must key frame ordinal 0: eid `f0/...`. Before the D24 frame-owner
    // node-type guard the main frame's `#document` was wrongly counted at
    // ordinal 0 and this same button keyed `f1/...`; asserting `f0/` here is the
    // live proof that the phantom owner is gone.
    assert!(
        oopif_eid.0.starts_with("f0/"),
        "the sole OOPIF must key frame ordinal 0 (eid f0/...), got {} - a phantom \
         frame owner (the main #document) is being counted (D24)",
        oopif_eid.0
    );
    println!(
        "\nfirst observe: OOPIF button -> eid {} (frame-namespaced), backend {first_backend}",
        oopif_eid.0
    );

    // Sanity: a root-frame element must NOT carry the frame namespace, proving
    // the OOPIF nodes folded under a *distinct* key, not merged into root.
    let root_eid = first
        .diff
        .added
        .iter()
        .find(|eid| {
            map.binding(eid)
                .is_some_and(|b| b.fingerprint.accessible_name == "Save document")
        })
        .cloned()
        .ok_or("the root-frame button did not surface in observe()")?;
    assert!(
        map.binding(&root_eid).unwrap().frame_key.is_root() && !root_eid.0.starts_with('f'),
        "root button eid {} should not be frame-namespaced",
        root_eid.0
    );
    println!("root button -> eid {} (root frame)", root_eid.0);

    // Wait past child.html's 1.2s timer: the OOPIF destroys and recreates the
    // button, so its backendNodeId churns under the same structural identity.
    tokio::time::sleep(Duration::from_millis(1600)).await;

    // Second observation: the OOPIF button is a fresh DOM node, so the fast
    // (FrameKey, backend) path misses and the engine must rebind it by name +
    // structural path. It must land in `rebound`, never removed/added.
    let second = map.observe(session.observer.observe().await?);
    let second_backend = map
        .binding(&oopif_eid)
        .ok_or("the OOPIF button's eid did not survive the innerHTML swap")?
        .backend_node_id;
    println!(
        "\nsecond observe: OOPIF button -> eid {} still, backend {second_backend}",
        oopif_eid.0
    );
    println!(
        "  diff: added={:?} rebound={:?} removed={:?}",
        second.diff.added, second.diff.rebound, second.diff.removed
    );

    assert!(
        second.diff.rebound.contains(&oopif_eid),
        "the OOPIF button must be reported rebound across the re-render, not churned"
    );
    assert!(
        !second.diff.removed.contains(&oopif_eid),
        "the OOPIF button must not be reported removed"
    );
    assert_ne!(
        first_backend, second_backend,
        "the swap must have produced a fresh DOM node (new backendNodeId); \
         otherwise the rebind path was never exercised"
    );

    println!(
        "\nOK: a cross-origin OOPIF widget surfaced under the frame-namespaced eid {} \
         and held that identity across an innerHTML swap inside the OOPIF \
         (backend {first_backend} -> {second_backend}).",
        oopif_eid.0
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
/// `webSocketDebuggerUrl`. Dependency-free on purpose (see `attach_oopif`).
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
