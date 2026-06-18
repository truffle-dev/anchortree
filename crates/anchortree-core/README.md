# anchortree-core

**The browser-free durable-identity engine for [anchortree](https://github.com/truffle-dev/anchortree).**

An agent's non-determinism in a browser is an identity problem, not a rendering
problem. `anchortree-core` is the part that solves the identity half: it gives
every interactive element a stable, human-legible handle that rebinds across
re-renders, and it turns each turn's page state into a token-cheap **diff**
instead of a fresh snapshot — so an agent never re-grounds.

This crate is pure. It has **no dependencies** and never touches a browser. It
operates on plain observation values behind the `ObservationSource` trait, which
means the whole identity engine is unit-tested without driving Chrome. To run it
over a live browser, pair it with a transport adapter such as
[`anchortree-cdp`](https://crates.io/crates/anchortree-cdp).

## What it does

1. **Durable identity.** A logical id — "the Sign in button" is `btn-sign-in` —
   stays bound to one element across the agent's own clicks *and* a re-render
   that destroys and recreates the underlying DOM node. When a node is replaced,
   the id rebinds to the new node by scoring a content fingerprint: a stable
   attribute first, then `(role, accessible-name)`, then a landmark-scoped
   structural path. The handle never breaks; the rebind is pure scoring, no
   inference call.
2. **Diff observations.** One full baseline, then deltas — `added` / `removed` /
   `rebound` / `changed`. A steady turn costs tens of tokens, not the 15K–35K of
   a raw accessibility dump.
3. **Hard budgets.** The `budget` module enforces the guardrails: a baseline
   stays `<= 5,000` tokens, each diff `<= 800`.

## The diff an agent reads

```text
+ st-toast            # added
- btn-old             # removed
* btn-sign-in         # rebound — same logical button, brand-new DOM node
~ st-cart-count: 3 items   # text changed
```

## Public surface

- `IdentityMap` — mints and rebinds element ids across observations.
- `Observation`, `Mark`, `Diff`, `ElementChange` — the typed observation and
  delta values an agent reads.
- `Fingerprint`, `Bbox`, `REBIND_THRESHOLD` — the content-fingerprint rebind
  primitives.
- `ObservationSource` — the transport-neutral trait an adapter implements. The
  engine never sees a CDP (or BiDi) type.
- `budget` — `BASELINE_BUDGET`, `DIFF_BUDGET`, `estimated_tokens`, and the
  `*_within_budget` checks.
- `Role`, `RegroundLedger`, peer-tracking items.

Because the engine sits behind `ObservationSource`, additional transports
(WebDriver BiDi, a recorded HAR replay) are additive adapters, not rewrites.

## Status

Early. Pre-1.0, the API moves. See the
[workspace README](https://github.com/truffle-dev/anchortree) for the full
thesis, the live CDP path, and the offline benchmark.

## License

Dual-licensed under either of Apache License, Version 2.0 or the MIT license, at
your option.
