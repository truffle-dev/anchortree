//! Phase 1.5a end-to-end demo: durable identity across a real SPA re-render.
//!
//! This is the "alive" proof. It connects to a live headless Chrome over a
//! plain `ws://` CDP endpoint, builds a small page with a `<main>` of
//! stable-id widgets, observes it, then forces a full `innerHTML` swap so every
//! child element is destroyed and recreated with a brand-new `backendNodeId`.
//! It observes again and prints the [`Diff`]. The headline: the logical
//! [`Eid`]s survive the re-render as `rebound` — the agent's handle on each
//! button and input outlives the DOM node it was first bound to. A third,
//! in-place text edit (same DOM node) then shows the cheaper `changed` path.
//!
//! ## Running it
//!
//! Bring up a headless Chrome on the phantom network (no extra Chrome flags;
//! the image's entrypoint already bridges 9222):
//!
//! ```text
//! docker run -d --name anchortree-chrome --network phantom_phantom-net \
//!     chromedp/headless-shell:latest
//! ```
//!
//! Then point the demo at it. Either hand it the HTTP endpoint and let it read
//! `/json/version` for the IP-based `webSocketDebuggerUrl`:
//!
//! ```text
//! ANCHORTREE_CDP_HTTP=http://<container-ip>:9222 \
//!     cargo run -p anchortree-cdp --example observe_rerender
//! ```
//!
//! or pass the WebSocket URL straight through:
//!
//! ```text
//! ANCHORTREE_CDP_WS=ws://<container-ip>:9222/devtools/browser/<id> \
//!     cargo run -p anchortree-cdp --example observe_rerender
//! ```
//!
//! Connect by container **IP**, not hostname: Chrome's CDP HTTP endpoint
//! rejects a `Host` header it does not recognise, and the `/json/version`
//! `webSocketDebuggerUrl` headless-shell returns is already IP-based.

use std::error::Error;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use anchortree_cdp::connect;
use anchortree_core::{Diff, Eid, IdentityMap, ObservationSource as _};

/// Baseline page: a named landmark holding two buttons and a text input, each
/// carrying a developer-stable `id` (the strongest rebind rung).
const JS_BASELINE: &str = r#"document.body.innerHTML = `<main><h1>Account</h1><button id="save">Save</button><button id="cancel">Cancel</button><input id="email" type="text" aria-label="Email"></main>`; true"#;

/// The re-render: replace `<main>`'s entire contents. Every child is a new DOM
/// node with a fresh `backendNodeId`; the ids are unchanged and the save
/// button's label is updated, exactly the churn a client-side framework emits
/// on a state change.
const JS_RERENDER: &str = r#"document.querySelector('main').innerHTML = `<h1>Account</h1><button id="save">Save changes</button><button id="cancel">Cancel</button><input id="email" type="text" aria-label="Email">`; true"#;

/// An in-place edit: mutate one button's text without recreating the node, so
/// its `backendNodeId` is preserved and the change rides the cheap path.
const JS_TEXT_EDIT: &str = r#"document.getElementById('cancel').textContent = 'Discard'; true"#;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let ws_url = resolve_ws_url()?;
    println!("connecting to {ws_url}");
    let mut session = connect(ws_url).await?;

    // One identity map spans every observation: it is what carries an Eid
    // forward across the re-render.
    let mut map = IdentityMap::new();

    // --- Observation 1: the baseline page. Everything is first-seen. ---
    session.observer.page().evaluate(JS_BASELINE).await?;
    let obs1 = session.observer.observe().await?;
    let d1 = map.observe(obs1);
    print_diff("observation 1 (baseline render)", &d1);

    // Snapshot each freshly-minted eid against the DOM node it bound to, so we
    // can prove below that the node changed underneath while the eid did not.
    let baseline: Vec<(Eid, i64)> = d1
        .added
        .iter()
        .map(|eid| {
            let backend = map.binding(eid).expect("just-added eid has a binding");
            (eid.clone(), backend.backend_node_id)
        })
        .collect();
    assert!(
        !baseline.is_empty(),
        "the baseline page should mint at least the two buttons and the input"
    );

    // --- Observation 2: a full innerHTML swap. New nodes, same identities. ---
    session.observer.page().evaluate(JS_RERENDER).await?;
    let obs2 = session.observer.observe().await?;
    let d2 = map.observe(obs2);
    print_diff("observation 2 (after innerHTML swap)", &d2);

    println!("  rebind ledger:");
    for (eid, old_backend) in &baseline {
        assert!(
            d2.rebound.contains(eid),
            "{eid} should have survived the re-render as a rebind, not a remove+add"
        );
        let new_backend = map
            .binding(eid)
            .expect("rebound eid still has a binding")
            .backend_node_id;
        assert_ne!(
            *old_backend, new_backend,
            "{eid} should be re-bound to a brand-new DOM node"
        );
        println!("    {eid}: backendNodeId {old_backend} -> {new_backend} (identity held)");
    }
    assert!(
        d2.added.is_empty() && d2.removed.is_empty(),
        "a pure re-render of the same logical elements adds and removes nothing"
    );

    // --- Observation 3: an in-place text edit on a surviving node. ---
    session.observer.page().evaluate(JS_TEXT_EDIT).await?;
    let obs3 = session.observer.observe().await?;
    let d3 = map.observe(obs3);
    print_diff("observation 3 (in-place text edit)", &d3);
    assert!(
        d3.changed.iter().any(|c| c.text == "Discard"),
        "editing a node's text in place is reported as a content change"
    );
    assert!(
        d3.rebound.is_empty(),
        "an in-place edit keeps the same DOM node, so nothing rebinds"
    );

    println!("\nOK: logical identity survived a real re-render over live CDP.");
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
/// `webSocketDebuggerUrl`. Deliberately dependency-free: no TLS, no HTTP crate,
/// just a single plain-text request so the demo stays inside the `ws://`-only
/// transport envelope (see `DECISIONS.md` D8/D10).
fn fetch_ws_debugger_url(http_endpoint: &str) -> Result<String, Box<dyn Error>> {
    let host_port = http_endpoint
        .strip_prefix("http://")
        .ok_or("ANCHORTREE_CDP_HTTP must start with http://")?
        .trim_end_matches('/');

    let mut stream = TcpStream::connect(host_port)?;
    // Chrome's CDP HTTP endpoint speaks keep-alive and ignores `Connection:
    // close`, so reading to EOF would block forever. We instead read the
    // headers, honour `Content-Length`, and stop. The read timeout is a belt
    // -and-braces guard so a misbehaving endpoint can never hang the demo.
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let request = format!(
        "GET /json/version HTTP/1.1\r\nHost: {host_port}\r\nAccept: application/json\r\n\r\n"
    );
    stream.write_all(request.as_bytes())?;

    // Read until we have the full headers (terminated by a blank line).
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

    // Read the remaining body bytes up to the advertised length.
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
