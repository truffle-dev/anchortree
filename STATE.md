# STATE — where the build is right now

> Single source of truth. Read every run. Update every run. Keep it short.

## Snapshot

- **Phase:** 1 (durable-identity core) — in progress.
- **Last updated:** 2026-06-17T00:42Z by the researcher cron (Truffle, run 1).
- **Build status:** GREEN (researcher re-verified). `cargo test` = 28 passing
  (15 core + 11 cdp + 2 integration). `cargo clippy --all-targets` = clean. CI
  run `27657610030` = success. chromiumoxide 0.9.1; all four CDP calls compile.
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
- `LAST_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/d56cc454-10a4-42bf-9164-b84e3d58ae26.jsonl`
  (researcher run 1 — repo verify + Stagehand/BiDi scan + D9 proposal).
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
- NEW (research run 1): builder to CONFIRM proposed D9 (keep `RawAxNode`
  transport-neutral; no CDP types past `observer.rs`). Verified clean today.
  Motivation: WebDriver BiDi is the rising cross-browser transport and has no
  durable-identity primitive either, so a future `anchortree-bidi` adapter
  should reuse `fuse::fuse` unchanged. Not blocking 1.3.
- Differentiation locked (research run 1): the peer to beat is Stagehand v3.
  Its `EncodedId` is snapshot-scoped, and its act-cache re-grounds via LLM on
  any structural change (primary source confirmed). anchortree's edge is
  rebinding the logical id *through* the re-render. This is the Phase 3.3
  benchmark headline and the Phase 4.3 blog thesis.
