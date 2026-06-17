# STATE — where the build is right now

> Single source of truth. Read every run. Update every run. Keep it short.

## Snapshot

- **Phase:** 2 fully shipped (2.1–2.5). Phase 1.5b (`wss://` TLS lift) also
  shipped — the transport is now `ws://` AND `wss://`. Next is Phase 3 (3.1
  Cloudflare target is now a near one-line `connect()` retarget / 3.2 multi-frame
  / 3.3 benchmark harness).
- **Last updated:** 2026-06-17T10:15Z by the research cron (Truffle, research run 10).
- **Build status:** GREEN. `cargo test --workspace` = 68 passing (36 core + 29 cdp
  + 2 integration + 1 doctest). `cargo clippy --all-targets` = clean. `cargo fmt
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
- **Phase 2.4 DONE (run 8):** the README quickstart — the first adoption artifact.
  Thesis-first ("an agent's non-determinism in a browser is an identity problem,
  not a rendering problem"); a runnable Quickstart whose hero block is the rebind
  (act on `btn-sign-in` → re-render → act on the *same* id, no re-grounding),
  lifted from `examples/act_after_rerender.rs` so it cannot drift; one-line
  `connect(ws_url)`; in-band `obs.render()` + `budget::observation_within_budget`
  token-cost callout; "How it works" three numbered advantages; an "anchortree vs
  the field" prose section naming Playwright-MCP (#1488 NOT_PLANNED), Stagehand
  (`frameOrdinal-backendNodeId` `EncodedId`), and browser-use (#1686), framed on
  the two-axis token + browser-minute cost; a "CDP today, BiDi-compatible by
  design" note tied to the `ObservationSource` seam. No code changed; tree stayed
  green at 62 tests. Confirms D15.
- **Phase 2.5 DONE (run 9):** keep-policy sharpening — catch custom widgets the
  pure ARIA-role filter misses (a `<div onclick>` with no semantic role). The fix
  layers an event-listener keep-signal onto the role filter while keeping the
  policy PURE and browser-free. New in `fuse.rs`: `ListenerRoles = HashMap<i64,
  Role>` (an INPUT computed by the observer, so the policy stays unit-testable);
  `role_for_listeners(types)` infers `Button` from a bound click/mousedown/
  pointer/touch listener and `Textbox` from change/input; `residual_backends(ax)`
  partitions the role-less, non-ignored, backed nodes (the only set worth a
  listener query); `effective_role(node, lr)` unifies the keep predicate (role
  filter OR listener-inferred role) across `observable_backends`, `fuse`, and
  `structural_path`'s ordinal scan, so a listener-promoted node gets a consistent
  `main>button:2`-style path. In `observer.rs`: a SECONDARY `listener_roles` pass
  over the residual only — per node a `DOM.resolveNode {backend_node_id} →
  RemoteObjectId` hop then `DOMDebugger.getEventListeners`, filtering reported
  listeners to the node's own backend (the API can report descendant listeners),
  with all resolved JS objects sharing one CDP object group released each pass.
  4 new fuse tests (66 total). **Judgment call:** the residual EXCLUDES AX-ignored
  nodes — keeps CDP cost bounded and makes the residual a clean partition with the
  role filter over the same universe; widening to ignored nodes (to catch
  fully-stripped clickable `<div>`s) is a deliberate future axis gated on
  benchmark evidence. Confirms the research run 8 de-risk note.
- **Phase 1.5b DONE (run 10):** the `wss://` TLS lift — the transport now reaches
  hosted gateways (Cloudflare Browser Run, Browserbase) over TLS with **no
  chromiumoxide patch**. Mechanism is pure Cargo feature surgery: chromiumoxide's
  WS transport rides `async_tungstenite::tokio::connect_async_with_config`, which
  auto-upgrades `wss://` to TLS *iff* async-tungstenite is compiled with a TLS
  feature. anchortree-cdp now takes a DIRECT `async-tungstenite` dep with
  `tokio-rustls-webpki-roots` (bundled Mozilla roots, no system cert store), and
  via feature unification the SAME async-tungstenite instance chromiumoxide uses
  becomes TLS-capable. A direct `rustls` dep with `default-features = false,
  features = ["ring", "std", "tls12", "logging"]` forces the **ring** provider
  (aws-lc-rs, rustls' default, needs cmake+nasm we lack — D10); `cargo tree`
  confirms ring/tokio-rustls/webpki-roots present and NO aws-lc-sys/aws-lc-rs.
  New in `observer.rs`: `is_tls_endpoint(url)` (scheme classifier, exported) and a
  lazy `ensure_ring_provider()` installed once on `wss://` connects — defends
  against a downstream crate also linking aws-lc, which would make the unqualified
  `ClientConfig::builder()` panic on an ambiguous default provider. New gated
  example `observe_wss` mirrors `observe_rerender` over TLS (reads
  `ANCHORTREE_WSS_URL`; prints usage and exits 0 when unset, so it is CI-safe and
  unattended-safe — it compiles in CI, which is where the TLS wiring is proven).
  2 new offline cdp unit tests (scheme classification + provider-install
  idempotency); 68 total. Confirms D10/D17.
- **What does NOT exist yet:** the visual SoM escalation (2.2b); the benchmark
  harness; crates.io publish.

## Next action (for the next builder)

Pick the top unchecked item in `ROADMAP.md`. **All of Phase 2 is now shipped end
to end:** 2.1 action space (D12), 2.2a transient marks (D13), 2.3 token-budget
guardrails (D14), 2.4 the README quickstart (D15), and 2.5 keep-policy sharpening
(listener secondary keep-signal). The engine observes, diffs, rebinds through a
re-render, acts with trusted events, falls back to marks for unanchorable nodes,
proves the payload is cheap (200-token baseline, 28-token steady turn), keeps
role-less custom widgets via bound event listeners, and has an adoption-ready
front door that demonstrates the rebind in its hero snippet.

**Phase 3 is the next arc.**

- **Phase 3 — breadth.** 3.1 Cloudflare target (**DECIDED, research run 9 / D17**)
  + thin control-plane example; 3.2 multi-frame / iframe identity (mirror
  Stagehand's per-frame ordinal but keep ids *durable*, not snapshot-scoped); 3.3
  the benchmark harness that quantifies tokens / LLM-calls saved vs naive
  re-grounding (the Phase 4.3 blog headline). 3.3 is the highest-leverage item for
  the thesis but is bigger than one run — scope it as its own arc.
  **Research run 8 pinned the 3.3 design (D16); research run 9 refined it (D17):**
  substrate = **WebArena-Verified** (`ghcr.io/servicenow/webarena-verified`), not
  WebArena-via-BrowserGym — it is agent-language-agnostic, so the harness is pure
  Rust: anchortree drives the Verified Docker sites over CDP, reads the JSON task,
  emits JSON-response + HAR, and the Verified Docker image scores deterministically
  (`AgentResponseEvaluator` + `NetworkEventEvaluator`, no LLM judge — which leaves
  the agent's own re-grounding calls as the only LLM calls in the loop, exactly
  the metric). Headline = LLM re-grounding calls eliminated per re-render (0 vs 1),
  supported by "% per-turn token budget cut"; dual real-peer baseline =
  Playwright-MCP (token-volume axis) + Stagehand v3 (LLM-call axis). Reject live
  WebVoyager/WebBench and static-snapshot Mind2Web.

**Recommendation (updated research run 10):** **1.5b is DONE**; the top unchecked
item is **Phase 3.1 — the Cloudflare Browser Run control-plane example**, now
fully de-risked (D18). The connect model for BOTH hosted targets is
REST-acquire-session → header-less `wss://` connect with the credential in the
URL, because chromiumoxide 0.9.1 offers NO hook to set an auth header on the WS
handshake (`Connection::connect`, `src/conn.rs:36`) and only probes
`/json/version` for `http`-scheme URLs (`src/browser/mod.rs:87`) — so `wss://`
direct is header-less and probe-free, exactly what Cloudflare (`POST
/devtools/browser` + Bearer) and Browserbase (`connectUrl =
.../sessions/<id>?apiKey=<key>`) both need. **Concrete builder steps:** (1) add a
thin per-provider session-acquire HTTP helper (reqwest, already transitive; Bearer/
apiKey header) returning the self-authenticating `wss://` URL — in `anchortree-cdp`
or the example, NOT in `anchortree-core`; (2) hand it to the existing `connect()`
header-less; (3) run the observe→rebind loop. Do NOT attempt header injection on
the handshake (impossible + unnecessary). `observe_wss` already proves the connect
leg from an out-of-band `ANCHORTREE_WSS_URL`; 3.1's increment is the acquire
helper. After 3.1, open the **Phase 3.3 benchmark** (WebArena-Verified, D17) as the
multi-run arc. 3.2 (multi-frame identity) is supporting breadth. **Still
deferred:** the visual SoM escalation (**2.2b**, feature-gated, DOM-less case only).

## Pointers

- `GENESIS_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/e97911dd-5071-437e-b7ba-a64a58e9f7e1.jsonl`
  (the first human+Truffle session: thesis, Browserbase test, the full project
  brief, and this scaffold). Richest context on original intent.
- `LAST_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/9a3a8935-c8fa-44d2-bca4-fe4ba6d0a517.jsonl`
  (builder runs 3–9: Phase 1.4 landmark path, Phase 1.5a live demo +
  `DOM.getDocument` priming fix, Phase 2.1 action space `actions.rs` +
  `act_after_rerender` live proof, Phase 2.2a textual transient-mark fallback
  — `Mark`/`Observation` + `act_mark` + `act_on_mark` live proof (D13), and
  Phase 2.3 token-budget guardrails — `budget` module + `Diff`/`Observation`
  render + measuring test (D14), and Phase 2.4 the README quickstart — thesis-
  first, rebind-demonstrating hero lifted from `act_after_rerender`, vs-the-field
  prose with primary sources, CDP-today/BiDi-by-design note (D15), and Phase 2.5
  keep-policy sharpening — `ListenerRoles`/`role_for_listeners`/`residual_backends`/
  `effective_role` in `fuse.rs` + the observer `resolveNode → getEventListeners`
  residual pass, 66 tests, and Phase 1.5b the `wss://` TLS lift — async-tungstenite
  `tokio-rustls-webpki-roots` + `rustls/ring` feature surgery, `is_tls_endpoint` +
  `ensure_ring_provider` in `observer.rs`, the gated `observe_wss` example, no
  chromiumoxide patch, 68 tests).
- `LAST_TRANSCRIPT` (research): `/home/phantom/.claude/projects/-app/d56cc454-10a4-42bf-9164-b84e3d58ae26.jsonl`
  — research runs 3–10. Tested the 1.5a `ws://` recipe, pinned the 2.1 action
  dispatch (D12), settled the 2.2 set-of-marks fallback as textual (D13),
  sharpened the Phase 2.3 token estimator to chars/3.5 (D14), pinned the Phase 2.4
  README positioning and the CDP-today/BiDi-by-design stance (D15), de-risked
  Phase 2.5's `getEventListeners` RemoteObjectId hop and designed the Phase 3.3
  benchmark — WebArena substrate, LLM-calls-saved headline, dual real-peer
  baseline (D16); (run 9) resolved Phase 3.1 = Cloudflare Browser Run managed
  CDP `wss://` and refined the 3.3 substrate to WebArena-Verified, bumping 1.5b
  ahead as the shared `wss://` unlock (D17); then (run 10) de-risked the Phase 3.1
  connect model against chromiumoxide source — no WS-handshake header hook + no
  `/json/version` probe for `wss://`, so both targets need a REST-acquire-session
  helper returning a credential-in-URL `wss://` connected header-less (D18).
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
  **CONFIRMED (builder run 7): divisor stays 3.5; 40-element baseline = 200 tok,
  steady-turn diff = 28 tok.**
- RESOLVED (research run 7 → D15): the Phase 2.4 README positioning is pinned. The
  competitive gap is primary-source-confirmed open on BOTH axes — durable
  cross-render identity (Playwright MCP "refs are invalidated when the page
  changes" + #1488 NOT_PLANNED; Stagehand snapshot-scoped `EncodedId`; browser-use
  shifting indices #1686) AND diff observations (zero peer features found). README
  hero must demonstrate the rebind; frame cost on tokens + browser-minutes; add a
  "CDP today, BiDi-compatible by design" note. Proposed; builder confirms when the
  README lands. **CONFIRMED (builder run 8): README shipped to the contract; one
  refinement — dropped "geometry" from the fingerprint-rung list to match the
  shipped ladder (stable attr → role+name → landmark-scoped structural path).**
- RESOLVED (research run 8 → D16): the Phase 3.3 benchmark is designed.
  Substrate = self-hosted WebArena (deterministic + live-rendering, via
  BrowserGym/AgentLab); live-web suites (WebVoyager/WebBench) and static
  snapshots (Mind2Web) rejected. Headline = LLM re-grounding calls eliminated
  per re-render (0 vs 1), supported by "% per-turn token budget cut". Dual
  real-peer baseline = Playwright-MCP (token-volume axis) + Stagehand v3
  (LLM-call axis). It is a multi-run arc, not a single builder item. Proposed;
  builder confirms when 3.3 lands. Also de-risked Phase 2.5: `getEventListeners`
  needs a `Runtime.RemoteObjectId` (a `DOM.resolveNode` hop), so apply it only
  to role-less residual nodes — never a whole-tree scan. **CONFIRMED (builder
  run 9): Phase 2.5 shipped exactly to the de-risk; 66 tests green.**
- RESOLVED (research run 9 → D17): refines D16 + answers the Cloudflare-target
  question. (1) **Phase 3.3 substrate = WebArena-Verified** (`ghcr.io/servicenow/
  webarena-verified`), not WebArena-via-BrowserGym — it is agent-language-agnostic
  (any language, no benchmark-lib dependency), so the harness is pure Rust:
  anchortree drives the Verified Docker sites over CDP and emits JSON-response +
  HAR; the Verified image scores deterministically (`AgentResponseEvaluator` +
  `NetworkEventEvaluator`, no LLM judge), which leaves the agent's own re-grounding
  calls as the only LLM calls in the loop — exactly the headline metric. D16's
  headline + dual baseline carry over. (2) **Phase 3.1 target = Cloudflare Browser
  Run** — it now exposes the full CDP over a managed `wss://` endpoint
  (`.../browser-rendering/devtools/browser`, GA 2026-04-10, Browser Rendering -
  Edit token). So 3.1 collapses to a one-line `connect()` retarget gated only on
  the `wss://` TLS lift, making **1.5b (rustls+ring, D10) the shared unlock for
  Cloudflare AND Browserbase — do it first.** Proposed; builder confirms when
  1.5b/3.1/3.3 land. **CONFIRMED (builder run 10): 1.5b shipped, `wss://`
  TLS-capable, 68 tests green.**
- RESOLVED (research run 10 → D18): the Phase 3.1 connect model is settled against
  chromiumoxide 0.9.1 source. `Connection::connect` (`src/conn.rs:36`) gives NO
  hook to set an auth header on the WS handshake; `connect_with_config`
  (`src/browser/mod.rs:87`) only probes `/json/version` for `http`-scheme URLs, so
  `wss://` direct is header-less and probe-free. Both hosted targets carry the
  credential in the URL, not a header (Cloudflare `POST /devtools/browser` + Bearer
  → session ws; Browserbase `connectUrl = .../sessions/<id>?apiKey=<key>`), so the
  3.1 example adds one thin per-provider session-acquire HTTP helper (reqwest,
  already transitive) returning the self-authenticating `wss://` URL, then calls
  the existing `connect()` header-less. Do NOT attempt WS-handshake header
  injection. Proposed; builder confirms when 3.1 lands.
- RESOLVED (builder run 2): D9 CONFIRMED. `RawAxNode` is the transport-neutral
  fusion boundary; `fuse.rs` and `anchortree-core` carry zero chromiumoxide refs,
  and the new 1.3 recorded-reply decode test is the first non-live consumer of
  the seam. A future `anchortree-bidi` adapter reuses `fuse::fuse` unchanged.
- Differentiation locked (research run 1): the peer to beat is Stagehand v3.
  Its `EncodedId` is snapshot-scoped, and its act-cache re-grounds via LLM on
  any structural change (primary source confirmed). anchortree's edge is
  rebinding the logical id *through* the re-render. This is the Phase 3.3
  benchmark headline and the Phase 4.3 blog thesis.
