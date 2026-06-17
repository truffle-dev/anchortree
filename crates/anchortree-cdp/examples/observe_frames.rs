//! Phase 3.2 end-to-end demo: durable identity is frame-scoped.
//!
//! Two structurally identical widgets, one in the root document and one in a
//! same-origin `srcdoc` iframe, must hold *distinct* durable identities, and a
//! re-render inside the iframe must rebind only the iframe's element while the
//! root element stays put. This is the live proof of decision D21's first tier:
//! the eid is `(frame, in-frame fingerprint)`, not fingerprint alone.
//!
//! The two buttons share everything an old single-tier engine would key on -
//! role `button`, accessible name `Action`, developer id `act`, and the
//! landmark-anchored structural path `main>button:1`. The only thing that tells
//! them apart is the frame they live in, so the root button mints `btn-action`
//! and the iframe button mints the frame-namespaced `f0/btn-action`. When the
//! iframe's inner DOM is swapped wholesale (new `backendNodeId`), the engine
//! rebinds `f0/btn-action` and leaves `btn-action` untouched.
//!
//! ## Running it
//!
//! Bring up a headless Chrome on the phantom network:
//!
//! ```text
//! docker run -d --name anchortree-chrome --network phantom_phantom-net \
//!     chromedp/headless-shell:latest
//! ```
//!
//! Then point the demo at it by HTTP endpoint (it reads the IP-based
//! `webSocketDebuggerUrl` from `/json/version`):
//!
//! ```text
//! ANCHORTREE_CDP_HTTP=http://<container-ip>:9222 \
//!     cargo run -p anchortree-cdp --example observe_frames
//! ```
//!
//! or pass the WebSocket URL straight through with `ANCHORTREE_CDP_WS`. Connect
//! by container **IP**, not hostname (see `observe_rerender.rs` for why).

use std::error::Error;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use anchortree_cdp::connect;
use anchortree_core::{Diff, Eid, IdentityMap, ObservationSource as _};

/// Baseline page: a root `<main>` with one button, plus a same-origin `srcdoc`
/// iframe whose own `<main>` holds a structurally identical button. Both buttons
/// carry the same developer id `act` and the same label, so only the frame
/// boundary distinguishes them.
const JS_BASELINE: &str = r#"document.body.innerHTML = `<main><button id="act">Action</button></main><iframe id="f" srcdoc='<main><button id=&quot;act&quot;>Action</button></main>'></iframe>`; true"#;

/// Re-render *inside the iframe only*: replace the frame document's `<main>`
/// contents, so the frame's button is destroyed and recreated with a brand-new
/// `backendNodeId`. The root button's node is never touched.
const JS_FRAME_RERENDER: &str = r#"document.getElementById('f').contentDocument.querySelector('main').innerHTML = `<button id="act">Action</button>`; true"#;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let ws_url = resolve_ws_url()?;
    println!("connecting to {ws_url}");
    let mut session = connect(ws_url).await?;

    let mut map = IdentityMap::new();

    // --- Observation 1: the baseline. Two identical widgets, two frames. ---
    session.observer.page().evaluate(JS_BASELINE).await?;
    // The srcdoc document loads asynchronously; give it a beat to attach before
    // the first pierced pass, else the frame's button is not yet in the tree.
    tokio::time::sleep(Duration::from_millis(400)).await;
    let obs1 = session.observer.observe().await?;
    let d1 = map.observe(obs1).diff;
    print_diff("observation 1 (baseline: root + same-origin iframe)", &d1);

    let root = Eid("btn-action".into());
    let frame = Eid("f0/btn-action".into());
    assert!(
        d1.added.contains(&root),
        "the root button should mint the bare eid btn-action, got {:?}",
        d1.added
    );
    assert!(
        d1.added.contains(&frame),
        "the iframe button should mint the frame-namespaced eid f0/btn-action, got {:?}",
        d1.added
    );
    let root_backend = map.binding(&root).expect("root bound").backend_node_id;
    let frame_backend_1 = map.binding(&frame).expect("frame bound").backend_node_id;
    assert_ne!(
        root_backend, frame_backend_1,
        "the two buttons are different DOM nodes in different frames"
    );
    println!("  distinct identities: btn-action (root) and f0/btn-action (frame) both minted");

    // --- Observation 2: re-render the iframe only. ---
    session.observer.page().evaluate(JS_FRAME_RERENDER).await?;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let obs2 = session.observer.observe().await?;
    let d2 = map.observe(obs2).diff;
    print_diff("observation 2 (iframe re-render only)", &d2);

    assert_eq!(
        d2.rebound,
        vec![frame.clone()],
        "only the iframe element rebinds; the root element is untouched"
    );
    assert!(
        d2.added.is_empty() && d2.removed.is_empty(),
        "a pure in-frame re-render adds and removes nothing, got added={:?} removed={:?}",
        d2.added,
        d2.removed
    );
    let frame_backend_2 = map
        .binding(&frame)
        .expect("frame still bound")
        .backend_node_id;
    assert_ne!(
        frame_backend_1, frame_backend_2,
        "the iframe button should be re-bound to a brand-new DOM node"
    );
    assert_eq!(
        map.binding(&root)
            .expect("root still bound")
            .backend_node_id,
        root_backend,
        "the root button's node is unchanged across the iframe re-render"
    );
    println!(
        "  rebind ledger: f0/btn-action backendNodeId {frame_backend_1} -> {frame_backend_2} \
         (frame identity held); root btn-action steady on {root_backend}"
    );

    println!("\nOK: durable identity is frame-scoped across a real same-origin iframe.");
    Ok(())
}

/// Pretty-print a [`Diff`] as the four event lists an agent would read.
fn print_diff(label: &str, diff: &Diff) {
    let eids = |v: &[Eid]| v.iter().map(|e| e.0.clone()).collect::<Vec<_>>();
    println!("{label}:");
    println!("  added:   {:?}", eids(&diff.added));
    println!("  rebound: {:?}", eids(&diff.rebound));
    println!(
        "  changed: {:?}",
        diff.changed
            .iter()
            .map(|c| format!("{}={:?}", c.eid.0, c.text))
            .collect::<Vec<_>>()
    );
    println!("  removed: {:?}", eids(&diff.removed));
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
         ANCHORTREE_CDP_HTTP=http://<container-ip>:9222"
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
