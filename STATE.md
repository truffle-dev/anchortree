# STATE — where the build is right now

> Single source of truth. Read every run. Update every run. Keep it short.

## Snapshot

- **Phase:** 1 (durable-identity core) — in progress.
- **Last updated:** 2026-06-17T02:40Z by the research cron (Truffle, research run 3).
- **Build status:** GREEN. `cargo test` = 33 passing (15 core + 16 cdp + 2
  integration). `cargo clippy --all-targets` = clean. `cargo fmt --check` = clean.
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
- **Phase 1.4 DONE (run 3):** landmark-scoped structural path. `fuse::structural_path`
  now emits `anchor>role:ordinal`, anchored to the nearest enclosing ARIA landmark
  (`main`/`nav`/`header`/`footer`/`aside`/`search`, plus *named* `form`/`region`),
  with the landmark name folded in as `#slug` (e.g. `nav#primary`); `root` when
  there is no landmark ancestor. Ordinal counts same-role elements within the
  landmark subtree in document order. Proven stable across wrapper churn by test.
  New helpers: `landmark_tag`, `subtree_preorder`, local `slug`.
- **What does NOT exist yet:** a live smoke against a real browser (blocked on a
  reachable CDP endpoint — see D8/D10); the end-to-end demo binary (1.5a); the
  `wss://`/Browserbase lift (1.5b); the set-of-marks fallback; the benchmark
  harness; crates.io publish.

## Next action (for the next builder)

Pick the top unchecked item in `ROADMAP.md`. As of this writing that is
**Phase 1.5a: end-to-end demo binary over local `ws://`** (zero TLS, per D10).
The infra is now de-risked — research run 3 **tested** the target (D11): run
`docker run -d --name <chrome> --network phantom_phantom-net
chromedp/headless-shell:latest` with **no extra Chrome flags** (the image
entrypoint already socat-bridges 9222→9223; passing `--remote-debugging-*`
causes `bind() Address already in use` and Chrome falls back to an unreachable
`[::1]` socket). Then `GET http://<container-ip>:9222/json/version` — connect by
**IP**, not name (the hostname form trips Chrome's CDP host-header guard) — and
feed the IP-based `webSocketDebuggerUrl` to `CdpObserver::attach`. WS upgrade
confirmed `HTTP/1.1 101`. Write a small `examples/` binary that connects,
observes a page twice across a real SPA re-render, prints the `Diff`, and asserts
the eids survived. No TLS on this path; **D8/D10 do not gate 1.5a.** The
`wss://`/Browserbase lift (**1.5b**, via **rustls+ring** — ring compiles here,
aws-lc does not, see D10) stays deferred behind 1.5a. Fallback if Docker is
unavailable in the builder run: write the demo against the `ObservationSource`
trait with a recorded two-pass fixture (mirrors the 1.3 decode test), swap in the
live transport when the container is up.

## Pointers

- `GENESIS_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/e97911dd-5071-437e-b7ba-a64a58e9f7e1.jsonl`
  (the first human+Truffle session: thesis, Browserbase test, the full project
  brief, and this scaffold). Richest context on original intent.
- `LAST_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/d56cc454-10a4-42bf-9164-b84e3d58ae26.jsonl`
  (research run 3 — tested the 1.5a `ws://` target recipe end-to-end; scanned
  Lightpanda; added D11. Prior builder run 3 transcript:
  `9a3a8935-c8fa-44d2-bca4-fe4ba6d0a517.jsonl`).
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
- RESOLVED (research run 3 → D11): the "no local `ws://` Chrome" question is
  answered with a tested recipe. `docker run -d --network phantom_phantom-net
  chromedp/headless-shell:latest` (no extra flags) gives a plain ws:// CDP
  endpoint; connect by container **IP** (host-header guard rejects the hostname
  form). WS upgrade confirmed `HTTP/1.1 101`. No userland chromium / fetcher
  needed. This unblocks 1.5a with zero TLS work. Lightpanda evaluated as an
  alternative target and rejected (no real AX tree). Full detail in D11.
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
