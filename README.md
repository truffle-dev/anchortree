# anchortree

**An agent's non-determinism in a browser is an identity problem, not a
rendering problem.** anchortree gives every interactive element a durable
handle that survives re-renders, and hands the agent a token-cheap **diff**
each turn instead of a fresh snapshot — so it never re-grounds.

> Status: early. The identity core and the live CDP action path are built and
> green; `wss://`/Browserbase and the benchmark are next. Pre-1.0, APIs move.

Today's agents lose the page constantly. An agent clicks a button, a framework
re-renders, the DOM node it was holding is destroyed and recreated, and the
agent has to re-snapshot and re-ground from scratch. That re-grounding is the
expensive, non-deterministic part of every agent-browser loop — and it costs on
two axes at once: the LLM tokens to re-read the page, and the billable
browser-minutes spent round-tripping while it does.

anchortree keeps one logical handle — "the Sign in button" is `btn-sign-in` —
across the agent's own clicks *and* a re-render that swaps the underlying DOM
node. No re-snapshot. No LLM re-grounding call. The handle just keeps working.

## Quickstart

Point anchortree at any CDP endpoint. A throwaway headless Chrome on the same
Docker network is enough:

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
    // One-line connect: a CDP WebSocket URL is all anchortree needs.
    let mut session = connect("ws://127.0.0.1:9222/devtools/browser/<id>").await?;
    let mut map = IdentityMap::new();

    // Observe once. `obs.render()` is the exact compact text an agent reads;
    // a full ~40-element page baseline costs ~200 tokens, not the 15K-35K of a
    // raw accessibility dump.
    let obs = map.observe(session.observer.observe().await?);
    println!("{}", obs.render());
    assert!(budget::observation_within_budget(&obs)); // <= 5,000 tokens

    // Act on a durable handle. `btn-sign-in` was minted from the element's
    // role + accessible name, so it is stable and human-legible.
    let sign_in = anchortree_core::Eid("btn-sign-in".into());
    act(session.observer.page(), &map, &sign_in, Action::Click).await?;

    // The framework re-renders: every control is now a brand-new DOM node with
    // a fresh backendNodeId. Re-observe — but only to learn what *changed*.
    let diff = map.observe(session.observer.observe().await?).diff;
    println!("{}", diff.render()); // "* btn-sign-in" — rebound, not re-grounded

    // The payoff: act on the SAME handle again. No re-snapshot, no LLM call to
    // re-locate it. anchortree rebound btn-sign-in onto the new node for you.
    act(session.observer.page(), &map, &sign_in, Action::Click).await?;
    Ok(())
}
```

That last block is the whole thesis: **act → re-render → act on the same id**,
with nothing re-grounded in between. No peer's hello-world does this. The
runnable version is `examples/act_after_rerender.rs` (live against a real
browser, the click lands as `isTrusted: true`).

The diff an agent reads each turn is just sigil-prefixed lines:

```text
+ st-toast            # added
- btn-old             # removed
* btn-sign-in         # rebound — same logical button, brand-new DOM node
~ st-cart-count: 3 items   # text changed
```

## How it works

1. **Durable identity.** CDP `backendNodeId` is the cheap primary key while a
   DOM node lives. When a node is destroyed and recreated, anchortree rebinds
   the logical id to the new node by scoring a content fingerprint — stable
   attribute, then (role, accessible-name), then a landmark-scoped structural
   path. The agent's handle never breaks.
2. **Diff observations.** One full baseline, then deltas: `added` / `removed` /
   `rebound` / `changed`. Polling the page every turn stays nearly free
   (a steady turn is tens of tokens), with hard guardrails in `budget`
   (`<= 5,000` baseline, `<= 800` per diff).
3. **Any CDP browser.** The core (`anchortree-core`) operates on plain
   observation values behind an `ObservationSource` trait, so it is browser-free
   and fully unit-tested without driving Chrome. It runs over any CDP endpoint:
   local Chrome, Lightpanda, Browserbase, Cloudflare Browser Rendering.

See `docs/DESIGN.md` for the architecture and `ROADMAP.md` for what is built and
what is next.

## anchortree vs the field

Every other agent-browser library re-snapshots the whole accessibility tree
each step and re-grounds when the page changes. That is the cost anchortree
removes — and it is removed on **both** axes: fewer LLM tokens *and* fewer
billable browser-minutes (managed browsers bill per session-minute, so every
saved round-trip is real money).

- **Playwright MCP** invalidates element refs on any change — the docs say so
  verbatim: *"refs are invalidated when the page changes"* and *"re-snapshot
  after navigation"* (playwright.dev/mcp/snapshots). Persisting element
  identity was proposed and **declined** for performance reasons
  (microsoft/playwright-mcp#1488, `NOT_PLANNED`).
- **Stagehand** keys nodes with an `EncodedId` of the form
  `frameOrdinal-backendNodeId` — snapshot-scoped by construction — and
  re-grounds through an LLM `observe` call when the snapshot turns over. It
  ships two caches: an absolute-selector resolver that re-tries a cached XPath,
  and a higher-level selector cache keyed on a DOM-hash fingerprint that **falls
  back to the model the moment the hash drifts**
  (browserbase.com/blog/stagehand-caching). The first is the one anchortree is
  measured against below; the DOM-hash cache is coarser still (it re-grounds on
  whole-page drift, even for nodes that did not move).
- **browser-use** addresses elements by per-snapshot integer indices that shift
  when the page re-renders (browser-use#1686).

anchortree's id is durable *across* snapshots, and the rebind is pure scoring —
no inference call. The "only what changed" diff is a feature none of these ship.

This is **measured offline**, not asserted: `scripts/run-once-m1.sh` reaches a
page entirely from a recorded HAR (no live origin) and runs anchortree side by
side with a modelled Stagehand absolute-XPath resolver over two re-renders of the
same page (`crates/anchortree-cdp/examples/webarena_replay.rs`):

- **In-place re-render** (fresh DOM nodes, same positions): anchortree rebinds
  the eids at **0 LLM re-grounds**; the Stagehand resolver pays **0 self-heals**
  too, because the cached selector still resolves. The two metrics honestly
  differ here — a rebind is not a self-heal.
- **Reorder** (the button moves to the end of the card, past the status node):
  anchortree still rebinds the button for free at **0 LLM re-grounds**, while the
  Stagehand resolver's cached absolute selector now points at the wrong node and
  pays a **self-heal** — one LLM `page.act`. This is the LLM-call axis as a number
  on a real transition, not a sentence.

Live numbers from the rail: **anchortree 4 rebinds at 0 LLM re-grounds** across
both legs; **Stagehand 0 self-heals on the in-place leg, 1 on the reorder**.

## CDP today, BiDi-compatible by design

anchortree speaks Chrome DevTools Protocol today, because that is what Chromium
agent work runs on. WebDriver BiDi is maturing as the cross-browser transport,
and the engine is ready for it: `anchortree-core` never sees a CDP type — it
sits behind the transport-neutral `ObservationSource` trait — so an
`anchortree-bidi` adapter is a clean, additive axis, not a rewrite.

## Build

```text
cargo test                      # core + cdp + integration + doctests, all green
cargo clippy --all-targets      # clean (CI runs -D warnings)
```

The CDP examples need a reachable browser; pass one via `ANCHORTREE_CDP_WS` or
`ANCHORTREE_CDP_HTTP` (see each example's header).

## License

Dual-licensed under either of

- Apache License, Version 2.0 (`LICENSE-APACHE`)
- MIT license (`LICENSE-MIT`)

at your option.
