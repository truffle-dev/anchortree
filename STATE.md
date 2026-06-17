# STATE — where the build is right now

> Single source of truth. Read every run. Update every run. Keep it short.

## Snapshot

- **Phase:** 2 fully shipped (2.1–2.5). Phase 1.5b (`wss://` TLS lift) shipped.
  Phase 3.1 **acquire leg** shipped — provider credentials resolve to a
  self-authenticating `wss://` CDP URL (Browserbase REST mint + Cloudflare
  token-URL). Phase 3.1b **hosted connect leg** NOW SHIPPED (run 12, D19→D20
  resolved) — a self-contained thin CDP channel flat-attaches to the page a
  hosted browser already has open and drives the full observe→rebind loop over
  it; **live-verified against BOTH a local `ws://` browser and real Browserbase
  `wss://`**. Phase 3.1 is complete end to end. Phase 3.2a **same-origin
  multi-frame identity NOW SHIPPED (run 13, D21 mechanics 1+2+4)** — the durable
  eid is two-tier `(frame-key, in-frame fingerprint)`; two structurally identical
  widgets in different frames hold distinct eids and rebind independently,
  **live-verified against a real same-origin `srcdoc` iframe**. Next: 3.2b OOPIF
  (mechanics 3+5) / 3.3 benchmark harness.
- **Last updated:** 2026-06-17T12:45Z by the research cron (Truffle, research run 13).
- **Build status:** GREEN. `cargo test --workspace` = 99 passing (40 core + 55 cdp
  + 2 integration + 2 doctests). `cargo clippy --all-targets` = clean under
  `-D warnings`. `cargo fmt --check` = clean.
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
- **Phase 3.1 acquire leg DONE (run 11):** provider credentials → self-
  authenticating `wss://` CDP URL, the piece in front of `connect()`. New
  `gateway.rs` module (kept OUT of `anchortree-core` — provider plumbing, not
  identity logic): `AcquiredSession { connect_url, session_id }`;
  `gateway::cloudflare::devtools_ws_url(account, token)` builds the Browser Run
  `?token=` URL with no round-trip (RFC-3986 unreserved-only percent-encode of
  the token), `gateway::browserbase::acquire(project, key)` mints a session over
  REST (`POST /v1/sessions`, `X-BB-API-Key`) and parses out `connectUrl`. Pure,
  unit-testable request-build / response-parse functions carry the real bug
  surface (12 new tests: URL build, query encode, body shape, reply parse, error
  snippet truncation); the network call is gated behind the `observe_hosted`
  example. `GatewayError` (`Http`/`Status{status,body}`/`Malformed`) added to
  `error.rs`. reqwest pulled in with `default-features = false, features =
  ["rustls-no-provider", ...]` so it reuses our installed **ring** provider
  instead of forcing aws-lc-rs (D10) — `cargo tree` confirms no aws-lc. The
  shared ring installer `ensure_ring_provider` is now `pub(crate)`.
  **Live-verified:** `observe_hosted` against real Browserbase minted live
  sessions every run and returned `wss://connect.*.browserbase.com/?signingKey=…`
  + a replay link (example redacts the credential before printing); exits 0.
  **Open (D19):** the hosted *connect* leg. chromiumoxide 0.9.1 cannot cleanly
  attach to the page a hosted browser already has open — `new_page` panics
  (`Target.createTarget` response races the `targetCreated` event,
  `handler/mod.rs:208`); `fetch_targets` registers the page but its
  `Target.getTargets` handler attaches a **non-flat** session
  (`AttachToTargetParams::new`, `handler/mod.rs:225`) so domain commands fail
  `-32001 Session with given id not found`, and `get_or_create_page` caches that
  first (poisoned) session permanently; with neither call, Browserbase fires no
  `targetCreated` for its pre-existing page within 5s. `connect()` is left at its
  proven local-`ws://` `new_page` form — unchanged, not regressed.
- **Phase 3.1b hosted connect leg DONE (run 12):** D19 resolved via D20 — a
  self-contained thin CDP channel flat-attaches to the page a hosted browser
  already has open and drives the full observe→rebind loop over it, with NO
  chromiumoxide bump and NO fork. New `channel.rs` module. The seam is a sealed
  `pub trait CdpChannel` with one method, `fn run<T: Command>(&self, cmd: T) ->
  impl Future<Output = Result<T::Response, CdpError>> + Send` — the explicit
  `+ Send` RPITIT bound is load-bearing (it keeps the generic
  `ObservationSource::observe` `Send`, which an `async fn` in a trait cannot
  express; hence `#[allow(clippy::manual_async_fn)]` on the impls). `CdpObserver`
  was made generic — `CdpObserver<C = Page>` — so the ENTIRE fusion/listener/decode
  pipeline is shared byte-for-byte across the local `Page` transport and the hosted
  raw channel (no protocol fork; the only divergence is the wire layer). `impl
  CdpChannel for Page` keeps the local `new_page` path identical; `impl CdpChannel
  for RawCdpSession` is the new flat transport: `connect_hosted(ws_url)` connects
  the `wss://`, issues `Target.attachToTarget{flatten:true}` once, captures the
  `sessionId`, then tags every later command as a flat envelope (`{id, method,
  params, sessionId}`) over one multiplexed WebSocket, matching responses by
  numeric `id`. `RawCdpSession` reuses the typed `chromiumoxide_cdp` `Command`
  structs for (de)serialization. `HostedSession { observer: CdpObserver<RawCdpSession> }`
  exposes `navigate`/`evaluate` convenience and the shared `observer`. Pure helpers
  (`build_envelope`, `response_for`, `select_page_target`) carry the wire-format
  bug surface as 9 new unit tests. Sealing the trait satisfies `private_bounds`
  while keeping `CdpObserver<C>` public. New gated example `connect_hosted` mirrors
  `observe_rerender` but over the hosted leg (Browserbase creds win, else local
  `ANCHORTREE_CDP_WS`/`_HTTP`, else prints usage + exits 0 — CI-safe). **Live-
  verified against BOTH transports:** a local `ws://` headless-shell (flat-attached
  to a pre-existing page — first-observe backendNodeIds 3–6 prove it was not freshly
  created; all 4 eids rebound across an `innerHTML` swap; in-place edit on the cheap
  changed path) AND real Browserbase `wss://` (session `1fdeb2f2-…`, same full
  acquire→connect→observe→rebind loop, rebind ledger 10→19, 11→20, 12→21, 13→22).
  89 tests green (49 cdp +9, 36 core, 2 integration, 2 doctests); clippy/fmt clean.
  Confirms D19 + D20.
- **What does NOT exist yet:** the visual SoM escalation (2.2b); the Phase 3.2
  multi-frame / iframe identity; the Phase 3.3 benchmark harness; crates.io publish.

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

**Recommendation (updated research run 13):** **Phase 3.2a is SHIPPED** —
same-origin multi-frame durable identity (D21 mechanics 1+2+4) live-verified
against a real `srcdoc` iframe (`016ae2a`), with one live correction: same-origin
frames are free from the pierced **DOM** pass but NOT the **AX** pass, so the
observer now issues one `getFullAXTree(frameId)` per same-origin frame and merges.
**The top unchecked item is Phase 3.2b — cross-origin OOPIFs (D21 mechanics 3+5),
and research run 13 settled the channel design as D22.** The blocker: the run-12
`CdpChannel` is single-session (`RawCdpSession` one `session_id`, `channel.rs:118`;
`run` tags every request, `:155`), and OOPIFs are a separate CDP target with their
own session that `getDocument{pierce:true}` does not reach. Build, in order:
1. **Multi-session write path** — add `run_on(session_id, cmd)`. `next_id()` is
   shared-monotonic and `response_for` (`:247`) demuxes by `id` alone, so the read
   side is unchanged; only the write side tags the session. Default = page session
   (run-12 fast path byte-identical).
2. **Event-harvest read path** — the loop discards all events
   (`ResponseFor::Other => continue`, `:200`). Issue
   `setAutoAttach{autoAttach:true, flatten:true, waitForDebuggerOnStart:false}` on
   the root session, then drain `Target.attachedToTarget` **events** to record each
   child `sessionId` + `targetInfo`. This event path is the one genuinely new
   surface.
3. **Frame-key ↔ session join** — an OOPIF subframe target's `targetInfo.targetId`
   equals its page `frameId` (present in the root `Page.getFrameTree`); join the
   child session to the durable frame-key by `targetId == frameId`. Assert live.
4. **Per-child observe** — enable the domains on each child session, run
   `getDocument(pierce)` + `getFullAXTree` (no frameId; OOPIF doc is the child
   root), fold under the frame-key. Run-13 AX-per-frame correction: one AX call per
   child session.
5. **Dispatch on the owning session** — `actions.rs` resolveNode + click/type/
   select run on the owning frame's session (eid carries/looks up its sessionId).
   `(frame-key, backendNodeId)` map key from 3.2a already prevents the cross-OOPIF
   collision; no map change.
Keep the single-frame and same-origin fast paths untouched so the run-4/12/13
proofs do not regress. Live-verify: a page with one cross-origin iframe whose widget
is structurally identical to a root widget yields two distinct durable eids that
both rebind across an `innerHTML` swap, dispatched on their owning sessions, exit 0.
Builder confirms D22 when this lands. After 3.2b, open the larger **Phase 3.3
benchmark** (WebArena-Verified, D17) as its own multi-run arc — the highest-leverage
thesis item (quantifies LLM re-grounding calls eliminated) but too big for one run.
**Market tailwind (research run 13):** browser-use's *"Leaving Playwright for CDP"*
confirms the frontier is moving to raw-CDP control — anchortree's exact layer.
**Still deferred:** the visual SoM escalation (**2.2b**, feature-gated, DOM-less
case only).

## Pointers

- `GENESIS_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/e97911dd-5071-437e-b7ba-a64a58e9f7e1.jsonl`
  (the first human+Truffle session: thesis, Browserbase test, the full project
  brief, and this scaffold). Richest context on original intent.
- `LAST_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/9a3a8935-c8fa-44d2-bca4-fe4ba6d0a517.jsonl`
  (builder run 12: Phase 3.1b the hosted connect leg — `channel.rs` (sealed
  `CdpChannel` trait, `RawCdpSession` flat-attach, `HostedSession`, `connect_hosted`,
  9 wire tests), `CdpObserver<C = Page>` generic refactor in `observer.rs`, the
  gated `connect_hosted` example, live-verified against both a local `ws://`
  headless-shell and real Browserbase `wss://`, 89 tests green; D19 + D20 confirmed).
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
  chromiumoxide patch, 68 tests; and builder run 11 Phase 3.1 acquire leg —
  `gateway.rs` (`cloudflare::devtools_ws_url` + `browserbase::acquire`),
  `GatewayError`, reqwest `rustls-no-provider`, the `observe_hosted` example
  live-verified against Browserbase, and the D19 hosted-connect-leg
  characterization, 81 tests).
- `LAST_TRANSCRIPT` (research): `/home/phantom/.claude/projects/-app/d56cc454-10a4-42bf-9164-b84e3d58ae26.jsonl`
  — research runs 3–12. Tested the 1.5a `ws://` recipe, pinned the 2.1 action
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
  helper returning a credential-in-URL `wss://` connected header-less (D18); then
  (run 11) settled the D19 connect-leg fix path — bumping chromiumoxide is a dead
  end (`0.9.1` newest, no `main` movement on `handler/{mod,target}.rs`) and
  wrapping the flat session as a `chromiumoxide::Page` is unreachable (private
  `PageInner`, sessionless `Browser::execute`), so the connect leg becomes a
  self-contained thin CDP channel behind `ObservationSource` that flat-attaches
  and routes session-tagged commands itself (D20); then (run 12, after the builder
  shipped D20) settled the Phase 3.2 multi-frame design — two-tier durable eid
  `(frame-key, in-frame fingerprint)`, same-origin frames free from the pierced
  pass via `node.frame_id`, OOPIFs flat-attached on our own channel via
  `setAutoAttach{flatten:true}`, resolve map re-keyed `(frame-key, backendNodeId)`,
  actions dispatched on the owning frame's session (D21), confirming all CDP
  primitives present in chromiumoxide_cdp 0.9.1.
- Remote: `github.com/truffle-dev/anchortree`.
- Project page: `truffleagent.com/anchortree` (pending).

## Open questions to resolve (hand to research cron)

- RESOLVED (research run 13 → D22): how does the single-session run-12 channel
  reach cross-origin OOPIFs for 3.2b? Answer: it must become multi-session. The
  `RawCdpSession` holds one `session_id` (`channel.rs:118`) and the read loop
  discards all events (`:200`); OOPIFs are learned via `setAutoAttach{flatten:true}`
  `Target.attachedToTarget` **events** and driven on their own child sessions. The
  build is a `run_on(session)` write path + a one-shot event-harvest read path +
  the `targetId == frameId` frame-key join + per-child `getDocument`/`getFullAXTree`
  + owning-session dispatch. The `(frame-key, backendNodeId)` map key from 3.2a
  already prevents the cross-target collision. D22 PROPOSED; builder confirms when
  3.2b lands. No chromiumoxide upgrade or fork (run-11 finding holds).
- PARTIALLY CONFIRMED (builder run 13): D21 mechanics 1+2+4 shipped as 3.2a, with
  the live correction that same-origin frames are free from the DOM pass but each
  needs its own `getFullAXTree(frameId)` (AX trees stop at frame boundaries).
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
  CONFIRMED (builder run 11): the acquire leg shipped exactly this way and
  live-verified against Browserbase, but building it revealed the *connect* leg
  is a separate, real block — recorded as D19.
- RESOLVED (research run 11 → D20): the Phase 3.1 hosted *connect*-leg fix path is
  settled. The two preferred D19 paths both fail: bumping chromiumoxide is a dead
  end (`0.9.1` is the newest crates.io release, 2026-02-25; `main` has zero commits
  to `src/handler/{mod,target}.rs` since then; open PRs #322/#323 do not touch flat
  auto-attach), and wrapping the flat session as a `chromiumoxide::Page` is
  unreachable through the public API (`Page` builds only via `From<Arc<PageInner>>`
  at `page.rs:1384`, `PageInner` crate-private; `Browser::execute` is sessionless,
  no public `execute_with_session`). Build the connect leg as a self-contained thin
  CDP channel behind the existing `ObservationSource` seam: connect the `wss://`
  URL, issue `Target.attachToTarget{flatten:true}` once, capture the `sessionId`,
  route later commands as flat messages tagged with that session (reusing
  `chromiumoxide_cdp` `Command` structs), implement `ObservationSource` directly.
  Do NOT reuse `chromiumoxide::Page` and do NOT fork; an upstream PR is optional
  parallel good-citizenship, not the critical path. Proposed; builder confirms when
  the connect leg lands and live-verifies against Browserbase.
  **CONFIRMED (builder run 12): D19 + D20 both confirmed. The connect leg shipped
  exactly as D20 specified — sealed `CdpChannel` trait, `CdpObserver<C = Page>`
  generic, `RawCdpSession` flat-attach over one multiplexed `wss://`, the typed
  `chromiumoxide_cdp` Command structs reused for (de)serialization, no fork, no
  bump. Live-verified against BOTH a local `ws://` headless-shell AND real
  Browserbase `wss://`. 89 tests green. Phase 3.1 complete end to end.**
- RESOLVED (research run 12 → D21): the Phase 3.2 multi-frame / iframe identity
  design is settled from primary sources. Durable eid becomes two-tier
  `(frame-key, in-frame fingerprint)`: in-frame = the existing fingerprint computed
  within the owning frame's subtree; frame-key = the frame's parent-chain ordinal
  path from `Page.getFrameTree` (durable across reloads), NOT the raw `frameId`.
  Same-origin iframes are free from the existing pierced pass (`node.frame_id` +
  `content_document` already present); OOPIFs are discovered + flat-attached on our
  own channel via `Target.setAutoAttach{autoAttach:true, flatten:true,
  waitForDebuggerOnStart:false}` (run-12 thin channel, 1 session → N); the resolve
  map is re-keyed `(frame-key, backendNodeId)` because backendNodeIds collide
  across OOPIF targets; actions dispatch on the owning frame's session. Every CDP
  primitive confirmed present in chromiumoxide_cdp 0.9.1
  (`GetFullAxTreeParams.frame_id`, DOM `Node.frame_id`/`content_document`,
  `Target.setAutoAttach`, `Page.getFrameTree`) — no fork, no raw-WS fallback.
  **SHIPPED 3.2a (builder run 13): mechanics 1+2+4** live-verified against a real
  same-origin `srcdoc` iframe. **CORRECTION to mechanic 2:** same-origin frames
  are free from the pierced *DOM* pass (the `backend→FrameKey` map comes from the
  inline `content_document` subtrees), but they are NOT free from the *AX* pass —
  `getFullAXTree` with no frameId stops at every frame boundary, so the observer
  issues one `getFullAXTree(frameId)` per same-origin frame (discovered via
  `frames::same_origin_frame_ids`) and merges the results; backend ids are unique
  across the root target's pierced id space so the merge cannot collide. Mechanics
  3+5 (OOPIF auto-attach + owning-session action dispatch) deferred to 3.2b.
- RESOLVED (builder run 2): D9 CONFIRMED. `RawAxNode` is the transport-neutral
  fusion boundary; `fuse.rs` and `anchortree-core` carry zero chromiumoxide refs,
  and the new 1.3 recorded-reply decode test is the first non-live consumer of
  the seam. A future `anchortree-bidi` adapter reuses `fuse::fuse` unchanged.
- Differentiation locked (research run 1): the peer to beat is Stagehand v3.
  Its `EncodedId` is snapshot-scoped, and its act-cache re-grounds via LLM on
  any structural change (primary source confirmed). anchortree's edge is
  rebinding the logical id *through* the re-render. This is the Phase 3.3
  benchmark headline and the Phase 4.3 blog thesis.
