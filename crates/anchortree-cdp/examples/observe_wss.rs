//! Phase 1.5b proof: observe a page over a **TLS** CDP endpoint (`wss://`).
//!
//! This is the hosted-gateway counterpart to `observe_rerender`, which runs
//! over a plain local `ws://`. The only difference that matters is the
//! transport: this connects through rustls (on the `ring` provider, trusting
//! the bundled `webpki-roots`) to a `wss://` endpoint, then runs the exact same
//! observe -> re-render -> observe loop and asserts the logical [`Eid`]s
//! survive. If the TLS handshake or the durable-identity rebind fail, the
//! example exits non-zero.
//!
//! ## Running it
//!
//! Point it at any `wss://` CDP endpoint via `ANCHORTREE_WSS_URL`.
//!
//! Cloudflare Browser Run (a custom API token with **Browser Rendering - Edit**
//! permission; the token rides the URL or an `Authorization` header per the
//! Cloudflare docs):
//!
//! ```text
//! ANCHORTREE_WSS_URL="wss://api.cloudflare.com/client/v4/accounts/<acct>/browser-rendering/devtools/browser" \
//!     cargo run -p anchortree-cdp --example observe_wss
//! ```
//!
//! Browserbase (the `connectOverCDP` URL from a created session):
//!
//! ```text
//! ANCHORTREE_WSS_URL="wss://connect.browserbase.com?apiKey=<key>&sessionId=<id>" \
//!     cargo run -p anchortree-cdp --example observe_wss
//! ```
//!
//! With no `ANCHORTREE_WSS_URL` set, the example prints these instructions and
//! exits 0, so it is safe to invoke unattended (and it still compiles in CI,
//! which is where the TLS feature wiring is actually proven).

use std::error::Error;

use anchortree_cdp::{connect, is_tls_endpoint};
use anchortree_core::{Eid, IdentityMap, ObservationSource as _};

/// Baseline page: a named landmark holding two buttons and a text input, each
/// carrying a developer-stable `id` (the strongest rebind rung).
const JS_BASELINE: &str = r#"document.body.innerHTML = `<main><h1>Account</h1><button id="save">Save</button><button id="cancel">Cancel</button><input id="email" type="text" aria-label="Email"></main>`; true"#;

/// A full `innerHTML` swap: every child becomes a fresh DOM node with a new
/// `backendNodeId`, the ids unchanged — the churn a framework emits on a state
/// change. The logical ids must survive it as `rebound`.
const JS_RERENDER: &str = r#"document.querySelector('main').innerHTML = `<h1>Account</h1><button id="save">Save changes</button><button id="cancel">Cancel</button><input id="email" type="text" aria-label="Email">`; true"#;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let Some(ws_url) = std::env::var("ANCHORTREE_WSS_URL")
        .ok()
        .filter(|s| !s.is_empty())
    else {
        print_usage();
        return Ok(());
    };

    if !is_tls_endpoint(&ws_url) {
        return Err(format!(
            "ANCHORTREE_WSS_URL must be a wss:// endpoint (got {ws_url:?}); \
             use observe_rerender for plain ws://"
        )
        .into());
    }

    println!("connecting over TLS to {ws_url}");
    let mut session = connect(ws_url).await?;
    println!("TLS handshake ok: connected to the hosted browser over wss://");

    // One identity map spans both observations: it is what carries an Eid
    // forward across the re-render.
    let mut map = IdentityMap::new();

    // --- Observation 1: the baseline page. Everything is first-seen. ---
    session.observer.page().evaluate(JS_BASELINE).await?;
    let obs1 = session.observer.observe().await?;
    let d1 = map.observe(obs1).diff;
    println!("\nobservation 1 (baseline): {} added", d1.added.len());

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
    let d2 = map.observe(obs2).diff;
    println!(
        "observation 2 (after innerHTML swap): {} rebound",
        d2.rebound.len()
    );

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

    println!("\nOK: durable identity survived a real re-render over a TLS (wss://) CDP endpoint.");
    Ok(())
}

fn print_usage() {
    eprintln!(
        "observe_wss: set ANCHORTREE_WSS_URL to a wss:// CDP endpoint to run this proof.\n\
         \n\
         Cloudflare Browser Run:\n  \
         ANCHORTREE_WSS_URL=\"wss://api.cloudflare.com/client/v4/accounts/<acct>/browser-rendering/devtools/browser\"\n\
         Browserbase:\n  \
         ANCHORTREE_WSS_URL=\"wss://connect.browserbase.com?apiKey=<key>&sessionId=<id>\"\n\
         \n\
         No URL set, nothing to do. Exiting 0."
    );
}
