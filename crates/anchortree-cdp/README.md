# anchortree-cdp

**The Chrome DevTools Protocol adapter for [anchortree](https://github.com/truffle-dev/anchortree).**

[`anchortree-core`](https://crates.io/crates/anchortree-core) is a browser-free
durable-identity engine: stable element handles that rebind across re-renders,
plus token-cheap diff observations. `anchortree-cdp` is the half that drives a
live browser. It connects over CDP, fuses the accessibility tree, DOM, and
layout into one observation, and dispatches real actions — a click lands as
`isTrusted: true`.

Point it at any CDP endpoint: local Chrome, Lightpanda, Browserbase, Cloudflare
Browser Rendering. A `wss://` URL upgrades to TLS with no extra setup
(webpki-roots are bundled, so no system cert store is needed in a container).

## Quickstart

```text
docker run -d --name anchortree-chrome --network <your-net> \
    chromedp/headless-shell:latest
# then read ws://<container-ip>:9222/devtools/browser/<id> from /json/version
```

```rust
use anchortree_cdp::{Action, act, connect};
use anchortree_core::{IdentityMap, ObservationSource as _, budget};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // A CDP WebSocket URL is all anchortree needs.
    let mut session = connect("ws://127.0.0.1:9222/devtools/browser/<id>").await?;
    let mut map = IdentityMap::new();

    // Observe once. A ~40-element page baseline costs ~200 tokens, not the
    // 15K-35K of a raw accessibility dump.
    let obs = map.observe(session.observer.observe().await?);
    println!("{}", obs.render());
    assert!(budget::observation_within_budget(&obs));

    // Act on a durable handle, minted from role + accessible name.
    let sign_in = anchortree_core::Eid("btn-sign-in".into());
    act(session.observer.page(), &map, &sign_in, Action::Click).await?;

    // The framework re-renders every control into a brand-new DOM node.
    // Re-observe only to learn what changed.
    let diff = map.observe(session.observer.observe().await?).diff;
    println!("{}", diff.render()); // "* btn-sign-in" — rebound, not re-grounded

    // Act on the SAME handle again. No re-snapshot, no LLM re-locate call.
    act(session.observer.page(), &map, &sign_in, Action::Click).await?;
    Ok(())
}
```

That is the whole thesis: **act → re-render → act on the same id**, with nothing
re-grounded in between. The runnable version is
`examples/act_after_rerender.rs`, live against a real browser.

## Transport notes

- **CDP today, BiDi-compatible by design.** `anchortree-core` never sees a CDP
  type, so a WebDriver BiDi adapter is an additive axis, not a rewrite.
- **Hosted gateways.** The session-acquire step (Cloudflare Browser Run mint,
  Browserbase create-session) speaks REST over `reqwest`; the CDP transport runs
  on `chromiumoxide`. rustls is pinned to the `ring` crypto provider.

The CDP examples need a reachable browser; pass one via `ANCHORTREE_CDP_WS` or
`ANCHORTREE_CDP_HTTP` (see each example's header).

## Status

Early. Pre-1.0, the API moves. See the
[workspace README](https://github.com/truffle-dev/anchortree) for the full
thesis and the offline benchmark.

## License

Dual-licensed under either of Apache License, Version 2.0 or the MIT license, at
your option.
