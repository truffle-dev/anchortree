//! Phase 2.1 end-to-end demo: act on an element *after* it has been re-rendered.
//!
//! This is the action-space counterpart to `observe_rerender`. It proves the
//! payoff of durable identity: an agent can decide to act on a control during
//! one render, and the action still lands after a framework has destroyed and
//! rebuilt that control underneath it.
//!
//! The script:
//!   1. builds a small settings page (a toggle button, an email field, a size
//!      `<select>`), observes it, and remembers the logical [`Eid`]s;
//!   2. forces a full `innerHTML` swap so every control is a brand-new DOM node
//!      with a fresh `backendNodeId` — the same churn `observe_rerender` shows —
//!      and observes again, confirming the eids survived as `rebound`;
//!   3. then, against the *post-re-render* eids, dispatches three trusted
//!      actions through the identity map: [`Action::Click`] the toggle,
//!      [`Action::Type`] into the email field, [`Action::Select`] the size; and
//!   4. reads the live DOM back to prove each action mutated the real page, and
//!      that the click arrived with `isTrusted: true` (a page-script
//!      `element.click()` could not).
//!
//! ## Running it
//!
//! Same target as `observe_rerender` — a headless Chrome on the phantom
//! network, addressed by container IP:
//!
//! ```text
//! docker run -d --name anchortree-chrome --network phantom_phantom-net \
//!     chromedp/headless-shell:latest
//! ANCHORTREE_CDP_HTTP=http://<container-ip>:9222 \
//!     cargo run -p anchortree-cdp --example act_after_rerender
//! ```
//!
//! or pass `ANCHORTREE_CDP_WS=ws://<ip>:9222/devtools/browser/<id>` straight
//! through.

use std::error::Error;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use anchortree_cdp::{Action, act, connect};
use anchortree_core::{Diff, Eid, IdentityMap, ObservationSource as _};

/// Baseline settings page: a toggle button that flips a status line (recording
/// `event.isTrusted` so we can prove the dispatch was trusted), an email field,
/// and a size `<select>` defaulting to `medium`. Each control carries a stable
/// `id`, the strongest rebind rung. The `<select>` gets an explicit
/// `role="combobox"` so its accessible role is deterministic across Chrome
/// builds; it remains a real `HTMLSelectElement` with a `.value`.
const JS_BASELINE: &str = r#"document.body.innerHTML = `<main><h1>Settings</h1><button id="toggle" onclick="window.__trusted = event.isTrusted; const s = document.getElementById('status'); s.textContent = s.textContent === 'Off' ? 'On' : 'Off';">Toggle</button><input id="email" type="text" aria-label="Email"><select id="size" role="combobox" aria-label="Size"><option value="small">Small</option><option value="medium" selected>Medium</option><option value="large">Large</option></select><p id="status" role="status">Off</p></main>`; window.__trusted = null; true"#;

/// The re-render: replace `<main>`'s contents wholesale. Every child is a new
/// DOM node with a fresh `backendNodeId`; ids and the select's `medium` default
/// are preserved, and the button label is nudged to show a content change folds
/// into the rebind rather than surfacing as a separate `changed`.
const JS_RERENDER: &str = r#"document.querySelector('main').innerHTML = `<h1>Settings</h1><button id="toggle" onclick="window.__trusted = event.isTrusted; const s = document.getElementById('status'); s.textContent = s.textContent === 'Off' ? 'On' : 'Off';">Toggle setting</button><input id="email" type="text" aria-label="Email"><select id="size" role="combobox" aria-label="Size"><option value="small">Small</option><option value="medium" selected>Medium</option><option value="large">Large</option></select><p id="status" role="status">Off</p>`; true"#;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let ws_url = resolve_ws_url()?;
    println!("connecting to {ws_url}");
    let mut session = connect(ws_url).await?;

    let mut map = IdentityMap::new();

    // --- Observe the baseline, capture the logical handles. ---
    session.observer.page().evaluate(JS_BASELINE).await?;
    let d1 = map.observe(session.observer.observe().await?).diff;
    print_diff("observation 1 (baseline)", &d1);

    let toggle = find_eid(&d1.added, "toggle").expect("baseline mints a toggle eid");
    let email = find_eid(&d1.added, "email").expect("baseline mints an email eid");
    let size = find_eid(&d1.added, "size").expect("baseline mints a size eid");
    println!("  handles: toggle={toggle}, email={email}, size={size}");

    // --- Re-render so every control is a new DOM node, same identity. ---
    session.observer.page().evaluate(JS_RERENDER).await?;
    let d2 = map.observe(session.observer.observe().await?).diff;
    print_diff("observation 2 (after innerHTML swap)", &d2);
    for eid in [&toggle, &email, &size] {
        assert!(
            d2.rebound.contains(eid),
            "{eid} should survive the re-render as a rebind, not remove+add"
        );
    }
    assert!(
        d2.added.is_empty() && d2.removed.is_empty(),
        "a pure re-render of the same controls adds and removes nothing"
    );
    println!("  all three controls rebound onto fresh DOM nodes");

    // --- Act on the post-re-render eids. Each is resolved through the map to a
    //     live backendNodeId at call time; none was captured before the swap. ---

    // Click the toggle: status flips Off -> On, and the handler records that the
    // event was trusted.
    act(session.observer.page(), &map, &toggle, Action::Click).await?;
    let status: String = session
        .observer
        .page()
        .evaluate("document.getElementById('status').textContent")
        .await?
        .into_value()?;
    let trusted: bool = session
        .observer
        .page()
        .evaluate("window.__trusted === true")
        .await?
        .into_value()?;
    println!("\nafter click(toggle): status={status:?}, isTrusted={trusted}");
    assert_eq!(
        status, "On",
        "the click should have toggled the status to On"
    );
    assert!(
        trusted,
        "the click must arrive as a trusted event (isTrusted: true)"
    );

    // Type into the email field, clearing first.
    act(
        session.observer.page(),
        &map,
        &email,
        Action::Type {
            text: "agent@anchortree.dev".to_string(),
            clear: true,
        },
    )
    .await?;
    let value: String = session
        .observer
        .page()
        .evaluate("document.getElementById('email').value")
        .await?
        .into_value()?;
    println!("after type(email): value={value:?}");
    assert_eq!(
        value, "agent@anchortree.dev",
        "the typed text should be the field's value"
    );

    // Select the large size.
    act(
        session.observer.page(),
        &map,
        &size,
        Action::Select {
            value: "large".to_string(),
        },
    )
    .await?;
    let chosen: String = session
        .observer
        .page()
        .evaluate("document.getElementById('size').value")
        .await?
        .into_value()?;
    println!("after select(size): value={chosen:?}");
    assert_eq!(
        chosen, "large",
        "the select should now hold the large option"
    );

    // --- A final observation: the engine sees the consequences of the actions. ---
    let d3 = map.observe(session.observer.observe().await?).diff;
    print_diff("observation 3 (after the three actions)", &d3);

    println!(
        "\nOK: three trusted actions landed on controls that were re-rendered after the agent chose them."
    );
    Ok(())
}

/// First eid in `added` whose string contains `needle` (matches regardless of
/// the role prefix the engine minted).
fn find_eid(added: &[Eid], needle: &str) -> Option<Eid> {
    added.iter().find(|e| e.0.contains(needle)).cloned()
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
/// `webSocketDebuggerUrl`. Dependency-free (no TLS, no HTTP crate) to stay
/// inside the `ws://`-only transport envelope (see `DECISIONS.md` D8/D10).
fn fetch_ws_debugger_url(http_endpoint: &str) -> Result<String, Box<dyn Error>> {
    let host_port = http_endpoint
        .strip_prefix("http://")
        .ok_or("ANCHORTREE_CDP_HTTP must start with http://")?
        .trim_end_matches('/');

    let mut stream = TcpStream::connect(host_port)?;
    // Chrome's CDP HTTP endpoint speaks keep-alive and ignores `Connection:
    // close`, so reading to EOF would block forever. Read the headers, honour
    // `Content-Length`, and stop. The read timeout is a belt-and-braces guard.
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
