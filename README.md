# anchortree

An agent-first browser interface: **durable element identity** and **diff
observations** over any CDP browser.

> Status: early. The pure-logic identity core is built and green; the CDP
> plumbing is in progress. Pre-1.0, APIs will move.

## The idea

An agent driving a browser keeps losing track of the page. It clicks a button,
a framework re-renders, the DOM node it was holding is destroyed and recreated,
and the agent has to screenshot and re-ground from scratch. That re-grounding
is the expensive, non-deterministic part of every agent-browser loop.

anchortree's claim: **this is an identity problem, not a rendering problem.**

A logical element — "the Sign in button" — should keep one durable handle
across the agent's own clicks *and* a re-render that swaps the underlying DOM
node. anchortree gives every interactive element a stable id (`btn-sign-in`,
`inp-email`) that survives those mutations, and hands the agent a token-cheap
**diff** each turn instead of a fresh screenshot:

```
added:   []
removed: []
changed: [{ eid: "st-cart-count", text: "3 items" }]
rebound: [btn-checkout]   # same logical button, brand-new DOM node
```

The agent never re-grounds. Its handles just keep working.

## How it works

- **Durable identity.** CDP `backendNodeId` is the cheap primary key while a
  DOM node lives. When a node is destroyed and recreated, anchortree rebinds
  the logical id to the new node by scoring a content fingerprint — stable
  attribute, then (role, accessible-name), then structural path, then geometry.
- **Diff observations.** One full baseline, then deltas. `added` / `removed` /
  `changed` / `rebound`. Cheap to read, turn after turn.
- **Browser-agnostic.** The core operates on plain observation values, so it
  runs over any CDP endpoint: local Chrome, Lightpanda, Browserbase, Cloudflare
  Browser Run.

The identity core (`anchortree-core`) is deliberately browser-free and fully
unit-tested without driving Chrome. See `docs/DESIGN.md` for the full
architecture and `ROADMAP.md` for what is built and what is next.

## Build

```
cargo test          # 16 passing, including the rebind-on-hard-render scenario
cargo clippy        # clean
```

## License

Dual-licensed under either of

- Apache License, Version 2.0 (`LICENSE-APACHE`)
- MIT license (`LICENSE-MIT`)

at your option.
