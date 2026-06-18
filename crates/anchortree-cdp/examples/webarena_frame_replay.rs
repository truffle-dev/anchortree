//! Phase 3.2f-live: the browser-tied twin of the CI-gated frame-tier
//! head-to-head (`crates/anchortree-core/src/peer.rs` `FrameOrdinalCache`,
//! DECISIONS D41). It is the FRAME-tier analogue of `webarena_replay.rs`: the
//! interactive element lives one frame down, inside a same-origin `srcdoc`
//! iframe, and the measured transition is a sibling-frame insert that shifts
//! the tracked frame's document-order ordinal.
//!
//! Like its node-tier sibling it drives the whole page entirely from a recorded
//! HAR - no network, no live origin. srcdoc frames are pierced inline by the
//! CDP adapter and carry no request of their own, so the parent document plus
//! both frames' markup all come from a single self-contained inline-body HAR
//! (the kind [`NetworkCapture`](anchortree_cdp::NetworkCapture) records). It
//! connects over a plain `ws://` CDP endpoint, starts a
//! [`ReplayFulfiller`](anchortree_cdp::ReplayFulfiller), navigates to the
//! recorded document URL, closes the fulfiller, and runs the observe loop over
//! the replayed cross-frame DOM, minting frame-namespaced [`Eid`]s.
//!
//! It then proves the thesis at the frame tier and measures the head-to-head
//! against a modelled Stagehand `frameOrdinal` resolver on the SAME transitions
//! (no live Stagehand - the
//! [`FrameOrdinalCache`](anchortree_core::FrameOrdinalCache) from
//! `anchortree-core`, fed the fixture's ground-truth frame-owner order):
//!
//! - **Observe 1** mints durable eids over the replayed cross-frame DOM. The
//!   checkout frame keys on its owner's `name="checkout"` (D40), so its button
//!   is `fcheckout/btn-buy-now`. A Stagehand `frameOrdinal` resolver binds the
//!   checkout frame at document-order ordinal 0.
//! - **Inner-frame churn** (`window.__atFrameRerender`): the checkout frame's
//!   card is rebuilt as fresh DOM nodes in the same order with identical roles +
//!   text. **Observe 2** shows the button's eid REBIND with **zero** model
//!   calls. The frame-ordinal resolver pays **zero** re-grounds: the frame tree
//!   is unchanged, so the checkout frame still sits at ordinal 0.
//! - **Frame-owner reorder** (`window.__atFrameReorder`): a sibling
//!   `name="ads"` iframe is inserted BEFORE the checkout owner, shifting the
//!   checkout frame's ordinal from 0 to 1. The insert does not touch the
//!   checkout frame's own document, so its button keeps both its
//!   `backendNodeId` and its frame discriminator key `checkout`. **Observe 3**
//!   therefore keeps the checkout button's eid bound with **zero churn** - not
//!   removed, not re-minted - and still **zero** model calls. Had the frame
//!   been keyed by document-order ordinal, the shift 0 -> 1 would have dropped
//!   the old `f0/...` eid and minted a fresh `f1/...` one. The frame-ordinal
//!   resolver's cached ordinal now resolves the wrong frame and pays a
//!   **re-ground**. This is the frame-tier LLM-call axis **measured**, not
//!   asserted: anchortree holds the eid at 0 re-grounds vs a frame-ordinal
//!   resolver's M re-grounds over the identical frame reorder.
//!
//! ## Running it
//!
//! Use `scripts/run-once-frame.sh`, which stands up a headless Chrome, captures
//! a self-contained HAR of `scripts/fixtures/frame-site/index.html` with the
//! `webarena_capture` example, then replays it through this one. The manual
//! shape mirrors `webarena_replay.rs`:
//!
//! ```text
//! ANCHORTREE_CDP_HTTP=http://<chrome-ip>:9222 \
//!     ANCHORTREE_REPLAY_HAR=/tmp/anchortree-frame-out/network.har \
//!     ANCHORTREE_REPLAY_URL=http://127.0.0.1:8081/index.html \
//!     cargo run -p anchortree-cdp --example webarena_frame_replay
//! ```

use std::error::Error;
use std::io::{Read as _, Write as _};
use std::net::TcpStream;
use std::time::Duration;

use anchortree_cdp::{ReplayFulfiller, ReplayHar, connect};
use anchortree_core::{
    Eid, FrameOrder, FrameOrdinalCache, IdentityMap, ObservationSource as _, RegroundLedger,
};

/// The eid prefix the checkout frame's elements mint under: the CDP adapter
/// keys the `name="checkout"` srcdoc owner as discriminator `checkout`, and the
/// IdentityMap namespaces in-frame eids as `f<framekey>/<local>`.
const CHECKOUT_PREFIX: &str = "fcheckout/";

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

    let fulfiller = ReplayFulfiller::start(&page, har).await?;

    println!("navigating to {target} (parent + frames served entirely from the HAR)");
    page.goto(&target).await?;
    page.wait_for_navigation().await?;
    tokio::time::sleep(Duration::from_millis(400)).await;

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

    // The peer is a modelled Stagehand `frameOrdinal` resolver. Its view of the
    // page is the document-order list of frame-owner discriminators - the
    // fixture's ground truth, exactly as `webarena_replay.rs` hardcodes the
    // act target "Buy now". Initially the checkout frame is the only frame and
    // sits at ordinal 0.
    let owners_initial = ["checkout"];
    let owners_reordered = ["ads", "checkout"];
    let mut peer = FrameOrdinalCache::new();

    let mut map = IdentityMap::new();
    let mut ledger = RegroundLedger::new();

    // --- Observe 1: mint frame-namespaced eids over the replayed cross-frame DOM. ---
    let nodes1 = session.observer.observe().await?;
    let diff1 = map.observe(nodes1).diff;
    ledger.record(&diff1);
    let order1 = FrameOrder::from_owner_order(&owners_initial);
    peer.bind("checkout", &order1);
    println!(
        "observe 1 (replayed cross-frame DOM): {} elements minted durable eids; \
         frame-ordinal resolver cached the checkout frame at ordinal {}",
        diff1.added.len(),
        order1
            .ordinal_of("checkout")
            .expect("checkout frame is present"),
    );
    assert!(
        has_frame_eid(&diff1.added, CHECKOUT_PREFIX),
        "observing the replayed DOM must mint the checkout frame's button under \
         {CHECKOUT_PREFIX:?}; none found. Is ANCHORTREE_REPLAY_URL pointing at the \
         frame-site fixture HAR, and is the srcdoc owner keyed by name=\"checkout\"?"
    );
    // Pin the checkout frame's button as the durable handle the agent reasons
    // about, and track that exact eid across both transitions.
    let checkout_button_eid = diff1
        .added
        .iter()
        .find(|e| e.0.starts_with("fcheckout/btn"))
        .cloned()
        .expect("the checkout frame must mint a button eid under fcheckout/btn");

    // --- Leg A: INNER-FRAME churn, then observe. ---
    // The checkout frame's card is rebuilt in place (same order, identical roles
    // + text). The button's eid rebinds with ZERO model calls. The frame tree is
    // unchanged, so a frame-ordinal resolver re-resolves the checkout frame at
    // the same ordinal 0 and pays ZERO re-grounds.
    eval_hook(&page, "window.__atFrameRerender", "__atFrameRerender").await?;
    tokio::time::sleep(Duration::from_millis(150)).await;

    let nodes2 = session.observer.observe().await?;
    let diff2 = map.observe(nodes2).diff;
    ledger.record(&diff2);
    let order2 = FrameOrder::from_owner_order(&owners_initial);
    let regrounds_inner = peer.reresolve(&order2);
    println!(
        "observe 2 (after inner-frame churn): {} rebound, {} added, {} changed, {} removed | \
         frame-ordinal re-grounds this leg: {}",
        diff2.rebound.len(),
        diff2.added.len(),
        diff2.changed.len(),
        diff2.removed.len(),
        regrounds_inner,
    );
    assert!(
        diff2.rebound.contains(&checkout_button_eid),
        "the inner-frame churn must REBIND the checkout button's eid ({checkout_button_eid}) \
         onto a fresh DOM node (not in diff.rebound); the durable-identity thesis is \
         not proven at the frame tier on replayed infra"
    );
    assert_eq!(
        regrounds_inner, 0,
        "the inner-frame churn leaves the frame tree intact, so a frame-ordinal \
         resolver must still resolve the checkout frame at ordinal 0 (0 re-grounds); \
         got {regrounds_inner}. This is the honest 'rebind without re-ground' leg."
    );

    // --- Leg B: FRAME-OWNER reorder, then observe (the measured win). ---
    // A sibling name="ads" iframe is inserted BEFORE the checkout owner. The
    // checkout frame's ordinal shifts 0 -> 1, but the insert does not touch the
    // checkout frame's own document: the button keeps its backendNodeId AND its
    // discriminator key "checkout", so the soft-match index still hits and the
    // eid stays bound with ZERO churn (not removed, not re-minted), still ZERO
    // model calls. The ads frame's button mints as an added element under
    // fads/. The frame-ordinal resolver's cached ordinal 0 now points at the ads
    // frame and must pay one re-ground.
    eval_hook(&page, "window.__atFrameReorder", "__atFrameReorder").await?;
    tokio::time::sleep(Duration::from_millis(150)).await;

    let nodes3 = session.observer.observe().await?;
    let diff3 = map.observe(nodes3).diff;
    ledger.record(&diff3);
    let order3 = FrameOrder::from_owner_order(&owners_reordered);
    let regrounds_reorder = peer.reresolve(&order3);
    println!(
        "observe 3 (after frame-owner reorder): {} rebound, {} added, {} changed, {} removed | \
         frame-ordinal re-grounds this leg: {}",
        diff3.rebound.len(),
        diff3.added.len(),
        diff3.changed.len(),
        diff3.removed.len(),
        regrounds_reorder,
    );
    // The frame-tier durability win: the checkout button's eid is NEITHER
    // dropped NOR re-minted across the sibling-frame insert. Had the frame been
    // keyed by document-order ordinal, the shift 0 -> 1 would have removed the
    // old `f0/...` eid and minted a fresh `f1/...` one.
    assert!(
        !diff3.removed.contains(&checkout_button_eid),
        "the frame reorder must NOT drop the checkout button's eid \
         ({checkout_button_eid}); it appeared in diff.removed, which means the frame \
         key shifted with the ordinal instead of holding on the discriminator"
    );
    assert!(
        !diff3.added.contains(&checkout_button_eid),
        "the checkout button's eid ({checkout_button_eid}) must stay stable across the \
         frame reorder, not re-mint; it reappeared in diff.added"
    );
    let still_bound = map
        .binding(&checkout_button_eid)
        .expect("the checkout button's eid must still be live in the map after the frame reorder");
    assert_eq!(
        still_bound.frame_key.0, "checkout",
        "the checkout button must still be keyed by its owner discriminator \"checkout\" \
         after a sibling frame was inserted ahead of it; got {:?}",
        still_bound.frame_key.0,
    );
    assert!(
        has_frame_eid(&diff3.added, "fads/"),
        "the frame reorder inserts a distinctly-identified name=\"ads\" frame whose \
         button should mint under fads/; none found in diff.added"
    );
    assert_eq!(
        regrounds_reorder, 1,
        "the sibling-frame insert shifted the checkout frame's ordinal 0 -> 1, so a \
         frame-ordinal resolver cached at ordinal 0 must pay exactly one re-ground; \
         got {regrounds_reorder}. Without a measured re-ground the head-to-head is \
         not a real comparison."
    );

    // anchortree's re-ground count is 0 across every leg, by construction.
    assert_eq!(
        ledger.llm_reground_calls(),
        0,
        "anchortree must rebind with zero model calls at the frame tier; the ledger \
         recorded a non-zero LLM re-ground count, which is structurally impossible \
         and signals a regression"
    );

    println!("\n{}", ledger.render());
    println!(
        "frame-tier head-to-head over the identical re-renders: anchortree {} rebinds at {} LLM \
         re-grounds | frame-ordinal resolver {} re-grounds",
        ledger.rebinds_zero_llm(),
        ledger.llm_reground_calls(),
        peer.regrounds(),
    );
    println!(
        "OK: a cross-frame page reached ENTIRELY from a recorded HAR churned its checkout frame's \
         card (eid rebound onto fresh nodes) and then had a sibling ad frame inserted ahead of it \
         (eid held bound with zero churn, still keyed \"checkout\"), both at zero LLM re-grounds, \
         while a Stagehand-style frame-ordinal resolver paid {} re-ground(s) on the frame reorder. \
         No live origin was ever touched.",
        peer.regrounds(),
    );
    Ok(())
}

/// Whether any eid in `eids` lives in the frame whose namespace is `prefix`
/// (e.g. `"fcheckout/"`).
fn has_frame_eid(eids: &[Eid], prefix: &str) -> bool {
    eids.iter().any(|e| e.0.starts_with(prefix))
}

/// Fire one of the fixture's inline transition hooks and assert it ran. The
/// hook is replayed from the HAR (no network), so a missing hook means the
/// replay URL is not the frame-site fixture.
async fn eval_hook(
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
         pointing at the frame-site fixture HAR?"
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
