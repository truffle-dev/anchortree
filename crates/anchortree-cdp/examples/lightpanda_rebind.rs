//! Phase 5.2 portability proof: durable identity across a re-render, driven over
//! a **non-Chromium CDP engine** — [Lightpanda](https://github.com/lightpanda-io/browser),
//! a from-scratch headless browser written in Zig.
//!
//! Every other live example in this crate runs against a Chromium build
//! (`chromedp/headless-shell`, Browserbase, Cloudflare Browser Run). This one
//! proves the same mint → re-render → rebind loop against an engine that shares
//! none of Chromium's code, only the CDP wire contract. If anchortree's identity
//! survives here, the durable-handle thesis is a property of the *protocol*, not
//! of one vendor's renderer.
//!
//! ## Why the flat connect leg, not [`connect`](anchortree_cdp::connect)
//!
//! Lightpanda implements `Page`, `DOM`, and `Accessibility` but **does not
//! implement `Runtime.enable`** (it times out). chromiumoxide's
//! [`connect`](anchortree_cdp::connect) opens a page through
//! `Browser::connect` + `new_page`, which primes the Runtime domain and so hangs
//! on Lightpanda. [`connect_hosted`] sidesteps that entirely: it flat-attaches to
//! the page the browser already has open and enables only Accessibility + DOM —
//! exactly the two domains the observation pipeline reads. That is why the
//! portability lane runs over the hosted leg.
//!
//! ## Why a server-driven re-render, not `innerHTML`
//!
//! [`connect_hosted`](../connect_hosted.rs) and [`act_after_rerender`](../act_after_rerender.rs)
//! force their re-render with `Runtime.evaluate` (an in-page `innerHTML` swap).
//! Lightpanda does not run `Runtime.evaluate` either, so this example re-renders
//! the only way that needs no script: it **navigates to a second document**. Both
//! fixtures are `data:` URLs describing the same three controls (a toggle button,
//! an email field, a size `<select>`), each carrying a stable `id` — the
//! strongest rebind rung. The second navigation throws away every DOM node and
//! builds fresh ones with new `backendNodeId`s, which is a strictly *harder*
//! re-render than an `innerHTML` swap: identity has to survive a whole new
//! document, not just a new subtree.
//!
//! ## The action-layer boundary (reported, not asserted)
//!
//! After the rebind, the example attempts one trusted [`Action::Click`] on the
//! rebound toggle and *reports* the outcome without asserting it. Lightpanda
//! accepts `Input.dispatchMouseEvent` over the wire but does not yet run the
//! element's event handlers, and it answers `DOM.focus` / `Runtime.*` with
//! `UnknownMethod`. So the trusted-consequence proof stays where it can be
//! verified — [`act_after_rerender`](../act_after_rerender.rs) against Chrome —
//! while this example surfaces, live, exactly how far the action leg reaches on a
//! partial engine.
//!
//! ## Running it
//!
//! Lightpanda advertises its `webSocketDebuggerUrl` as the unroutable
//! `ws://0.0.0.0:9222/`, so the URL must be passed explicitly (there is no
//! `/json/version` derive path here). Its CDP socket lives at the root path `/`,
//! not `/devtools/browser/<id>`:
//!
//! ```text
//! docker run -d --name lightpanda --network phantom_phantom-net \
//!     lightpanda/browser:nightly
//! ANCHORTREE_CDP_WS=ws://<container-ip>:9222/ \
//!     cargo run -p anchortree-cdp --example lightpanda_rebind
//! ```
//!
//! With `ANCHORTREE_CDP_WS` unset it prints usage and exits 0, so it is
//! unattended-safe and still type-checks the whole portability path in CI.

use std::error::Error;

use anchortree_cdp::{ActError, Action, connect_hosted};
use anchortree_core::{Diff, Eid, IdentityMap, ObservationSource as _};

/// Baseline settings page as a `data:` document: a toggle button, an email
/// field, and a size `<select>` defaulting to `medium`. Each control carries a
/// stable `id` (the strongest rebind rung) and an explicit accessible name so
/// the engine's AX tree is deterministic.
const FIXTURE_A: &str = r#"data:text/html,<!doctype html><html><body><main><h1>Settings</h1><button id="toggle" aria-label="Toggle">Toggle</button><input id="email" type="text" aria-label="Email"><select id="size" role="combobox" aria-label="Size"><option value="small">Small</option><option value="medium" selected>Medium</option><option value="large">Large</option></select><p id="status" role="status">Off</p></main></body></html>"#;

/// The re-render: a *different document* describing the same three controls.
/// Every node is rebuilt from scratch with a fresh `backendNodeId`; the ids are
/// unchanged and the toggle's label is nudged to show a content change folds
/// into the rebind rather than surfacing as a separate add/remove.
const FIXTURE_B: &str = r#"data:text/html,<!doctype html><html><body><main><h1>Settings</h1><button id="toggle" aria-label="Toggle">Toggle setting</button><input id="email" type="text" aria-label="Email"><select id="size" role="combobox" aria-label="Size"><option value="small">Small</option><option value="medium" selected>Medium</option><option value="large">Large</option></select><p id="status" role="status">Off</p></main></body></html>"#;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let Some(ws_url) = resolve_ws_url() else {
        print_usage();
        return Ok(());
    };

    println!("connecting (hosted leg) to Lightpanda at {ws_url}");
    let mut session = connect_hosted(&ws_url).await?;
    println!("flat-attached to the page Lightpanda already had open");

    // One identity map spans both observations: it is what carries an Eid
    // forward across the re-render.
    let mut map = IdentityMap::new();

    // --- Observation 1: the baseline document. Everything is first-seen. ---
    session.navigate(FIXTURE_A).await?;
    let obs1 = session.observer.observe().await?;
    let d1 = map.observe(obs1).diff;
    print_diff("observation 1 (baseline document)", &d1);

    let toggle = find_eid(&d1.added, "toggle").expect("baseline mints a toggle eid");
    let email = find_eid(&d1.added, "email").expect("baseline mints an email eid");
    let size = find_eid(&d1.added, "size").expect("baseline mints a size eid");
    let handles = [("toggle", &toggle), ("email", &email), ("size", &size)];
    println!("  handles: toggle={toggle}, email={email}, size={size}");

    let before: Vec<(String, i64)> = handles
        .iter()
        .map(|(name, eid)| {
            let backend = map
                .binding(eid)
                .expect("just-added eid has a binding")
                .backend_node_id;
            ((*name).to_string(), backend)
        })
        .collect();

    // --- Observation 2: a whole new document. New nodes, same identities. ---
    session.navigate(FIXTURE_B).await?;
    let obs2 = session.observer.observe().await?;
    let d2 = map.observe(obs2).diff;
    print_diff("observation 2 (after navigating to a fresh document)", &d2);

    println!("  rebind ledger:");
    for ((name, eid), (_, old_backend)) in handles.iter().zip(before.iter()) {
        assert!(
            d2.rebound.contains(eid),
            "{name} ({eid}) should survive the re-render as a rebind, not a remove+add"
        );
        let new_backend = map
            .binding(eid)
            .expect("rebound eid still has a binding")
            .backend_node_id;
        assert_ne!(
            *old_backend, new_backend,
            "{name} ({eid}) should be re-bound to a brand-new DOM node"
        );
        println!("    {name}: backendNodeId {old_backend} -> {new_backend} (identity held)");
    }
    assert!(
        d2.added.is_empty() && d2.removed.is_empty(),
        "a pure re-render of the same three controls adds and removes nothing"
    );
    println!("  all three controls rebound onto fresh DOM nodes across a full navigation");

    // --- Action-layer boundary: dispatch one trusted click, report the reach. ---
    //
    // This is deliberately *not* asserted. Lightpanda accepts the Input command
    // but does not run the click handler, so there is no consequence to observe;
    // the trusted-consequence proof lives in `act_after_rerender` (Chrome). What
    // this surfaces, live, is exactly how far anchortree's action leg reaches on
    // a partial CDP engine.
    println!("\naction-layer boundary (reported, not asserted):");
    match session.observer.act(&map, &toggle, Action::Click).await {
        Ok(()) => println!(
            "  act(click, toggle) dispatched over the flat session without a protocol error"
        ),
        Err(ActError::Cdp(e)) => {
            println!("  act(click, toggle) reached a CDP method Lightpanda does not implement: {e}")
        }
        Err(e) => println!("  act(click, toggle) returned: {e}"),
    }

    println!(
        "\nOK: logical identity survived a real re-render against Lightpanda, \
         a non-Chromium CDP engine."
    );
    Ok(())
}

/// Resolve the Lightpanda CDP WebSocket URL from `ANCHORTREE_CDP_WS`.
///
/// Unlike the Chromium examples there is no `/json/version` derive path:
/// Lightpanda advertises an unroutable `ws://0.0.0.0:9222/`, so the caller must
/// pass the reachable URL directly.
fn resolve_ws_url() -> Option<String> {
    std::env::var("ANCHORTREE_CDP_WS")
        .ok()
        .filter(|s| !s.is_empty())
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

fn print_usage() {
    eprintln!(
        "lightpanda_rebind: set ANCHORTREE_CDP_WS to run the non-Chromium portability proof.\n\
         \n\
         Lightpanda (Zig CDP engine, ws:// at the root path):\n  \
         docker run -d --name lightpanda --network phantom_phantom-net lightpanda/browser:nightly\n  \
         ANCHORTREE_CDP_WS=ws://<container-ip>:9222/ \\\n    \
         cargo run -p anchortree-cdp --example lightpanda_rebind\n\
         \n\
         No endpoint configured, nothing to do. Exiting 0."
    );
}
