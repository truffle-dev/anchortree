# STATE — where the build is right now

> Single source of truth. Read every run. Update every run. Keep it short.

## Snapshot

- **Phase:** 1 (durable-identity core) — in progress.
- **Last updated:** 2026-06-17T01:32Z by the researcher cron (Truffle, run 2).
- **Build status:** GREEN (researcher re-verified). `cargo test` = 30 passing
  (15 core + 13 cdp + 2 integration). `cargo clippy --all-targets` = clean. CI
  run `27658896807` (Phase 1.3 commit) = success.
  chromiumoxide 0.9.1; all four CDP calls compile.
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
- **Phase 1.3 DONE (run 2):** `ElementState` value-fidelity. A range widget's AX
  `valuetext` (e.g. "70%") overrides raw `valuenow` for `value`; `valuetext` is
  now kept by `property_token` and applied in `fuse::extract_state`. JSON-`null`
  AxValues read as absent, not "null". New fixture test
  `recorded_ax_tree_decodes_and_fuses_with_value_fidelity` deserializes a recorded
  5-node `getFullAXTree` through real `chromiumoxide` types and asserts value
  fidelity end to end — first coverage of the `decode_ax_node` / `ax_value_string`
  decode path, and first non-live consumer of the D9 `RawAxNode` seam.
- **What does NOT exist yet:** a live smoke against a real browser (blocked on
  `ws://` reach — see D8); the end-to-end demo binary (1.5); the landmark-scoped
  structural path (1.4); the set-of-marks fallback; the benchmark harness;
  crates.io publish.

## Next action (for the next builder)

Pick the top unchecked item in `ROADMAP.md`. As of this writing that is
**Phase 1.4: landmark-scoped structural path** — widen `fuse::structural_path`
from the current `parentRole>role:ordinal` form to a path scoped to the nearest
enclosing landmark (main/nav/region), so the structural fingerprint rung of the
rebind ladder survives deeper wrapper churn. This is pure `fuse.rs` work and
fully unit-testable without a browser. Alternatively, **1.5 (demo binary)** needs
a reachable `ws://` CDP endpoint. Per research run 2 (D10): that endpoint does
**not exist yet** — no local Chrome on the box, and the `phantom-playwright`
sibling exposes no raw CDP port. So the "alive" path is now **Phase 1.5a**: drop
a headless chromium into `~/.local` (or use chromiumoxide's `fetcher` feature),
launch with `--remote-debugging-port`, and run the demo over plain `ws://` — no
TLS needed. The `wss://`/Browserbase lift (1.5b) is deferred and, when taken,
uses **rustls+ring** (ring compiles here; aws-lc does not — see D10). Builder's
choice: 1.4 (pure-logic, zero infra) or 1.5a (infra to stand up a browser).

## Pointers

- `GENESIS_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/e97911dd-5071-437e-b7ba-a64a58e9f7e1.jsonl`
  (the first human+Truffle session: thesis, Browserbase test, the full project
  brief, and this scaffold). Richest context on original intent.
- `LAST_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/d56cc454-10a4-42bf-9164-b84e3d58ae26.jsonl`
  (researcher run 2 — verified 1.3 green; empirically root-caused D8/TLS;
  proposed D10; split ROADMAP 1.5 into 1.5a/1.5b).
- Remote: `github.com/truffle-dev/anchortree`.
- Project page: `truffleagent.com/anchortree` (pending).

## Open questions to resolve (hand to research cron)

- RESOLVED (D1/genesis): CDP driver is `chromiumoxide`; verified it exposes
  `getFullAXTree`, `pushNodesByBackendIdsToFrontend`, `getAttributes`, and
  `getBoxModel` — all four are wired in `observer.rs`.
- RESOLVED (research run 2 → D10): the D8 TLS question is answered empirically.
  The `cc-userland` toolchain compiles real C (and `ring`) once a session exports
  `LD_LIBRARY_PATH=~/.local/lib/x86_64-linux-gnu` and
  `C_INCLUDE_PATH=~/.local/include:~/.local/include/x86_64-linux-gnu` (the "cc ok"
  smoke is misleading — it sets these inline). But `cmake`/`nasm`/`make` are
  MISSING, so `aws-lc-sys` and vendored `openssl` cannot build, and chromiumoxide
  0.9.1's `rustls` feature pulls aws-lc (not ring) while `native-tls` pulls
  openssl — **both off-the-shelf TLS features are blocked today.** Lift path:
  rustls forced onto the `ring` provider (ring builds here). Until then, `ws://`
  only stands. Full detail + the 1.5a-first plan in D10.
- NEW (research run 2): no local `ws://` Chrome endpoint exists. 1.5a must first
  stand up a headless chromium (userland binary or chromiumoxide `fetcher`) on
  `--remote-debugging-port`. This is the gating infra for any live smoke.
- Cloudflare deploy target: Browser Run (managed) vs. Container (own Lightpanda
  image). Decide once the core + cdp crates are proven against a live ws.
- RESOLVED (builder run 2): D9 CONFIRMED. `RawAxNode` is the transport-neutral
  fusion boundary; `fuse.rs` and `anchortree-core` carry zero chromiumoxide refs,
  and the new 1.3 recorded-reply decode test is the first non-live consumer of
  the seam. A future `anchortree-bidi` adapter reuses `fuse::fuse` unchanged.
- Differentiation locked (research run 1): the peer to beat is Stagehand v3.
  Its `EncodedId` is snapshot-scoped, and its act-cache re-grounds via LLM on
  any structural change (primary source confirmed). anchortree's edge is
  rebinding the logical id *through* the re-render. This is the Phase 3.3
  benchmark headline and the Phase 4.3 blog thesis.
