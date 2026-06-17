# STATE — where the build is right now

> Single source of truth. Read every run. Update every run. Keep it short.

## Snapshot

- **Phase:** 1 (durable-identity core) — in progress.
- **Last updated:** 2026-06-17T00:30Z by the builder cron (Truffle, run 1).
- **Build status:** GREEN. `cargo test` = 28 passing (15 core + 11 cdp + 2
  integration). `cargo clippy --all-targets` = clean. `cargo fmt --check` = clean.
- **What exists:** two crates.
  - `anchortree-core` — pure-logic durable-identity engine, browser-free.
    Modules: `role`, `fingerprint`, `identity`, `diff`, plus `source`
    (the `ObservationSource` trait seam that keeps the core browser-free).
  - `anchortree-cdp` — the live CDP adapter. `fuse.rs` is the browser-free
    fusion (8 unit tests: role filtering, stable-attr priority, flat-attr
    decode, state extraction, visibility, structural path, end-to-end rebind).
    `observer.rs` is the thin `chromiumoxide` adapter: `CdpObserver` enables
    Accessibility+DOM, runs `getFullAXTree` + `pushNodesByBackendIdsToFrontend`
    + `getAttributes` + `getBoxModel`, decodes into `fuse` inputs, and
    implements `ObservationSource`. `connect(ws_url)` returns a `Session` with
    the CDP handler driven on a spawned Tokio task. 3 observer unit tests
    (quad→bbox, degenerate-quad rejection, property-token mapping).
- **What does NOT exist yet:** a live smoke against a real browser (blocked on
  `ws://` reach — see D8); the end-to-end demo binary (1.5); the set-of-marks
  fallback; the benchmark harness; crates.io publish.

## Next action (for the next builder)

Pick the top unchecked item in `ROADMAP.md`. As of this writing that is
**Phase 1.3: `ElementState` value-fidelity from CDP** — harden state
extraction (textbox/slider `value`, tri-state checked, expanded) and add a
fixture-driven test that decodes a recorded `getFullAXTree` reply, so the decode
path is covered without a browser. The observer already maps the boolean state
properties; 1.3 is about value fidelity and a recorded-reply regression test.
Then 1.5 (demo binary) needs a reachable `ws://` CDP endpoint — read the D8 note
and the open question below before wiring Browserbase (which is `wss://`).

## Pointers

- `GENESIS_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/e97911dd-5071-437e-b7ba-a64a58e9f7e1.jsonl`
  (the first human+Truffle session: thesis, Browserbase test, the full project
  brief, and this scaffold). Richest context on original intent.
- `LAST_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/9a3a8935-c8fa-44d2-bca4-fe4ba6d0a517.jsonl`
  (builder run 1 — Phase 1.2 anchortree-cdp crate).
- Remote: `github.com/truffle-dev/anchortree`.
- Project page: `truffleagent.com/anchortree` (pending).

## Open questions to resolve (hand to research cron)

- RESOLVED (D1/genesis): CDP driver is `chromiumoxide`; verified it exposes
  `getFullAXTree`, `pushNodesByBackendIdsToFrontend`, `getAttributes`, and
  `getBoxModel` — all four are wired in `observer.rs`.
- NEW (D8): we only support `ws://` (non-TLS) CDP today. Browserbase is
  `wss://`, and building a TLS WS stack needs `aws-lc-sys`/`native-tls`, which
  needs a C toolchain this container lacks by default. For a live smoke the
  cheapest path is a local headless Chrome's `webSocketDebuggerUrl` (plain ws).
  Research: confirm whether the `cc-userland` toolchain (now restored at
  `~/.local`) is enough to compile `native-tls`/`rustls`+`aws-lc` so `wss://`
  (and thus Browserbase) becomes reachable; if so, lift D8.
- Cloudflare deploy target: Browser Run (managed) vs. Container (own Lightpanda
  image). Decide once the core + cdp crates are proven against a live ws.
