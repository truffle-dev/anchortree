//! Phase 3.5b live proof: drive a navigation entirely from a recorded HAR and
//! observe durable identity over the replayed DOM — no network, no live origin.
//!
//! This is the "alive" proof for the replay layer, the transport-touching half
//! of DECISIONS D34 step c. It connects to a live headless Chrome over a plain
//! `ws://` CDP endpoint, loads a self-contained inline-body HAR (the kind
//! anchortree's own [`NetworkCapture`](anchortree_cdp::NetworkCapture) records),
//! starts a [`ReplayFulfiller`](anchortree_cdp::ReplayFulfiller) on the page,
//! navigates to the recorded document URL, and lets the fulfiller answer every
//! `Fetch.requestPaused` from the HAR. The browser never touches the network:
//! every request is served from the recording or honestly failed. Once load
//! settles the fulfiller is closed (interception disabled) and the observe loop
//! runs over the replayed DOM, minting [`Eid`]s — the agent's durable handles on
//! a page it reached without ever hitting a live server.
//!
//! It then proves the thesis on that replayed page, and measures the
//! head-to-head against a modelled Stagehand baseline on the SAME transitions
//! (no live Stagehand — the [`StagehandCache`](anchortree_core::StagehandCache)
//! absolute-XPath resolver from `anchortree-core`, run over the observed DOM):
//!
//! - **Observe 1** mints durable eids over the replayed DOM and binds the
//!   button into a Stagehand-style selector cache (the element the agent acts
//!   on), keyed at its document position.
//! - **In-place re-render** (`window.__atRerender`): the card's children are
//!   rebuilt as fresh DOM nodes in the same order with identical roles + text.
//!   **Observe 2** shows the eids REBIND onto those fresh nodes
//!   ([`diff.rebound`](anchortree_core::Diff)) with **zero** model calls. The
//!   Stagehand resolver pays **zero** self-heals too: positions did not move, so
//!   its cached selector still resolves. This is the honest "rebind without
//!   self-heal" case — the two metrics genuinely differ.
//! - **Reorder** (`window.__atReorder`): the same children, same roles + text,
//!   but the button moves ahead of the paragraph — its document position shifts.
//!   **Observe 3** rebinds the button for free (the accessible name is
//!   unchanged), still **zero** model calls, while the Stagehand resolver's
//!   cached absolute selector now resolves to the wrong node and pays a
//!   **self-heal** (one LLM `page.act`). This is where the LLM-call axis is
//!   **measured**, not asserted: anchortree N rebinds at 0 re-grounds vs
//!   Stagehand M self-heals over the identical re-render.
//!
//! ## Running it
//!
//! Bring up a headless Chrome on the phantom network (no static file server is
//! needed — the bytes come from the HAR):
//!
//! ```text
//! docker run -d --name anchortree-chrome --network phantom_phantom-net \
//!     chromedp/headless-shell:latest
//! ```
//!
//! Capture a self-contained HAR once with the `webarena_capture` example, then
//! replay it:
//!
//! ```text
//! ANCHORTREE_CDP_HTTP=http://<chrome-ip>:9222 \
//!     ANCHORTREE_REPLAY_HAR=/tmp/anchortree-capture-out/network.har \
//!     ANCHORTREE_REPLAY_URL=http://www/index.html \
//!     cargo run -p anchortree-cdp --example webarena_replay
//! ```
//!
//! `ANCHORTREE_REPLAY_URL` is the document URL the HAR recorded; the fulfiller
//! keys requests on URL, so it must match a recorded entry. Connect to Chrome by
//! container **IP**, not hostname (its CDP HTTP endpoint rejects an unknown
//! `Host`, and the `webSocketDebuggerUrl` headless-shell returns is IP-based).

use std::error::Error;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use anchortree_cdp::{ReplayFulfiller, ReplayHar, connect};
use anchortree_core::{
    DomPositions, IdentityMap, ObservationSource as _, RegroundLedger, StagehandCache,
};

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
    // Own a Page handle (Arc-backed clone) so the observe loop can borrow
    // `session.observer` mutably after the fulfiller is done with the page.
    let page = session.observer.page().clone();

    // Start the fulfiller BEFORE navigating: it subscribes to
    // `Fetch.requestPaused` and enables interception for every URL, so the
    // document request itself is answered from the HAR, not the network.
    let fulfiller = ReplayFulfiller::start(&page, har).await?;

    println!("navigating to {target} (served entirely from the HAR)");
    page.goto(&target).await?;
    page.wait_for_navigation().await?;
    // Give late sub-resources a beat to pause and be answered before we stop.
    tokio::time::sleep(Duration::from_millis(400)).await;

    // Close the fulfiller: stop answering, drain, disable interception. After
    // this the page is a static replayed DOM with no interception live.
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

    // --- Observe 1: mint eids over the replayed DOM (Path 3). ---
    // Identity over a page reached with no live origin. We keep the observed
    // nodes around to build the Stagehand baseline's view of the page (the
    // absolute-XPath positions a selector cache would key on) before handing
    // them to the IdentityMap.
    let mut map = IdentityMap::new();
    let mut ledger = RegroundLedger::new();

    let nodes1 = session.observer.observe().await?;
    let positions1 = DomPositions::from_document_order(&nodes1);
    // The agent acts on the one interactive element — the button. A Stagehand
    // resolver caches its absolute selector here for free (it already located it
    // this turn). `act_target` is the element's accessible name, the logical
    // handle both sides reason about.
    let act_target = "Buy now";
    let mut peer = StagehandCache::new();
    peer.bind(act_target, &positions1);
    assert!(
        positions1.xpath_of(act_target).is_some(),
        "the fixture must expose a button named {act_target:?} for the agent to \
         act on; is ANCHORTREE_REPLAY_URL pointing at the m1-site fixture HAR?"
    );

    let diff = map.observe(nodes1).diff;
    ledger.record(&diff);
    println!(
        "observe 1 (replayed DOM): {} elements minted durable eids; Stagehand cached 1 selector",
        diff.added.len()
    );
    assert!(
        !diff.added.is_empty(),
        "observing the replayed DOM should mint at least one Eid; the page the \
         agent reached from the recording carries no observable elements"
    );

    // --- Leg A: IN-PLACE re-render, then observe (Path 2). ---
    // The fixture's OWN inline script (replayed from the HAR, no network)
    // rebuilds the card's children as fresh DOM nodes in the SAME order with
    // identical roles + text. anchortree rebinds across the swap with ZERO model
    // calls. The Stagehand resolver re-tries its cached selector against the new
    // page state and pays ZERO self-heals — positions did not move, so the
    // selector still resolves. The two metrics legitimately differ here: a
    // rebind is not a self-heal.
    eval_rerender(&page, "window.__atRerender", "__atRerender").await?;
    tokio::time::sleep(Duration::from_millis(150)).await;

    let nodes2 = session.observer.observe().await?;
    let positions2 = DomPositions::from_document_order(&nodes2);
    let heals_inplace = peer.reresolve(&positions2);
    let diff2 = map.observe(nodes2).diff;
    ledger.record(&diff2);
    println!(
        "observe 2 (after in-place re-render): {} rebound, {} added, {} changed, {} removed | \
         Stagehand self-heals this leg: {}",
        diff2.rebound.len(),
        diff2.added.len(),
        diff2.changed.len(),
        diff2.removed.len(),
        heals_inplace,
    );
    assert!(
        !diff2.rebound.is_empty(),
        "the in-place re-render must REBIND at least one eid onto a fresh DOM \
         node (diff.rebound was empty); the durable-identity thesis is not \
         proven on replayed infra"
    );
    assert_eq!(
        heals_inplace, 0,
        "the in-place re-render keeps document positions, so a Stagehand \
         absolute-XPath selector must still resolve (0 self-heals); got {heals_inplace}. \
         This is the honest 'rebind without self-heal' leg."
    );

    // --- Leg B: REORDER re-render, then observe (Path 2, the measured win). ---
    // Same children, same roles + text, but the button moves PAST the observed
    // status region to the end of the card: its position among OBSERVED nodes
    // shifts (it crosses the role="status" paragraph; the plain intro <p> is not
    // surfaced, so the button must cross status for the shift to be real).
    // anchortree rebinds it for free (the accessible name is unchanged, clearing
    // REBIND_THRESHOLD), still ZERO model calls. The Stagehand resolver's cached
    // absolute selector now points at the wrong node and must self-heal via an
    // LLM page.act. This is the LLM-call axis MEASURED on one real transition,
    // not asserted in a comment.
    eval_rerender(&page, "window.__atReorder", "__atReorder").await?;
    tokio::time::sleep(Duration::from_millis(150)).await;

    let nodes3 = session.observer.observe().await?;
    let positions3 = DomPositions::from_document_order(&nodes3);
    let heals_reorder = peer.reresolve(&positions3);
    let diff3 = map.observe(nodes3).diff;
    ledger.record(&diff3);
    println!(
        "observe 3 (after reorder): {} rebound, {} added, {} changed, {} removed | \
         Stagehand self-heals this leg: {}",
        diff3.rebound.len(),
        diff3.added.len(),
        diff3.changed.len(),
        diff3.removed.len(),
        heals_reorder,
    );
    assert!(
        !diff3.rebound.is_empty(),
        "the reorder must REBIND the button onto its moved DOM node \
         (diff.rebound was empty); anchortree should follow the element by \
         fingerprint across a position change"
    );
    assert!(
        heals_reorder >= 1,
        "the reorder shifted the cached selector's target, so a Stagehand \
         resolver must pay at least one self-heal here; got {heals_reorder}. \
         Without a measured self-heal the head-to-head is not a real comparison."
    );

    // anchortree's re-ground count is 0 across every leg, by construction.
    assert_eq!(
        ledger.llm_reground_calls(),
        0,
        "anchortree must rebind with zero model calls; the ledger recorded a \
         non-zero LLM re-ground count, which is structurally impossible and \
         signals a regression"
    );

    println!("\n{}", ledger.render());
    println!(
        "head-to-head over the identical re-renders: anchortree {} rebinds at {} LLM re-grounds | \
         Stagehand (absolute-XPath resolver) {} self-heals",
        ledger.rebinds_zero_llm(),
        ledger.llm_reground_calls(),
        peer.self_heals(),
    );
    println!(
        "OK: a page reached ENTIRELY from a recorded HAR re-rendered (in place) and reordered \
         its own DOM; anchortree's durable eids rebound onto the fresh, moved nodes with zero \
         LLM re-grounds, while a Stagehand-style selector cache paid {} self-heal(s) on the same \
         reorder. No live origin was ever touched.",
        peer.self_heals(),
    );
    Ok(())
}

/// Fire one of the fixture's inline re-render hooks and assert it ran. The hook
/// is replayed from the HAR (no network), so a missing hook means the replay URL
/// is not the m1-site fixture.
async fn eval_rerender(
    page: &chromiumoxide::Page,
    expr_fn: &str,
    name: &str,
) -> Result<(), Box<dyn Error>> {
    let ran: bool = page
        .evaluate(format!("{expr_fn} ? {expr_fn}() : false"))
        .await?
        .into_value()?;
    assert!(
        ran,
        "the replayed page did not expose {name}; is ANCHORTREE_REPLAY_URL \
         pointing at the m1-site fixture HAR?"
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
