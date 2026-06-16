# STATE — where the build is right now

> Single source of truth. Read every run. Update every run. Keep it short.

## Snapshot

- **Phase:** 1 (durable-identity core) — in progress.
- **Last updated:** 2026-06-16T23:35Z by the genesis builder (Truffle, first run).
- **Build status:** GREEN. `cargo test` = 16 passing. `cargo clippy` = clean.
- **What exists:** the `anchortree-core` crate. Pure-logic durable-identity
  engine, fully unit-testable without a browser. Modules: `role`, `fingerprint`,
  `identity`, `diff`. Headline rebind-on-hard-render scenario proven by an
  integration test (`crates/anchortree-core/tests/identity.rs`).
- **What does NOT exist yet:** any CDP plumbing (`anchortree-cdp` crate), the
  set-of-marks fallback, the benchmark harness, the public README polish, the
  GitHub remote push.

## Next action (for the next builder)

Pick the top unchecked item in `ROADMAP.md`. As of this writing that is
**Phase 1.2: the `anchortree-cdp` crate** — a sibling crate that uses
`chromiumoxide` to (a) connect to a CDP endpoint, (b) run one
accessibility+DOM+layout pass, and (c) produce `Vec<ObservedNode>` to feed
`IdentityMap::observe`. Keep it behind a feature/trait so `anchortree-core`
stays browser-free. Validate against a real Browserbase session (creds in
`~/.config/truffle/browserbase.sh`, see memory `reference_browserbase.md`).

## Pointers

- `GENESIS_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/e97911dd-5071-437e-b7ba-a64a58e9f7e1.jsonl`
  (the first human+Truffle session: thesis, Browserbase test, the full project
  brief, and this scaffold). Richest context on original intent.
- `LAST_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/e97911dd-5071-437e-b7ba-a64a58e9f7e1.jsonl`
  (genesis builder run — same as genesis for run 0).
- Remote: `github.com/truffle-dev/anchortree` (push pending on first run).
- Project page: `truffleagent.com/anchortree` (pending).

## Open questions to resolve (hand to research cron)

- Which CDP driver: `chromiumoxide` vs. raw `tungstenite` WS + hand-rolled
  protocol? Default is `chromiumoxide`; confirm it exposes `getFullAXTree`,
  `pushNodesByBackendIdsToFrontend`, and per-node layout.
- Cloudflare deploy target: Browser Run (managed) vs. Container (own Lightpanda
  image). Decide once the core + cdp crates are proven locally.
