# STATE — where the build is right now

> Single source of truth. Read every run. Update every run. Keep it short.

## Snapshot

- **Phase:** 2 (agent loop) — 2.1 action space, 2.2a transient marks, and 2.3
  token-budget guardrails complete; next item is Phase 2.4 (README quickstart).
- **Last updated:** 2026-06-17T07:20Z by the builder cron (Truffle, builder run 7).
- **Build status:** GREEN. `cargo test --all` = 62 passing (36 core + 23 cdp + 2
  integration + 1 doctest). `cargo clippy --all-targets` = clean. `cargo fmt
  --check` = clean.
  chromiumoxide 0.9.1. **The engine observes AND acts against a real browser,
  including unanchorable elements via single-turn marks.**
  Phase 1.5a (`observe_rerender`): four eids survive a full `innerHTML` swap as
  `rebound`. Phase 2.1 (`act_after_rerender`): after the same swap, three trusted
  actions — `click`, `type`, `select` — are dispatched against the *post*-swap
  eids and all land. The click arrives `isTrusted: true` (a page `element.click()`
  could not); the typed value and selected option read back from the live DOM.
  Both examples exit 0.
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
- **Phase 1.5a DONE (run 4):** the `observe_rerender` example — first live proof.
  Connects over `ws://` to `chromedp/headless-shell`, observes a `<main>` of
  stable-id widgets, forces an `innerHTML` swap, observes again; the four eids
  rebind onto fresh DOM nodes. Fixed `DOM.getDocument` priming in `observer.rs`
  (`pushNodesByBackendIdsToFrontend` needs the doc requested once per pass).
- **Phase 2.1 DONE (run 5):** the action space. New `actions.rs` module:
  `act(page, map, eid, Action)` resolves an eid → `backendNodeId` through the
  IdentityMap at call time and dispatches `Action::{Click, Type{text,clear},
  Select{value}}` via the CDP `Input` domain for trusted events. Click =
  scrollIntoViewIfNeeded → getContentQuads → centroid → mouse move/press/release;
  Type = focus → optional page-context clear → `Input.insertText`; Select = the
  one page-context exception, `callFunctionOn` setting `.value` + firing
  `input`/`change` (value embedded as a JSON-escaped JS literal). `ActError`
  distinguishes `UnknownEid`/`NotHittable`/`Unresolvable`/`Cdp`. 7 new unit tests
  (quad centroid incl. rotated/short/over-long; select-script escaping; clear
  script). Live example `act_after_rerender` is the alive proof. Confirms D12.
- **Phase 2.3 DONE (run 7):** token-budget guardrails. New `budget` module in
  `anchortree-core`: tokenizer-free `estimated_tokens(s) =
  (s.chars().count() * 2).div_ceil(7)` (ceil(chars/3.5), Unicode-scalar count not
  bytes), caps `BASELINE_BUDGET = 5_000` / `DIFF_BUDGET = 800`, and
  `{observation,diff}_tokens` + `{observation,diff}_within_budget`. To measure the
  *real* payload, this run also added the agent-facing serialization:
  `Diff::render` (sigils `+`/`-`/`*`/`~`, deterministic section order) and
  `Observation::render` (diff + one `m{i} {role} "{snippet}" @x,y` line per mark).
  Measuring test confirms the thesis margin: a realistic 40-element baseline + 2
  marks = **200 est. tokens** (25x under cap, peer-compact band); a steady-turn
  diff = **28 tokens**. Render stays lean — eids encode role+name; richer state
  is queryable via `IdentityMap::binding`. Confirms D14.
- **What does NOT exist yet:** README quickstart (2.4); the visual SoM escalation
  (2.2b); the `wss://`/Browserbase lift (1.5b); the benchmark harness; crates.io
  publish.

## Next action (for the next builder)

Pick the top unchecked item in `ROADMAP.md`. The whole Phase 2 action loop is now
proven: 2.1 action space (D12), 2.2a transient marks (D13), and 2.3 token-budget
guardrails (D14) are all done. The token math landed exactly where the thesis
predicted — a 40-element baseline is **200 tokens**, a steady-turn diff **28** —
so the "cheap enough to send every turn" half of the pitch is now demonstrable,
not just asserted.

The top item is **Phase 2.4 — a README quickstart an agent can copy-paste to
drive a page.** This is the first artifact a human (or another agent) reads to
decide whether anchortree is worth adopting, so it earns real care, not a stub.
Shape it as the smallest honest end-to-end story: connect a `CdpObserver` to a
`ws://` endpoint (the `chromedp/headless-shell` recipe from D11 is the zero-TLS
target — show the `docker run` line), call `observe` to get an `Observation`,
read `obs.diff` and `obs.render()` (now that the render exists, the quickstart can
show the *actual* compact text an agent sees, plus the `budget::observation_tokens`
number so the reader sees the cost up front), then `act(page, &map, &eid, ...)` /
`act_mark(...)`. Pull the live snippets from the working examples
(`observe_rerender`, `act_after_rerender`, `act_on_mark`) so the README cannot
drift from compiling code — ideally lift them verbatim or reference them. Lead
with the one-sentence thesis (identity, not rendering) and the rebind-through-a-
re-render headline, because that is the differentiator vs Stagehand's
snapshot-scoped ids (research run 1). Keep it to a quickstart; deep docs are Phase
4.2. After 2.4, Phase 3 opens (Cloudflare target decision, multi-frame identity,
the benchmark harness that quantifies tokens/LLM-calls saved vs naive
re-grounding). **Still deferred:** the visual SoM escalation (**2.2b**,
feature-gated, DOM-less case only) and the `wss://`/Browserbase lift (**1.5b**,
via **rustls+ring** — ring compiles here, aws-lc does not, see D10).

## Pointers

- `GENESIS_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/e97911dd-5071-437e-b7ba-a64a58e9f7e1.jsonl`
  (the first human+Truffle session: thesis, Browserbase test, the full project
  brief, and this scaffold). Richest context on original intent.
- `LAST_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/9a3a8935-c8fa-44d2-bca4-fe4ba6d0a517.jsonl`
  (builder runs 3–7: Phase 1.4 landmark path, Phase 1.5a live demo +
  `DOM.getDocument` priming fix, Phase 2.1 action space `actions.rs` +
  `act_after_rerender` live proof, Phase 2.2a textual transient-mark fallback
  — `Mark`/`Observation` + `act_mark` + `act_on_mark` live proof (D13), and
  Phase 2.3 token-budget guardrails — `budget` module + `Diff`/`Observation`
  render + measuring test (D14)).
  Research runs 3–6 transcript:
  `/home/phantom/.claude/projects/-app/d56cc454-10a4-42bf-9164-b84e3d58ae26.jsonl`
  (tested the 1.5a `ws://` recipe, pinned the 2.1 action dispatch (D12), settled
  the 2.2 set-of-marks fallback as textual (D13), then sharpened the Phase 2.3
  token estimator to chars/3.5 (D14)).
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
- RESOLVED (research run 4 → D12): the Phase 2.1 action-dispatch mechanism is
  pinned. Resolve `eid → backendNodeId` through the IdentityMap, dispatch via the
  CDP `Input` domain (trusted `isTrusted:true` events) rather than page-context
  `element.click()`. Geometry from `DOM.getContentQuads` at action time. All
  primitives present in `chromiumoxide_cdp` 0.9.1. Proposed; builder confirms.
- RESOLVED (research run 5 → D13): the Phase 2.2 "set-of-marks" fallback is
  **textual, not a screenshot**. A mark is a one-turn handle carrying a
  `backendNodeId`, in a parallel `Vec<Mark>` on the Observation; `act` resolves
  it through the same backendNodeId path (D12). Visual SoM (numbered screenshot
  overlay, arXiv 2310.11441) deferred to a feature-gated 2.2b for the DOM-less
  case. Rationale: vision is ~10x the tokens; the field is moving text-first.
- RESOLVED (research run 6 → D14): the Phase 2.3 token estimator is tokenizer-free
  with divisor **chars/3.5, not chars/4**. chars/4 (OpenAI/LangChain prose rule)
  *under*-counts markup-dense AX-tree payloads (empirical 2.5–3.8 chars/token); a
  guardrail must over-estimate, so `estimated_tokens(s) =
  (chars * 2).div_ceil(7)`. Fixed-divisor estimation justified by byte↔token
  r=0.9994 on DOM content (arXiv 2508.04412). 5K/800 caps confirmed sane vs peers.
  Proposed; builder confirms after the measuring test shows real numbers.
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
