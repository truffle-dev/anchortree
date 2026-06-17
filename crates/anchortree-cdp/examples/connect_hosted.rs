//! Phase 3.1b end-to-end proof: durable identity across a re-render, driven over
//! the **hosted connect leg** rather than a browser we launched ourselves.
//!
//! [`observe_rerender`](../observe_rerender.rs) proves identity survival against
//! a page anchortree *opened*; this proves it against a page a browser
//! **already had open**, flat-attached to over a raw CDP channel
//! ([`connect_hosted`]). That is the case chromiumoxide 0.9.1 cannot drive
//! cleanly (see `DECISIONS.md` D19/D20): the channel issues
//! `Target.attachToTarget { flatten: true }` itself and tags every later command
//! with the returned `sessionId`. The fusion pipeline above it is byte-for-byte
//! the same one the local path runs.
//!
//! ## Running it
//!
//! Against a **local** headless Chrome (the cheap, repeatable check — the raw
//! channel is transport-agnostic, so a `ws://` browser exercises the exact same
//! flat-attach path a hosted `wss://` one does):
//!
//! ```text
//! docker run -d --name anchortree-chrome --network phantom_phantom-net \
//!     chromedp/headless-shell:latest
//! ANCHORTREE_CDP_HTTP=http://<container-ip>:9222 \
//!     cargo run -p anchortree-cdp --example connect_hosted
//! ```
//!
//! Against **Browserbase** (the real hosted gateway, `wss://` + TLS): set both
//! credentials and the example mints a session over REST, then flat-attaches to
//! the page that session already has open:
//!
//! ```text
//! BROWSERBASE_API_KEY=… BROWSERBASE_PROJECT_ID=… \
//!     cargo run -p anchortree-cdp --example connect_hosted
//! ```
//!
//! With nothing configured it prints usage and exits 0, so it is unattended-safe
//! and still type-checks the whole connect leg in CI.

use std::error::Error;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use anchortree_cdp::{connect_hosted, gateway};
use anchortree_core::{Diff, Eid, IdentityMap, ObservationSource as _};

/// Baseline page: a named landmark holding two buttons and a text input, each
/// carrying a developer-stable `id` (the strongest rebind rung).
const JS_BASELINE: &str = r#"document.body.innerHTML = `<main><h1>Account</h1><button id="save">Save</button><button id="cancel">Cancel</button><input id="email" type="text" aria-label="Email"></main>`; true"#;

/// The re-render: replace `<main>`'s entire contents. Every child is a new DOM
/// node with a fresh `backendNodeId`; the ids are unchanged.
const JS_RERENDER: &str = r#"document.querySelector('main').innerHTML = `<h1>Account</h1><button id="save">Save changes</button><button id="cancel">Cancel</button><input id="email" type="text" aria-label="Email">`; true"#;

/// An in-place edit: mutate one button's text without recreating the node, so
/// its `backendNodeId` is preserved and the change rides the cheap path.
const JS_TEXT_EDIT: &str = r#"document.getElementById('cancel').textContent = 'Discard'; true"#;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let Some((label, ws_url)) = resolve_endpoint().await? else {
        print_usage();
        return Ok(());
    };

    println!("connecting (hosted leg) to {label}");
    // The credential rides in a hosted URL's query string; never print it raw.
    let mut session = connect_hosted(&ws_url).await?;
    println!("flat-attached to the page the browser already had open");

    // One identity map spans every observation: it is what carries an Eid
    // forward across the re-render.
    let mut map = IdentityMap::new();

    // Give the attached page a known starting point, then paint the baseline.
    session.navigate("about:blank").await?;

    // --- Observation 1: the baseline page. Everything is first-seen. ---
    session.evaluate(JS_BASELINE).await?;
    let obs1 = session.observer.observe().await?;
    let d1 = map.observe(obs1).diff;
    print_diff("observation 1 (baseline render)", &d1);

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
    session.evaluate(JS_RERENDER).await?;
    let obs2 = session.observer.observe().await?;
    let d2 = map.observe(obs2).diff;
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
    session.evaluate(JS_TEXT_EDIT).await?;
    let obs3 = session.observer.observe().await?;
    let d3 = map.observe(obs3).diff;
    print_diff("observation 3 (in-place text edit)", &d3);
    assert!(
        d3.changed.iter().any(|c| c.text == "Discard"),
        "editing a node's text in place is reported as a content change"
    );
    assert!(
        d3.rebound.is_empty(),
        "an in-place edit keeps the same DOM node, so nothing rebinds"
    );

    println!("\nOK: logical identity survived a real re-render over the hosted connect leg.");
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

/// Resolve the hosted endpoint to connect to, returning a `(label, ws_url)`
/// pair. `Ok(None)` means nothing was configured.
///
/// Browserbase credentials win if both are set (the real hosted-gateway path);
/// otherwise a local `ws://` browser via `ANCHORTREE_CDP_WS` or the
/// `/json/version` endpoint in `ANCHORTREE_CDP_HTTP`.
async fn resolve_endpoint() -> Result<Option<(String, String)>, Box<dyn Error>> {
    if let (Ok(key), Ok(project)) = (
        std::env::var("BROWSERBASE_API_KEY"),
        std::env::var("BROWSERBASE_PROJECT_ID"),
    ) {
        if !key.is_empty() && !project.is_empty() {
            println!("minting a Browserbase session over REST…");
            let acquired = gateway::browserbase::acquire(&project, &key).await?;
            let label = match &acquired.session_id {
                Some(id) => format!("Browserbase session {id}"),
                None => "Browserbase".to_string(),
            };
            return Ok(Some((label, acquired.connect_url)));
        }
    }

    if let Ok(ws) = std::env::var("ANCHORTREE_CDP_WS") {
        if !ws.is_empty() {
            return Ok(Some((format!("local {ws}"), ws)));
        }
    }
    if let Ok(http) = std::env::var("ANCHORTREE_CDP_HTTP") {
        if !http.is_empty() {
            let ws = fetch_ws_debugger_url(&http)?;
            return Ok(Some((format!("local {ws}"), ws)));
        }
    }
    Ok(None)
}

fn print_usage() {
    eprintln!(
        "connect_hosted: configure one endpoint to run the hosted connect-leg proof.\n\
         \n\
         Browserbase (real hosted gateway, wss://):\n  \
         BROWSERBASE_API_KEY=<key> BROWSERBASE_PROJECT_ID=<id> \\\n    \
         cargo run -p anchortree-cdp --example connect_hosted\n\
         \n\
         Local headless Chrome (ws://, same flat-attach path):\n  \
         ANCHORTREE_CDP_HTTP=http://<container-ip>:9222 \\\n    \
         cargo run -p anchortree-cdp --example connect_hosted\n\
         \n\
         No endpoint configured, nothing to do. Exiting 0."
    );
}

/// Issue a minimal blocking `GET /json/version` and pull out
/// `webSocketDebuggerUrl`. Dependency-free, matching `observe_rerender`.
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
