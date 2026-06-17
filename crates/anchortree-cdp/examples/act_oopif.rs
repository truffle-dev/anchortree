//! Phase 3.2d live proof: a trusted click on a cross-origin OOPIF element lands
//! *inside* the out-of-process iframe, dispatched on that frame's owning CDP
//! session.
//!
//! [`observe_oopif`](observe_oopif) proved the read half — an OOPIF widget
//! surfaces in `observe()` under a frame-namespaced eid that rebinds across a
//! re-render. This proves the write half, and it is the whole reason the read
//! half matters: an agent that can *see* the OOPIF button has to be able to
//! *press* it.
//!
//! The hard part is that an out-of-process iframe lives in a different renderer
//! and a different CDP session. Its `backendNodeId` space does not exist on the
//! page session, so a click dispatched there would resolve nothing (or, worse,
//! the wrong node). The agent must never have to know this. It holds the same
//! flat eid the engine handed back (`f0/...`), calls one method —
//! [`CdpObserver::act`](anchortree_cdp::CdpObserver::act) — and the engine reads
//! the eid's frame off its live binding, looks up the child session that owns
//! that frame, and tags the trusted pointer gesture with it. The action lands in
//! the frame the identity was observed in. That closes the dispatch half of D23
//! and decision D22.
//!
//! The proof is end to end and needs no privileged read into the child: the
//! OOPIF's button reveals a `role="status"` line whose text is taken from
//! `event.isTrusted`. After the routed click, a *second* observation reports a
//! new node, under the OOPIF's non-root frame key, whose accessible name is
//! exactly `Purchased` — which can only happen if the click reached the right
//! node in the right frame and arrived trusted. A misrouted click leaves the
//! status hidden; an untrusted one names it `Untrusted click`.
//!
//! ## Running it
//!
//! Same harness as [`observe_oopif`](observe_oopif): a site-isolated Chrome and a
//! two-origin static server. The fixtures live next to this file under
//! `examples/fixtures/oopif/` (`parent_action.html` on origin-a embeds
//! `child_action.html` on origin-b).
//!
//! ```text
//! # serve examples/fixtures/oopif on a host reachable as origin-a and origin-b
//! ANCHORTREE_CDP_HTTP=http://<chrome-ip>:9222 \
//!     ANCHORTREE_OOPIF_URL=http://origin-a:8080/parent_action.html \
//!     cargo run -p anchortree-cdp --example act_oopif
//! ```

use std::error::Error;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use anchortree_cdp::{Action, connect_hosted};
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

    let mut map = IdentityMap::new();

    // First observation: locate the OOPIF "Buy now" button by its frame-
    // namespaced eid (begins "f<key>/", non-root frame key).
    let first = map.observe(session.observer.observe().await?);
    let buy_eid = first
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
    assert!(
        buy_eid.0.starts_with("f0/"),
        "the sole OOPIF must key frame ordinal 0 (eid f0/...), got {}",
        buy_eid.0
    );
    let before = map
        .binding(&buy_eid)
        .unwrap()
        .fingerprint
        .accessible_name
        .clone();
    println!(
        "\nfirst observe: OOPIF button -> eid {} (frame-namespaced), name {before:?}",
        buy_eid.0
    );
    assert_eq!(
        before, "Buy now",
        "the OOPIF button must start labelled \"Buy now\"; the fixture is wrong if it is not"
    );

    // --- The action under test: a routed, trusted click. The agent passes only
    //     the flat eid; the observer resolves it to the OWNING child session and
    //     dispatches the pointer gesture there. ---
    session.observer.act(&map, &buy_eid, Action::Click).await?;
    println!("dispatched routed Action::Click on {} ...", buy_eid.0);

    // Let the OOPIF's click handler run and relabel the button.
    tokio::time::sleep(Duration::from_millis(400)).await;

    // Second observation: the OOPIF button (same eid, same OOPIF frame) now reads
    // its post-click label. The eid must still resolve under a non-root frame
    // key, and its accessible name must be exactly "Purchased" — which can only
    // happen if the click reached this node, in this frame, and arrived trusted.
    let _second = map.observe(session.observer.observe().await?);
    let binding = map
        .binding(&buy_eid)
        .ok_or("the OOPIF button's eid did not survive the click")?;
    let after = binding.fingerprint.accessible_name.clone();
    println!(
        "second observe: OOPIF button name {after:?} (frame {})",
        binding.frame_key
    );

    assert!(
        !binding.frame_key.is_root(),
        "the relabelled button must still resolve inside the OOPIF frame, not the root"
    );
    assert_ne!(
        after, "Untrusted click",
        "the click reached the button but arrived untrusted (isTrusted: false) - the CDP Input \
         path was not used"
    );
    assert_eq!(
        after, "Purchased",
        "the OOPIF button did not relabel after the routed click; the dispatch did not land on \
         the OOPIF element in its owning session"
    );

    println!(
        "\nOK: a routed trusted click on OOPIF eid {} landed inside the out-of-process iframe \
         (\"Buy now\" -> \"Purchased\"), dispatched on the frame's owning child session.",
        buy_eid.0
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
