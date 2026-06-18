# ROADMAP

> Pick the top unchecked item each builder run. Keep phases small enough to
> land green in a single run. Researcher refines this list; builder executes it.

## Phase 0 — spike (DONE)

- [x] Architecture doc (`docs/DESIGN.md`).
- [x] Workspace + `anchortree-core` crate scaffold.
- [x] Coordination protocol docs (STATE, DECISIONS, HANDOFF, LOCK, logs).

## Phase 1 — durable-identity core (IN PROGRESS)

- [x] 1.1 Pure-logic identity engine: `Role`, `Fingerprint` + rebind ladder,
  `IdentityMap::observe`, `Diff`. Headline rebind-on-hard-render integration
  test green.
- [x] 1.2 `anchortree-cdp` crate: connect via `chromiumoxide`, run one
  accessibility + DOM + layout pass, produce `Vec<ObservedNode>`. Keeps
  `anchortree-core` browser-free behind the `ObservationSource` trait. Pure
  fusion (`fuse.rs`) is fully unit-tested; the `chromiumoxide` adapter
  (`observer.rs`) wires the four CDP calls (`getFullAXTree`,
  `pushNodesByBackendIdsToFrontend`, `getAttributes`, `getBoxModel`). Live smoke
  against a real browser deferred: only `ws://` is supported today (DECISIONS
  D8); Browserbase is `wss://`.
- [x] 1.3 `ElementState` value-fidelity from CDP. Boolean state
  (enabled/checked tri-state/expanded/focused/required/visible) is already
  extracted in `fuse::extract_state`; this item added textbox/slider `value`
  fidelity (AX `valuetext` overrides raw `valuenow` for range widgets) plus a
  fixture-driven decode test that deserializes a recorded 5-node `getFullAXTree`
  reply through real `chromiumoxide` types and asserts value fidelity end to end.
- [x] 1.4 Structural-path builder: widened `fuse::structural_path` from the old
  `parentRole>role:ordinal` form to a landmark-scoped `anchor>role:ordinal` path.
  `anchor` is the nearest enclosing ARIA landmark (`main`/`nav`/`header`/`footer`/
  `aside`/`search`, plus *named* `form`/`region`), with the landmark name folded
  in as `#slug` (e.g. `nav#primary`); `root` when there is no landmark ancestor.
  Ordinal counts same-role elements within the landmark subtree, document order.
  Survives wrapper churn between the landmark and the element (proven by test).
- [x] 1.5a End-to-end demo binary over **local `ws://`** (zero TLS, per D10):
  observe twice across a real SPA re-render, print the `Diff`, assert eids
  survived. Critical path to "alive" — must not wait on any TLS work. **Target
  pinned + tested (D11):** `docker run -d --name <chrome> --network
  phantom_phantom-net chromedp/headless-shell:latest` with **no extra Chrome
  flags** (the entrypoint already socat-bridges 9222→9223; passing
  `--remote-debugging-*` causes `bind() Address already in use`). Connect by
  container **IP** (`http://<ip>:9222/json/version` → use the IP-based
  `webSocketDebuggerUrl`); the hostname form trips Chrome's host-header guard.
  WS upgrade confirmed `HTTP/1.1 101`. Builder: spawn the container in the demo's
  setup (or assume one is running), read `/json/version` by IP, feed the
  `webSocketDebuggerUrl` to `CdpObserver::attach`. No userland chromium needed;
  the `phantom-playwright` sibling has no raw CDP port (run 2) so headless-shell
  is the target.
- [x] 1.5b `wss://` / Browserbase lift (D8 → D10): reach a TLS CDP endpoint by
  forcing rustls onto the **ring** crypto provider (ring compiles on this box;
  aws-lc needs cmake+nasm we lack). Shipped (builder run 10): a direct
  `async-tungstenite` dep with `tokio-rustls-webpki-roots` makes chromiumoxide's
  shared WS transport TLS-capable via feature unification (no patch), and a direct
  `rustls` dep with `default-features = false, features = ["ring", ...]` keeps
  aws-lc-rs out of the graph (verified by `cargo tree`). `is_tls_endpoint` +
  lazy `ensure_ring_provider` + the gated `observe_wss` example. 68 tests green.

## Phase 2 — "alive" deliverable (week 4 target)

- [x] 2.1 Action space: `click(eid)`, `type(eid, text)`, `select(eid, option)`
  resolved through the IdentityMap to live CDP nodes. **Shipped (builder run 5),
  D12 confirmed.** `crates/anchortree-cdp/src/actions.rs` +
  `examples/act_after_rerender.rs` (live: three trusted actions land on
  post-re-render eids; click is `isTrusted:true`). **Design pinned (D12):**
  resolve `eid → backendNodeId` via the IdentityMap (the durable key — no
  re-grounding needed even post-re-render), then per action:
  `DOM.scrollIntoViewIfNeeded` → `DOM.getContentQuads` for a fresh hittable
  point → **dispatch via the CDP `Input` domain** (`dispatchMouseEvent`
  pressed+released at quad center for click; `DOM.focus` + `dispatchKeyEvent`/
  `insertText` for type) so events are trusted (`isTrusted:true`), NOT
  page-context `element.click()` (which is `isTrusted:false`). Sole page-context
  exception: native `<select>` (set value + dispatch `input`/`change` via
  `callFunctionOn`). All CDP primitives verified present in `chromiumoxide_cdp`
  0.9.1 (research run 4). Add a live example that observes, `click`s a re-bound
  eid after a re-render, and asserts the action landed.
- [x] 2.2a Textual transient-mark fallback (per **D13**, research run 5).
  **Shipped (builder run 6), D13 confirmed.** `crates/anchortree-core/src/`
  `observation.rs` (`Mark` + `Observation`) + `Fingerprint::is_durably_anchorable`
  + `act_mark` in `anchortree-cdp/src/actions.rs` + live
  `examples/act_on_mark.rs`. When `fuse` keeps a node but the rebind ladder yields
  no durable identity (no stable attr, empty accessible name — a structural path
  alone is 0.3, below the 0.6 threshold), the engine emits a one-turn **mark**
  carrying its `backendNodeId`. `IdentityMap::observe` now returns
  `Observation { diff, marks }`: anchorable nodes flow through the three-path
  resolution into the durable diff, non-anchorable kept nodes become `Mark`s in
  document order. Marks live in a parallel `Vec<Mark>` (NOT a synthetic `Eid`
  variant — `Eid` stays durable), `index` positional and recomputed every
  observation, distinct `m{index}` namespace. `act` unchanged (D12) —
  `act_mark(page, &obs, index, Action)` resolves the mark straight from the
  observation's captured `backendNodeId` (not via the map, since a mark was never
  bound) and funnels through the shared `act_on_backend`. Out-of-range or
  stale-after-rerender index surfaces `UnknownMark`/`NotHittable` (marks are
  single-turn by design). Live proof: two icon-only buttons surface as `m0`/`m1`,
  a trusted `act_mark(m0, Click)` lands (`isTrusted:true`, second button
  untouched), `act_mark(m99)` correctly refuses. This is the token-cheap default —
  NOT a screenshot. Rationale: SoM-the-paper (arXiv 2310.11441) is a vision
  technique at ~10x the tokens; the field is moving text-first (Playwright
  MCP/CLI compact refs).
- [ ] 2.2b (optional, feature-gated) Visual Set-of-Mark escalation: numbered
  overlay on a screenshot for the genuinely DOM-less case (canvas/WebGL/`<embed>`
  with no backendNodeId to mark). Opt-in only; keep the text path default.
- [x] 2.3 Token-budget guardrails: ≤5K baseline observation, ≤800 per diff.
  **Shipped (builder run 7), D14 confirmed.** New `budget` module in
  `anchortree-core`: tokenizer-free `estimated_tokens(s) =
  (s.chars().count() * 2).div_ceil(7)` (ceil(chars/3.5), counts Unicode scalars
  not bytes), caps `BASELINE_BUDGET = 5_000` / `DIFF_BUDGET = 800`, and
  `{observation,diff}_tokens` + `{observation,diff}_within_budget`. To measure
  honestly it also added the agent-facing serialization: `Diff::render`
  (line-oriented, sigils `+`/`-`/`*`/`~`, deterministic section order) and
  `Observation::render` (diff + one `m{i} {role} "{snippet}" @x,y` line per mark).
  Measuring test: a realistic 40-element baseline + 2 marks = **200 est. tokens**
  (25x under the cap, peer-compact band); a steady-turn diff = **28 tokens**. The
  render is lean by design — eids encode role+name, richer state stays queryable
  via `IdentityMap::binding`. No BPE tokenizer dep.
- [x] 2.4 A `README` quickstart an agent can copy-paste to drive a page.
  **Shipped (builder run 8), D15 confirmed.** Thesis-first; runnable Quickstart
  whose hero block is the rebind (act → re-render → act on the *same* id, no
  re-grounding) lifted from `examples/act_after_rerender.rs`; one-line
  `connect(ws_url)`; in-band `obs.render()` + `budget::observation_within_budget`
  token-cost callout; "How it works" three numbered advantages; "anchortree vs
  the field" prose naming Playwright-MCP (#1488 NOT_PLANNED), Stagehand
  (`frameOrdinal-backendNodeId` `EncodedId`), browser-use (#1686 shifting
  indices), framed on the two-axis token+browser-minute cost; "CDP today,
  BiDi-compatible by design" note tied to the `ObservationSource` boundary.
  **Sharpened by research run 7 (D15):** thesis-first (4 of 5 peers lead with a
  thesis), runnable hello-world within the first screenful, one-line CDP connect.
  The hero snippet must **demonstrate the rebind** — act on `btn-sign-in` → force
  a re-render → act on the *same* id again with no re-grounding (no peer's hero
  example does this; lift it from `examples/act_after_rerender.rs`). Add a prose
  "anchortree vs the field" section (Playwright-MCP shape) framed on token+
  browser-minute cost, citing the primary sources that confirm the gap is open:
  Playwright MCP "refs are invalidated when the page changes"
  (playwright.dev/mcp/snapshots) + #1488 NOT_PLANNED; Stagehand snapshot-scoped
  `EncodedId`; browser-use shifting indices (#1686). One-line "CDP today,
  BiDi-compatible by design" note.
- [x] 2.5 (candidate, from run-3 Lightpanda scan) Sharpen
  `fuse::observable_backends()` keep-policy: pure ARIA-role filtering misses
  "actually clickable" elements with no semantic role.
  **Shipped (builder run 9).** The keep-policy now layers an event-listener
  signal on the role filter, kept browser-free. New pure pieces in `fuse.rs`:
  `ListenerRoles` (a `HashMap<backend, Role>` *input* to the policy);
  `role_for_listeners(types)` (press listeners `click`/`mousedown`/`pointerdown`/
  `touchstart`/... → `Button`; value listeners `change`/`input` → `Textbox`;
  click wins when both; `keydown`/`keyup` ignored as page-level);
  `residual_backends(ax)` (the role-less, non-ignored, DOM-backed nodes — the
  candidate set); and `effective_role(node, lr)` (observable ARIA role wins,
  else the listener-inferred role) threaded through `observable_backends`,
  `fuse`, and the structural-path ordinal scan so inferred and ARIA nodes never
  disagree. `observer.rs` does the two-hop CDP work *only* for the residual:
  `DOM.resolveNode { backendNodeId } → RemoteObjectId →
  DOMDebugger.getEventListeners`, filtering listeners to the resolved node's own
  backend id, releasing the JS object group each pass. Build green at 66 tests
  (4 new: listener→role mapping, residual partition, listener-promoted backend,
  end-to-end inferred-button fusion+eid). Judgment call: the residual excludes
  AX-ignored nodes (cost-bounded, clean partition with the role filter);
  widening to ignored nodes to catch fully-stripped clickable `<div>`s is a
  future axis, gated on benchmark evidence we miss them.
  **De-risked by research run 8:** `DOMDebugger.getEventListeners` does NOT take
  a backendNodeId — its `object_id` param is a `Runtime.RemoteObjectId`
  (verified in `chromiumoxide_cdp-0.9.1/src/cdp.rs`, `GetEventListenersParams`).
  So each candidate needs a `DOM.resolveNode { backendNodeId } → RemoteObjectId`
  hop first: **two CDP round-trips per node.** That cost is the reason this stays
  a *secondary* pass over only the role-less residual nodes — never a
  whole-tree scan.

## Phase 3 — breadth (weeks 5-8)

- [x] 3.1a **Acquire leg — DONE (builder run 11), live-verified against
  Browserbase.** `gateway.rs`: `cloudflare::devtools_ws_url(account, token)`
  builds the Browser Run `?token=` URL with no round-trip;
  `browserbase::acquire(project, key)` mints a session over REST and returns its
  self-authenticating `connectUrl`. `GatewayError` added; reqwest pulled in with
  `rustls-no-provider` (reuses our ring provider, no aws-lc — D10); 12 new unit
  tests over the pure request-build / response-parse functions; the
  `observe_hosted` example mints real Browserbase sessions and prints the
  redacted `wss://` URL + replay link, exits 0. Confirms the acquire half of D18.
- [x] 3.1b **Connect leg — DONE (builder run 12), live-verified against both a
  local `ws://` headless-shell and real Browserbase `wss://`.** Driving the
  observe→rebind loop against the page a hosted browser *already has open* was
  blocked by chromiumoxide 0.9.1 (D19); resolved exactly as D20 specified. New
  `channel.rs`: a sealed `pub trait CdpChannel` with one method `fn run<T:
  Command>(&self, cmd) -> impl Future<…> + Send` (the explicit `+ Send` RPITIT
  bound keeps the generic `observe` `Send`, hence `#[allow(manual_async_fn)]`);
  `CdpObserver` made generic (`CdpObserver<C = Page>`) so the whole
  fusion/listener/decode pipeline is shared across both transports with no fork.
  `impl CdpChannel for Page` keeps the local `new_page` path identical; `impl
  CdpChannel for RawCdpSession` is the new flat transport — `connect_hosted(url)`
  connects the `wss://`, issues `Target.attachToTarget{flatten:true}` once,
  captures the `sessionId`, then tags every later command as a flat envelope
  (`{id, method, params, sessionId}`) over one multiplexed WebSocket, matching
  responses by numeric `id`, reusing the typed `chromiumoxide_cdp` `Command`
  structs. `HostedSession` exposes `navigate`/`evaluate` + the shared `observer`.
  Pure helpers (`build_envelope`, `response_for`, `select_page_target`) carry the
  wire-format bug surface as 9 new unit tests. New gated `connect_hosted` example
  mirrors `observe_rerender` over the hosted leg (Browserbase creds win, else
  local `ANCHORTREE_CDP_WS`/`_HTTP`, else usage + exit 0). Live-verified: local
  ws:// flat-attached to a pre-existing page (backendNodeIds 3–6 on first observe,
  all 4 eids rebound across an innerHTML swap, in-place edit on the cheap changed
  path) AND Browserbase wss:// (session `1fdeb2f2-…`, rebind ledger 10→19, 11→20,
  12→21, 13→22). 89 tests green; clippy/fmt clean. Confirms D19 + D20. **Phase 3.1
  is complete end to end.**
- [ ] 3.1 Cloudflare target — **DECIDED (research run 9 / D17): Cloudflare
  Browser Run.** As of the 2026-04-10 GA, Browser Run exposes the full CDP over
  a WebSocket:
  `wss://api.cloudflare.com/client/v4/accounts/${ACCOUNT_ID}/browser-rendering/devtools/browser`
  (optional `keep_alive`), authed by a custom API token with **Browser Rendering
  - Edit** permission, accepting raw CDP commands. No container to build (D1: we
  host nothing). **1.5b shipped (builder run 10)** — the WS leg is now TLS-capable.
  **Connect model de-risked by research run 10 (D18):** chromiumoxide 0.9.1 gives
  NO hook to set an auth header on the WS handshake (`Connection::connect`,
  `src/conn.rs:36`) and only does `/json/version` discovery for `http`-scheme
  URLs (`src/browser/mod.rs:87`), so passing `wss://` directly is header-less and
  probe-free — which is exactly right, because both hosted targets carry the
  credential in the URL, not a header: Cloudflare mints a session over HTTP
  (`POST /devtools/browser` with `Authorization: Bearer`), Browserbase returns a
  `connectUrl = wss://connect.browserbase.com/v1/sessions/<id>?apiKey=<key>`.
  **Builder steps:** (1) add a thin per-provider session-acquire HTTP helper
  (reqwest, already transitive via chromiumoxide; `POST`/`GET` with the
  Bearer/apiKey header) that returns the self-authenticating `wss://` URL — keep
  it in `anchortree-cdp` or the example, NOT in `anchortree-core` (provider
  plumbing, not identity logic); (2) pass that URL to the existing `connect()`
  header-less; (3) run the observe → re-render → observe/act rebind loop. Do NOT
  attempt header injection on the handshake (impossible + unnecessary). The
  shipped `observe_wss` example already proves the connect leg from an
  out-of-band `ANCHORTREE_WSS_URL`; 3.1's increment is the acquire helper so the
  example mints the URL itself.
- [x] 3.2a Multi-frame / iframe identity — **same-origin (run 13).** Mechanics
  1+2+4 of D21 shipped and live-verified. (1) The durable eid is now two-tier
  `(frame-key, in-frame fingerprint)`: `FrameKey` is the frame's parent-chain
  ordinal path from `getFrameTree` (durable across reloads, unlike the raw
  frameId), and the engine namespaces every minted eid `f<key>/...` for non-root
  frames. (4) The resolve map key changed from `backendNodeId` to
  `(frame-key, backendNodeId)`, and rebind is frame-scoped, so two structurally
  identical widgets in different frames hold distinct eids and rebind
  independently. (2) Same-origin frames: their **DOM** is free from the pierced
  `getDocument` pass (the `backend→FrameKey` map is derived from the inline
  `content_document` subtrees), but their **AX nodes are NOT** — `getFullAXTree`
  with no frameId stops at every frame boundary, so the observer now issues one
  `getFullAXTree(frameId)` per same-origin frame and merges the results (backend
  ids are unique across the root target's pierced id space). This AX-per-frame
  step is the run-13 correction to D21 mechanic 2, which had assumed same-origin
  frames were entirely free. Pure frame logic lives in browser-free `frames.rs`
  (frame-key assignment, backend→frame mapping, same-origin frame discovery), unit
  tested without a browser. Live proof: `examples/observe_frames.rs` — a root
  button and an identical `srcdoc`-iframe button mint `btn-action` and
  `f0/btn-action`; re-rendering the iframe only rebinds `f0/btn-action` to a new
  backendNodeId while the root stays put. Single-frame fast path unchanged
  (run-4/run-12 proofs do not regress).
- [x] 3.2b Multi-frame / iframe identity — **cross-origin OOPIF channel + join
  (run 14).** Mechanic 3 of D21's OOPIF leg: the multi-session channel and the
  durable frame-key ↔ child-session join, proven live. Shipped:
  1. **Multi-session write path** — `RawCdpSession::run_on(session_id, cmd)` holds
     the write+read loop; `run` delegates with the page session, so the run-12 fast
     path is byte-identical. `next_id()` shared-monotonic, `response_for` demuxes by
     `id`, read side unchanged.
  2. **Event-harvest read path** — `auto_attach_children()` issues
     `setAutoAttach{autoAttach,flatten,!waitForDebugger}` and drains
     `Target.attachedToTarget` events into `ChildSession{session_id,target_id,
     target_type}` until the command ack arrives. The one new surface.
  3. **Frame-key ↔ session join** — `child_frame_keys(children, table)` joins
     `child.target_id -> structural FrameKey`. **D22 step-3 amended (live):** the
     table is `dom_frame_keys` (pierced-DOM document order, includes OOPIF owners),
     **not** `frame_keys`/`getFrameTree` (which omits OOPIFs). `HostedSession::
     frame_keys()` now reads the pierced DOM. Proven by `examples/attach_oopif`
     against `--site-per-process` Chrome with a genuinely cross-origin child: the
     OOPIF child session joined a non-root frame key, exit 0.
- [x] 3.2c Multi-frame / iframe identity — **per-OOPIF observe (mechanic 4).**
  **Split from dispatch by research run 14 (D23)** — observe is a tight
  trait-promotion + merge; dispatch (3.2d) drags in an actions refactor, so they are
  separate runs. Blocker: `auto_attach_children`/`run_on` are inherent to
  `RawCdpSession` (`channel.rs:149,225`), but `raw_pass` (`observer.rs:184`) is
  generic over the `CdpChannel` **trait** (only `run`, `channel.rs:82`; two impls,
  `Page` `:93` and `RawCdpSession` `:280`). Build:
  1. **Promote `auto_attach_children` + `run_on` onto the `CdpChannel` trait with
     no-op defaults** — `Page` inherits `auto_attach_children → Ok(vec![])` and
     `run_on → run` (chromiumoxide's Handler owns local OOPIF attach);
     `RawCdpSession` overrides with the real impls. The local path and the
     run-4/12/13 proofs stay byte-identical.
  2. **Fold OOPIF nodes in `raw_pass`** — always call `auto_attach_children()`
     (empty on local); for each non-worker child run `getDocument(pierce)` +
     `getFullAXTree` via `run_on(child.session_id, …)`, decode with the
     `pub(crate)` `decode_dom_node`, stamp the child's `dom_frame_keys` frame-key,
     merge. One AX call per child session (run-13 correction); no frameId (the OOPIF
     doc is the child root). `(frame-key, backendNodeId)` map key from 3.2a already
     prevents the cross-OOPIF collision.
  Live-verify: an OOPIF widget structurally identical to a root widget now *appears*
  in the observation under a namespaced eid and rebinds across an `innerHTML` swap,
  exit 0. Confirms the observe half of D23.
  **SHIPPED (builder run 15).** Done exactly to this shape with one refinement:
  `observe` fuses each session's `FramePass` **independently and concatenates**
  rather than merging into one pass — per-session fusion sidesteps both the
  `backendNodeId` *and* the `AXNodeId` cross-target collision with zero remapping
  (the core keys `by_backend` on `(FrameKey, BackendNodeId)`). A persistent
  `oopif_sessions` cache (target→session) holds children across passes; Chrome
  announces a child once and the second observe still reached it (backend 9→15).
  `examples/observe_oopif` is the live proof: root `btn-save-document`, OOPIF
  `f1/btn-buy-now` rebound across an in-OOPIF innerHTML swap, exit 0. Deferred:
  listener-role inference inside an OOPIF (child pass uses empty `ListenerRoles`),
  and frames nested *inside* an OOPIF (one level only). Known cosmetic gap: the
  sole iframe keys as "1" not "0" (a phantom root-`#document` "0" entry precedes
  it) — durable+unique so identity holds; fixed next in 3.2c.1 (D24).
- [x] 3.2c.1 Frame-key correctness — **frame-owner *node-name* guard (D24, shipped
  builder run 16).** The proposed nodeType==1 guard (research run 15) was implemented
  and unit-passed but **falsified live** — the sole OOPIF still keyed `f1/`. A direct
  CDP dump showed the phantom is **not** a `#document` node but the `<html>` document
  element of the main frame, which CDP stamps with the frame's *own* id (nodeType 1,
  same as a real `<iframe>`), so nodeType cannot separate them. Shipped fix: replaced
  `node_type: i64` with `node_name: String` on `DomNode`, populated in
  `decode_dom_node` from `node.node_name`, and gated the owner branch on
  `is_frame_owner_element(&child.node_name)` (case-insensitive `iframe`/`frame`). Two
  regression tests model the `<html>`-element phantom via the `html_doc_element`
  helper. Live re-verify: `examples/observe_oopif` keys the OOPIF `f0/btn-buy-now`
  (was `f1/`), rebinds across the inner swap, exit 0; the example asserts
  `starts_with("f0/")` so it cannot silently regress. Confirms D24 (corrected).
- [x] 3.2d Multi-frame / iframe identity — **per-OOPIF dispatch (mechanic 5).**
  **Bigger than it reads (D23):** `actions.rs` is `chromiumoxide::Page`-only
  (`act(page: &Page, …)`, `:112`) with no channel-based action path. So first
  **channelize actions** — generalize `act`/`click`/`type`/`select` from `&Page` to
  `&impl CdpChannel`, driving `Runtime.resolveNode` + the click/type/select dispatch
  through `run`/`run_on` — then route an OOPIF eid to its owning child session.
  Live-verify: a channelized, trusted click lands on an OOPIF element dispatched on
  its owning session, exit 0. Confirms the dispatch half of D23 and closes D22.
  **Shipped run 17:** `actions.rs` is now `<C: CdpChannel>` + `session: Option<&str>`,
  all dispatch through `run_on`; `CdpObserver` carries a `frame_sessions` routing table
  rebuilt each pass and exposes routed `act`/`act_mark`; the agent passes only the flat
  eid and the engine resolves its owning session. **Live-verified** (`examples/act_oopif`,
  exit 0): routed trusted click on `f0/btn-buy-now` flips `"Buy now"` → `"Purchased"`
  inside the out-of-process iframe, on the frame's owning child session.
- [ ] 3.3 Benchmark harness — own arc, own branch (designed in D16, **refined by
  research run 9 / D17**). **Substrate: WebArena-Verified** (`ghcr.io/servicenow/
  webarena-verified`) — not WebArena-via-BrowserGym. WebArena-Verified is
  explicitly agent-language-agnostic ("any programming language ... no dependency
  on the benchmark's libraries"), so the harness is **pure Rust**: anchortree
  drives the Verified Docker sites over CDP, reads the JSON task (`intent`,
  `start_urls`, `task_id`), and emits a JSON response + HAR trace; the Verified
  Docker image scores via `AgentResponseEvaluator` (type-aware normalization, no
  LLM judge) + `NetworkEventEvaluator` (HAR-trace, no DOM selectors). The
  deterministic, no-LLM-judge evaluator is a feature: the only LLM calls left in
  the loop are the agent's own re-grounding calls — exactly the headline metric.
  Reject WebVoyager/WebBench (live web, non-deterministic) and Mind2Web (static
  snapshots, no live rebind). **Headline metric:** LLM re-grounding calls
  eliminated per re-render (0 vs 1), supported by "% of per-turn token budget
  cut" — the cost no prior art isolates. **Dual real-peer baseline:**
  Playwright-MCP on the token-volume axis (full-tree re-snapshot + ref
  invalidation) and Stagehand v3 on the LLM-call axis (re-ground via LLM on
  structural change). One baseline per axis so neither saving is mis-attributed.
  Hold model choice / task-success / network constant via the deterministic
  substrate. Bigger than one run; scoped into separable deliverables by research
  run 16 (**D25**), build order = dependency order:
  - [x] **3.3a HAR recorder** (FIRST, critical path) — record a `network.har`
    from CDP `Network.*` events (`Network.enable` + `EventRequestWillBeSent` /
    `EventResponseReceived` / `EventLoadingFinished` / `EventLoadingFailed`, all
    present in `chromiumoxide_cdp 0.9.1`, no fork). Hermetic, unit-testable
    against synthetic events, **no WebArena dependency** — so it cannot be blocked
    by harness setup. The evaluator consumes this HAR, so it is on the critical path.
  - [x] **3.3b task-runner skeleton + `agent_response.json` emitter** (shape pinned
    by **D26**; sub-steps **i (live pump) + ii (agent_response.json writer)
    SHIPPED run 19, live-verified**; sub-step **iii (offline-replay eval-assertion)
    SHIPPED run 20, live-verified — first real `result.score` = 1.0**) — wire the
    `HarRecorder` to a live CDP event stream via
    `chromiumoxide::Page::event_listener::<T>()` → `EventStream<T>: Stream` (one
    stream per Network event type, merged; the thin `RawCdpSession` channel discards
    events, so use the local `Page` path, not the channel). **DONE (i+ii):**
    `runner.rs` `NetworkCapture::start`/`finish` pump + `AgentResponse` +
    `write_task_output` emit `{output_dir}/agent_response.json` = `{task_type,
    status, retrieved_data, error_details}` + `{output_dir}/network.har` (exact
    filename); proven live by `examples/webarena_capture` (3 real HAR entries, the
    agent JSON written). **DONE (iii) (run 20):** the eval surface is `eval.rs` —
    `EvalResult`/`EvaluatorResult` (`from_eval_result_json` parsed against the real
    captured `eval_result.json`), `task_output_dir(root, id)` (the `{root}/{task_id}`
    layout), `eval_tasks_args`/`eval_tasks_command` (pure argv builder), and
    `run_eval_tasks(root, ids, cfg)` (the one subprocess edge, degrading to
    `EvalError::BinaryNotFound` when the Python CLI is absent so CI stays green). The
    `TaskStatus` enum was completed to all six D27 values
    (`ActionNotAllowedError`/`DataValidationError`/`UnknownError` added; the existing
    `rename_all = "SCREAMING_SNAKE_CASE"` handles the wire spelling). The gated
    `examples/eval_task` writes `agent_response.json` + a one-entry `network.har` into
    `{root}/{task_id}` and drives the real `webarena-verified eval-tasks` offline —
    **live-verified score 1.0 on pinned RETRIEVE task 21**. Empirical finding (vs the
    D27 carry-in): an `AgentResponseEvaluator` RETRIEVE task scores with just
    `agent_response.json` + a ≥1-entry `network.har`; **no `config.json` is required**
    (the evaluator ignores HAR contents but the loader must parse the file, and an
    empty-entries HAR errors the task to 0.0). Hosted/OOPIF HAR is out of scope here.
  - [x] **3.3c re-grounding-calls instrumentation** (headline; spec pinned by **D28**)
    — DONE (builder run 21). `anchortree-core::metric::RegroundLedger` is a pure
    per-task accumulator whose only mutator, `record(&Diff)`, adds `diff.rebound.len()`
    to `rebinds_zero_llm` and counts the observe pass; `llm_reground_calls()` is 0 **by
    construction** (the type has no API to record a model call). **Honesty guardrails
    enforced by tests, not just prose:** `added`/`changed`/`removed` never inflate the
    headline (only Path 2 `rebound` counts), proven against a 50-diff churn and against
    **real `IdentityMap` output** (`tests/metric.rs` — first paint 0, hard re-render 3,
    benign attr update 0). Score pairing = `anchortree-cdp::eval::task_headline(eval,
    ledger)` →
    `task 21: score 1.00 (success) — 3 durable rebinds at 0 LLM re-grounds (over 2 observes)`.
    The 3.3d peer baseline is **Stagehand self-heal LLM calls** (cached absolute-XPath
    breaks on re-render → `page.act`). 145 tests green.
  - [x] **3.3d dual real-peer baseline** (spec pinned by **D29**; stays HERMETIC —
    no live Stagehand/Node/OpenAI/Playwright-MCP server). **SHIPPED builder run 22**
    (`anchortree-core::peer`): `playwright_snapshot`/`snapshot_tokens` (Playwright-MCP
    token model, same `ceil(chars/3.5)` ruler) + `DomPositions`/`StagehandCache`
    (absolute-XPath self-heal model, NOT a rebind proxy) + `BaselineReport` (two-axis
    headline). `tests/peer.rs` drives the REAL `IdentityMap` through a 4-turn login
    task proving both D29 directions: turn 2 in-place re-render = 3 engine rebinds /
    0 peer self-heals; turn 3 sibling-insert = 0 rebinds / 3 self-heals. Grand totals
    6 rebinds vs 3 self-heals — cannot coincide if one proxied the other. 157 tests
    green. Replay the same captured
    observe/mutation sequence through two offline peer *models*, scored with the
    engine's own tokenizer. **Token axis (Playwright-MCP model):** per observe,
    tokenize the *full* AX snapshot with `budget::estimated_tokens` vs anchortree's
    per-turn `budget::diff_tokens(&diff)` — same `ceil(chars/3.5)` ruler, fully
    offline (peer dumps 15K–35K, our `DIFF_BUDGET` 800). **LLM-re-ground axis
    (Stagehand model):** an **absolute-XPath resolver**, NOT a reuse of
    `rebinds_zero_llm`. Critical nuance: Path 2 rebind (`backendNodeId` change) ≠
    XPath break — an absolute XPath can survive a backendNodeId change (in-place
    replace) and break without one (sibling inserted above → Path 1). So record each
    acted element's absolute XPath at bind time and count, per re-render, whether it
    still resolves; each miss = one Stagehand self-heal `page.act` LLM call.
    Counting rebinds as self-heals would over-claim. First cut: task 21 only.
  - [x] **3.3d** shipped (`f5e7f20`, builder run 22): `BaselineReport` +
    `StagehandCache` + `peer.rs` prove rebind ≠ self-heal in BOTH directions
    (6 engine rebinds vs 3 peer self-heals over a 4-turn login task; per-turn AND
    grand totals diverge). Token axis: peer snapshot total strictly exceeds diff total.
  - [x] **3.3e report** shipped (builder run 23): `report.rs` in `anchortree-cdp`
    — `Report` + `TaskRecord` aggregate the whole **WebArena Verified Hard** set
    (210 single-site + 48 multi-site, 68.2% runtime cut; ServiceNow) with the two
    denominators kept structurally apart (D30, CONFIRMED). The **score** axis
    (`scored_tasks`/`mean_score`/`pass_rate`) divides by N = the RETRIEVE-scorable
    count; the **baseline** axis (`anchortree_diff_tokens`/`peer_snapshot_tokens`/
    `engine_rebinds`/`peer_self_heals`) sums over M = the replayed count. No method
    crosses the two; `render()` states "N scored, M baselined". Proven against the
    real task-21 eval + engine-driven baseline-only tasks (4 rebinds vs 2 self-heals
    over M=3; mean score 1.00 over N=1). Wiring to the full 258-task replay corpus
    is a data-capture task, not an engine one.
- [x] 3.4 (guard, per D9 + D31) **SHIPPED (builder run 24).** Keep `RawAxNode` transport-neutral so an
  `anchortree-bidi` adapter is a drop-in. No CDP types past `observer.rs`.
  WebDriver BiDi is the rising cross-browser standard; the engine must not be
  CDP-locked. **But per D31 (research run 22), transport-neutral ≠ drop-in for
  BiDi today.** The seam must abstract THREE sources, not one: (1) the node-identity
  key (CDP `backendNodeId` → BiDi `sharedId`, an opaque session+context-scoped
  reference — fine as a Path-1 soft-match key since the engine rebuilds durability
  via fingerprint, not the transport id; and note BiDi's OWN id is snapshot-scoped —
  the spec defines "no such node — Tried to deserialize an unknown SharedReference"
  (w3c.github.io/webdriver-bidi/), and the staleness bites in the field: webdriverio#13556
  shows a stale BiDi reference silently falling back to WebDriver Classic and returning the
  WRONG element. The standards-track stable handle going stale is direct corroboration that
  durable identity must be an additive engine ABOVE the transport id, not a protocol feature);
  (2) the AX-node property source; (3) the per-node box model. The load-bearing gap: **BiDi has
  no full-AX-tree dump.** Re-verified research 33 (2026-06-18): the W3C "Accessibility module in
  WebDriver BiDi" (issue #443) is still OPEN, last updated 2025-12-12 (no movement; interop-2025
  AX work still referenced) — only an accessibility *locator* (`browsingContext.locateNodes` by role/name)
  exists; full internal-AX-property exposure is at Interop-2025 investigation/prototype
  stage (geckodriver + safaridriver prototypes, RFC in progress). So the future
  `anchortree-bidi` adapter must CONSTRUCT the tree (script-injected accessibility
  walk + DOM), not read a `getFullAXTree` equivalent. 3.4 ships the seam; the adapter
  itself waits until BiDi AX exposure lands or the constructed-tree path is specced.
  **Shipped (run 24):** `tests/transport_neutrality.rs` — a 3-test fitness function that
  scans both crates' real source and fails the build if a CDP type crosses the seam:
  (1) `anchortree-core` names no CDP type; (2) the cdp crate's code-level chromiumoxide
  surface is exactly the pinned transport adapters (actions/channel/error/har/observer/
  runner); (3) the fusion path (`fuse.rs`/`eval.rs`/`report.rs`) is CDP-free. Plus a
  `TransportNodeKey` alias naming the opaque per-pass node key (CDP `backendNodeId` today,
  BiDi `sharedId` tomorrow) at the `RawAxNode` seam, and the deferred-adapter rationale in
  the `fuse.rs` module docs. Guard verified to bite via an injected-leak negative check.
  - [ ] **3.5 (data) capture the replayable observe corpus offline** — the `Report`
    aggregator (3.3e) is proven on task-21 + synthetic; feeding it real WebArena Verified
    tasks needs each task's observe sequence from a `network.har`. Per D32 (research run
    23) the cheapest first cut needs NO Docker and NO agent run: the ServiceNow repo
    SHIPS real fixtures — `examples/agent_logs/demo/107/` and `108/` each carry the full
    triple (`agent_response.json` + `eval_result.json` + `network.har`), so both are
    scorable (N) offline. The Hard task list is vendored at
    `assets/dataset/subsets/webarena-verified-hard.json` (2,431 bytes, the 258 ids).
    - [x] **3.5a (SHIPPED run 25):** vendored the 2 in-repo fixtures (`corpus/107`,
      `corpus/108`) + the Hard task list (`corpus/subsets/`), wired `corpus.rs`: a loader
      that walks `corpus/<task_id>/{eval_result.json,agent_response.json,network.har}` and
      `report_from_corpus` folds the scorable tasks into `Report`. Ships a REAL **N=2**
      score aggregate over genuine WebArena-Verified artifacts (108 RETRIEVE pass 1.0, 107
      NAVIGATE fail 0.0, mean 0.50) — the first non-task-21 numbers. ServiceNow repo is
      Apache-2.0, vendored with attribution (`corpus/README.md`).
      **Correction to D32 (the M claim):** a `network.har` is a *network trace*, not an
      accessibility capture, and the crate has no offline HTML→AX path, so it cannot
      produce the baseline axis (M) offline. M is deferred to 3.5b; a present HAR only
      marks a task `is_replayable` (the precondition a 3.5b capture can run). The big HARs
      are git-ignored and fetched on demand by `corpus/fetch-hars.sh`.
    - **3.5b (M-capture, two-tier per D33):** the 3.5a correction proved M needs the engine
      to *observe a real page*, which a `network.har` cannot do alone. D33 pins the fix as a
      hermetic HAR→chromium fulfill layer (Tier 1, CI-runnable) plus a live WebArena Docker
      standup (Tier 2, growth) for tasks where HAR replay hits the dynamic-app gap.
      - [x] **3.5b Tier 1 matcher (SHIPPED run 26):** `replay.rs` — the browser-free heart of
        the fulfill layer. Parses a third-party `network.har` (its own `Deserialize` read
        model, distinct from the `Serialize`-only record-side `har.rs`) and, per Playwright's
        `routeFromHAR` rule, selects the recorded entry that answers a live request: strict
        URL + method, strict POST payload when present, ties broken by most-matching request
        headers, and **no match = abort** (the D30 honesty guard — an off-trajectory request
        fails loudly rather than rendering a wrong page and polluting M). Surfaces the matched
        response's status/headers/mime and body location (inline / base64 / external `_file` /
        empty) via `ReplayBody` for the fulfiller. CDP-free, behind the transport seam, 10
        hermetic unit tests. Real corpus HARs are 359-entry browser-use trajectories with
        external `_file` bodies.
      - [x] **3.5b recorder body capture (SHIPPED run 27, D34):** the demo HARs (107/108) are
        *unfulfillable* — 359 GET entries, zero inline `content.text`, 354 external `_file`
        refs to a sidecar dir the repo never vendors, 5 empty including the primary document
        body (D34). So the hermetic replay target is **anchortree's own recorder output**, not
        the demo HARs. `har.rs` now captures response bodies: `HarContent` carries optional
        `text`/`encoding` (base64 for binary), a `ResponseBody { text, base64 }` input feeds
        `HarRecorder::on_response_body(request_id, body)` between the response and
        loading-finished events, and `finalize` writes it into `content`. `skip_serializing_if`
        keeps a body-less recording byte-identical to the pre-capture output. The body-capture
        state transition is the CI-runnable heart (5 new unit tests); the live
        `Network.getResponseBody` call is transport-touching and lands with the feeder.
      - [x] **3.5b fulfill-leg param builder (SHIPPED run 28, D35):** `fulfill.rs` — the pure,
        CI-tested half of the fulfill leg. `replay_action(request_id, &MatchOutcome) -> ReplayAction`
        maps a matcher verdict to the exact CDP params to dispatch: `Abort` →
        `Fail(FailRequestParams, ErrorReason::Failed)` (honest abort, never a guessed response);
        `Fulfill(entry)` → `FulfillRequestParams` with recorded status, 1:1 `HeaderEntry` headers,
        and body. D35 recommended store-everything-base64 at capture (OPTION 1); run 28 chose
        OPTION 2 (encode raw text on the fulfill side) to keep captured HARs human-readable —
        `base64==true` passes through, `base64==false` is base64-encoded via the now-direct
        `base64 = "0.22"` dep, encode runs once per intercepted request (not a hot path). `External`
        body → `Fail` (matcher never opens sidecars; self-captured HARs never produce `External`).
        In `CDP_ADAPTER_FILES` (names CDP types). 7 new unit tests.
      - [x] **3.5b live fulfill event loop (SHIPPED run 29, D36):** `fulfill.rs` gains the
        transport-touching half. `request_from_paused(&EventRequestPaused) -> ReplayRequest` decodes a
        live paused event into the matcher's plain value (headers flatten from `network::Headers`,
        `post_data` None for the GET proof target), and `ReplayFulfiller` (`start`/`finish` +
        `FulfillStats`) subscribes `Fetch.requestPaused`, enables interception at the Request stage for
        `*`, and pumps each paused event through `request_from_paused` → `har.outcome` → `replay_action`
        → `page.execute(...)`, then disables on finish. **D36's pump citation was wrong:** it said build
        on a raw-WS `TcpStream` loop (`webarena_capture.rs` ~149-182), but those lines are the one-shot
        HTTP `/json/version` lookup, not a WS event pump. The real non-discarding tap is chromiumoxide's
        `Page::event_listener::<T>()` EventStream, which `NetworkCapture` (runner.rs) already uses;
        `ReplayFulfiller` mirrors its subscribe-before-enable / spawn-pump / stop-and-drain shape. D36's
        SEQUENCING constraint is honored exactly (enable→navigate→fulfill-all→disable→THEN observe).
        6 new CI decode/stat tests (synthetic deserialized events, no browser); live proof rides
        `examples/webarena_replay.rs` (compiles + clippy-clean in CI).
      - [x] **3.5b run-once live M=1 — SHIPPED (build run 30, D37 resolved).** Stood up the
        in-container headless-shell + a static fixture, captured a SELF-CONTAINED inline-body HAR,
        replayed it with NO live origin → **1 fulfilled / 0 failed / 0 dispatch errors, 3 durable
        eids** = the first real **M=1** offline. The roadmap framed this as "no new code", but the
        capture-side body feeder had never been wired (captured HARs were body-less → unreplayable):
        `NetworkCapture::start_with_bodies` + a `record_event` feeder issuing `Network.getResponseBody`
        at each `loadingFinished`. Landed as `scripts/run-once-m1.sh` +
        `scripts/fixtures/m1-site/index.html`; `webarena_capture.rs` honors `ANCHORTREE_CAPTURE_OUT`.
      - [x] **3.5b rebind-on-replay M datapoint — SHIPPED (build run 31, D38 resolved).** Deepened the
        M=1 from mint-only (Path 3) to a durable REBIND through a re-render (Path 2, `diff.rebound`, 0
        LLM) on the SAME replay rail. Added an inline `<script>` (`window.__atRerender`) to
        `scripts/fixtures/m1-site/index.html` that rebuilds the card's children as fresh DOM nodes with
        byte-identical roles + text (fresh `backendNodeId`, same fingerprint); replays deterministically
        because the body is inlined in the HAR. `webarena_replay.rs` now does observe → re-render →
        observe, feeds both diffs to a `RegroundLedger`, and asserts `!diff.rebound.is_empty()` +
        `llm_reground_calls() == 0`. **Live: observe 1 = 3 minted; observe 2 = 2 rebound / 0 added / 0
        changed / 0 removed → "2 durable rebinds at 0 LLM re-grounds".** README vs-the-field section now
        carries the Stagehand-cache contrast (DOM-hash drift → LLM fallback;
        browserbase.com/blog/stagehand-caching) — the exact re-render where anchortree rebinds with zero
        model calls.
      - [x] **3.5b measured head-to-head — SHIPPED (build run 32, D39 resolved).** Converted the
        central competitive claim from a doc sentence to a number on our own rail. `peer.rs` gained
        `DomPositions::from_document_order` (the absolute `/*[k]` view a raw-XPath resolver caches,
        keyed by accessible name over document order; 2 new unit tests). The m1-site fixture gained
        `window.__atReorder`, which moves the button PAST the observed `role="status"` node to the
        end of the card — the plain intro `<p>` is not surfaced, so the button must cross an OBSERVED
        sibling for its index to shift. `webarena_replay.rs` now runs three legs (observe → in-place
        re-render → reorder), binds a `StagehandCache` from `from_document_order`, and re-resolves it
        after each. **Live: anchortree 4 rebinds at 0 LLM re-grounds across both legs; Stagehand 0
        self-heals on the in-place leg (honest — positions unchanged, a rebind is not a self-heal),
        1 self-heal on the reorder (the LLM-call axis measured on one real transition).** Variant
        mismatch reconciled per D39 option (a): measure only the faithfully-modelable absolute-XPath
        resolver; the coarser DOM-hash whole-page cache stays as clearly-scoped README prose (its
        internal hash cannot be modelled without overclaiming — a byte-identical in-place re-render
        would not even drift an outerHTML hash). README vs-the-field names both caches and carries
        the live two-leg numbers.
      - [x] **3.2e cross-frame FRAME-TIER durability (run 33 → D40 RESOLVED).** The node
        tier of `(frame, in-frame fingerprint)` is proven + measured (run 32). The FRAME tier is not:
        `FrameKey = parent.child(structural-ordinal)` (frames.rs:11) is durable against CDP `frameId`
        reassignment but NOT against a frame-owner reorder/insert — a sibling iframe added before the target
        shifts every later FrameKey, so the in-frame fingerprint is looked up under a different key and the
        eid re-mints. Tests cover sibling + nested iframes (frames.rs:382,526) but NONE reorders a real
        frame-owner and asserts eid survival; the gap is unmeasured. This is the same ordinal fragility the
        field just hit: Stagehand v3 (CDP-native) cross-frame composite ID is `frame ordinal + backendNodeId`
        (browserbase.com/blog/taming-iframes-a-stagehand-update), neither tier durable across re-render.
        Build (no Docker, HAR rail): (a) fixture with a same-origin `<iframe>` whose inner card re-renders +
        a hook that reorders a sibling frame-owner before it; (b) measure two legs — inner-frame churn
        (expected rebind at 0 LLM) and frame-owner reorder (likely re-mints today; report the measured gap
        like run 32's reorder leg); (c) fix `FrameKey` to carry a durable frame-owner discriminator
        (accessible name / src-origin / structural-path) beyond the bare ordinal, then re-run leg B for a
        rebind at 0 LLM — a head-to-head where Stagehand pays on BOTH tiers and anchortree pays on neither.
        Builder confirms the discriminator shape and how it composes with the phantom-owner skip (frames.rs:188).
        DONE (run 33): `FrameKey::child_segment` + a frame-owner discriminator (src origin+path → name → title → id,
        sanitized, `#n`-deduped per document) make a labelled frame's key survive a sibling-owner reorder; the live
        node→frame map switched from `frame_keys(getFrameTree)` to `dom_frame_keys(dom)` so the discriminator reaches
        eids. 11 new unit tests (8 frames + 3 observer): the gap, the fix, dedup, ordinal-mix, nesting, OOPIF, and the
        attribute selector. The CI-gated unit proof is step (c); the live HAR two-leg measurement (a/b) is the follow-up.
      - [x] **3.2f cross-frame FRAME-TIER measured head-to-head, CI-gated (D41 resolved, run 34).** The frame-tier twin
        of the node-tier head-to-head, made a CI-gated NUMBER (one tier more rigorous than the node tier, whose head-to-head
        only runs in the browser script). `peer.rs` gains `FrameOrder` (a positional ordinal→discriminator view of the owner
        order, identical discriminators collapsing to first ordinal) + `FrameOrdinalCache` (a Stagehand `frameOrdinal`
        resolver: `bind` free, `reresolve` charges one re-ground per cached handle whose ordinal no longer holds its
        discriminator). 6 peer tests measure the reorder as `(1 positional reground, 0 discriminator reground)` and the Leg-A
        churn as 0; the D41 bound is encoded as `identical_discriminator_siblings_collapse_to_first_ordinal`. The duplicate-`src`
        degradation unit test (`ads`→`ads#1`→`ads#2` on a front-insert) lives in `frames.rs`. README vs-the-field carries the
        frame-tier `1`-vs-`0` paragraph + the distinct-vs-identical bound (parity with Playwright `.nth()`). No content-fingerprint
        disambiguator built (blocked by per-frame-AX availability; already at field parity).
      - [x] **3.2f-live cross-frame FRAME-TIER live HAR measurement (run 35 — the browser-tied twin of 3.2f).** Run-32-style
        HAR rail: `crates/anchortree-cdp/examples/webarena_frame_replay.rs` + `scripts/fixtures/frame-site/index.html`, a single
        self-contained page whose interactive element lives one frame down inside a same-origin `name="checkout"` srcdoc iframe,
        plus a `__atFrameRerender` hook (rebuilds the checkout frame's card in place) and a `__atFrameReorder` hook (inserts a
        sibling `name="ads"` srcdoc iframe BEFORE the checkout owner — the D41 distinctly-identified-target constraint). Driven by
        `scripts/run-once-frame.sh`: capture a self-contained HAR via the recorder, replay with no live origin, measure two legs.
        **Design choice (D42):** srcdoc owners (no `src` attr) key cleanly on `name` (D40 priority) and are pierced inline with no
        request of their own, so a single-document HAR carries the whole cross-frame page — the node-tier offline rail lifted one
        tier up. **Semantic correction caught only by the live smoke-run (D42):** a frame-owner reorder does NOT touch the checkout
        frame's own document, so the button keeps its `backendNodeId` and stays bound with ZERO churn (not removed, not re-minted) —
        a STRONGER proof than a rebind, since ordinal keying would instead have dropped `f0/...` and minted `f1/...`. Leg A
        (inner-frame churn) is the rebind; Leg B (frame reorder) is stability + the peer re-ground. Live result: 2 rebinds at 0 LLM
        re-grounds, `FrameOrdinalCache` pays 1 re-ground on the reorder leg. Closes the prove(33)→measure-in-CI(34)→measure-live(35)
        split for the FRAME tier.
      - [x] **3.5b Tier 2 (growth):** live WebArena-Verified Docker standup for HAR-resistant
        dynamic tasks; widen toward all 258 Hard ids. **DONE (build run 36 → D43 RESOLVED):** booted the smallest
        per-site image (`am1n3e/webarena-verified-map`, 1.19 GB) as a sibling, `docker network connect`ed it to
        `phantom_phantom-net` for container-DNS reachability, captured a real OSM `/about` page's self-contained
        HAR live (`webarena_capture`), tore the site down, and replayed offline through the new general
        `webarena_observe` rail — **31 AX nodes → 30 durable eids minted over a genuine server-rendered page with
        no live origin.** New `examples/webarena_observe.rs` (raw `Page.navigate`, no fixture hooks) +
        `scripts/run-once-webarena.sh` (boot-one-site harness, pre-builds with the browser DOWN to stay under the
        phantom pids budget). The live run caught two real `ReplayFulfiller` fidelity bugs only real pages exercise:
        wire-framing-header strip (decoded body kept origin `Content-Encoding: gzip`/`Content-Length` → Chrome
        gunzips plain text → empty DOM) and status-0 fail (an opaque entry is `-32602`-rejected by
        `Fetch.fulfillRequest` and stalls a head `<script src>` parser); +3 unit tests pin both. **Gate REVISED (research run 34 → D43 PROPOSED): the old
        `pids.max=256` gate is a FALSE PREMISE — that ceiling is on the phantom container, NOT on siblings; a
        `docker run` from inside phantom launches on the host daemon and gets its own pids cgroup (verified:
        no-limit sibling reports `pids.max=37558`, host default; 16 cores; 164 GB free on the docker overlay).
        The real gate is per-site disk + a boot-ONE-site M=1 smoke, because `ghcr.io/servicenow/webarena-verified`
        (~0.2 GB) is a thin CLI EVALUATOR that hosts no sites — the environments are separate per-site containers
        (`am1n3e/webarena-verified-shopping/-gitlab/-reddit/…`, "up to 92% smaller than originals" but still
        likely 1-3 GB each). The evaluator scores from `agent_response` + `network_trace` (HAR) files, i.e.
        anchortree's offline-rail output, so a site is booted ONCE to capture, then replayed offline. Execute:
        (1) `docker manifest inspect` the smallest per-site image, confirm it fits 164 GB; (2) boot it as a
        sibling, point `chrome-headless-shell` at it, capture one task's self-contained HAR via `webarena_capture`;
        (3) replay offline + feed `agent_response`+`network_trace` to the evaluator container, confirm deterministic
        scoring — the pure-Rust D17 loop end-to-end at M=1. Only then widen M/N.** Lower priority than the
        now-shipped 3.2f-live: that cross-frame proof was cheaper and no-Docker and landed where the field struggles.
    Until 3.5b's live legs land, the published headline is "proven on the N/M actually in the
    corpus", never "X% on 258".
      - [x] **3.5b Tier 2 — external evaluator score at M=1 — SHIPPED (build run 37, D44 RESOLVED).** The external
        `ghcr.io/servicenow/webarena-verified:latest` evaluator scored a live-captured navigation **1.0** on map
        task **356** (NAVIGATE → `__MAP__`): `AgentResponseEvaluator` 1.0 + `NetworkEventEvaluator` 1.0, checksums
        banked (`evaluator 35c3385b…`, `data d6527566…`, version 1.2.3). The internal eid count is now backed by an
        EXTERNAL deterministic score. Two findings made it land: (a) the recorder needed a
        `requestWillBeSentExtraInfo` header-merge so the document carries the on-wire `Accept`/`sec-fetch-*` the
        evaluator's `is_navigation_event` classifies on (sparse provisional `requestWillBeSent` headers alone fail
        the nav check); (b) the public slim map image ships NO OSM way/node data, so a `/way/`-class task (369)
        can't honestly serve 200 — task 356's home-page target genuinely serves 200, the honest M=1. `/way/` NAVIGATE
        + RETRIEVE deferred to a data-loaded-image widen phase. Harness: `scripts/run-once-eval.sh` (self-contained
        boot→capture→score→assert; host-path-translated mounts for the sibling evaluator container). Research run 35
        pinned the
        evaluator I/O contract (D44): `webarena-verified eval-tasks --task-ids <id> --output-dir <dir>` (thin
        ~0.2 GB image: `docker run --rm -v $PWD/output:/data ghcr.io/servicenow/webarena-verified:latest eval-tasks
        --task-ids <id> --output-dir /data`); `agent_response.json` = 4 fields `{task_type
        (NAVIGATE|RETRIEVE|MUTATE), status (SUCCESS|…), retrieved_data (null|[typed]), error_details}`; offline
        network-trace replay is first-class (no live env at scoring time); determinism is checksummed
        (`evaluator_checksum` + `data_checksum`). **Execute:** (1) export the map-site task ids (`subset-export` or
        filter `webarena-verified.json` by `sites==["map"]`), pick the simplest **NAVIGATE** task (expected
        `{navigate, success, null}` — the clean first 1.0; RETRIEVE needs typed-data extraction, defer); (2) reuse
        `run-once-webarena.sh` to capture THAT task's `network_<id>.har`, emit `output/<id>/agent_response.json`
        (NAVIGATE/SUCCESS/null/null) from the observe outcome; (3) `eval-tasks` offline, assert
        `eval_result.score == 1.0`, bank both checksums. This closes the D16/D17 loop with an EXTERNAL score, not
        an internal count. Only after the single 1.0 lands do we widen M/N + add RETRIEVE. **This is now the top
        Tier-2 lane and is also a strong Phase-4 blog headline** (the benchmark's own evaluator removed
        LLM-as-a-judge — README Features — so anchortree's 0-LLM re-ground is scored by a 0-LLM evaluator; the
        convergence is the story).
      - [x] **3.5b Tier 2 widen item (1) — first RETRIEVE on a self-contained site (build run 38; D45 item (1)
        RESOLVED).** Build run 37's map 404s are by design, not a bug: upstream README "Start and Stop Sites" —
        shopping/shopping_admin/reddit/gitlab start via direct `docker run` with data baked in, while **wikipedia and
        map** require a separate multi-GB `webarena-verified env setup init --site <s> --data-dir ./downloads`
        download. The slim map image has no OSM way/node data, hence the /way/ 404s; we do NOT boot a data-loaded map
        image. **DONE:** anchortree drove the authenticated Magento admin (`am1n3e/webarena-verified-shopping_admin`,
        `admin`/`admin1234`), navigated to the filtered review grid
        (`/admin/review/product/index/filter/ZGV0YWlsPWRpc2FwcG9pbnRlZA==/`, base64(`detail=disappointed`)), read the
        `#reviewGrid-total-count` Magento server-renders (`6 records found`, no async JS), emitted
        `{RETRIEVE, SUCCESS, 6, null}`, and the GENUINE ServiceNow evaluator scored `eval_result.score == 1.0` on
        shopping_admin **task 11** (only `AgentResponseEvaluator`; scalar `6` normalises to `(6,)` == expected `[6]`).
        Honest read, not a DB query: a different store count would score 0. Pinned Magento `base_url` to `http://at-sa/`
        + `cache:flush` so the container-DNS admin serves 200 not a 302. Files: `examples/webarena_retrieve.rs`
        (site-agnostic login-then-read via `ANCHORTREE_LOGIN_*`/`READ_JS`/`RETRIEVE_NUMBER`, +5 parse tests),
        `scripts/run-once-retrieve.sh`. Proves the typed-data path D44 deferred.
      - [x] **3.5b Tier 2 widen item (2) — data-backed NAVIGATE to a real CONTENT page (build run 39; D46 item (2)
        RESOLVED via shopping_admin task 157; gitlab deferred on disk).** The remaining D45 score — a NAVIGATE PAST a
        home page on a self-contained data-loaded site, refuting the map 404 as image-specific — is banked. Research
        run 37 picked **gitlab task 45**, but the gitlab-ce image extracts to ~12 GB+ and the pull died with "no space
        left on device"; reclaiming it means deleting other live projects' images, so the build PIVOTED to the
        already-cached `shopping_admin` image (forward motion over a destructive sweep — see BUILD_LOG run 39, DECISIONS
        D46). anchortree logged into the admin (`admin`/`admin1234`), navigated to the customer grid
        (`/admin/customer/index/`), captured the NAVIGATE HAR, emitted `{NAVIGATE, SUCCESS, null, null}`, tore the site
        down, and scored offline → **`eval_result.score == 1.0` on task 157** (intent_template_id 255, revision 2),
        BOTH the `AgentResponseEvaluator` (NAVIGATE/SUCCESS) AND the `NetworkEventEvaluator` (url
        `__SHOPPING_ADMIN__/customer/index`, response_status 200, GET). **URL-normalization discovery:** the
        `__SHOPPING_ADMIN__` placeholder maps to the admin base (`http://<host>/admin`), so the eval config points at
        `ADMIN_BASE`; the dataset's theme tasks (374/375) carry a stray second `/admin` segment AND 404 on this image's
        Magento build, so task 157 (the customer grid, 200-serving) is the clean content page. Files:
        `examples/webarena_capture.rs` (optional login via `ANCHORTREE_LOGIN_URL`/`ANCHORTREE_LOGIN_JS`),
        `scripts/run-once-admin-nav.sh` (boot/pin/login/navigate/capture/score, robust pin-and-verify base_url loop).
        gitlab task 45 stays the canonical pick for when disk headroom exists. Closes the D45 NAVIGATE-to-content goal.
      - [x] **3.5b Tier 2 widen — widen M/N across the Hard ids — SHIPPED (build run 40, D47 RESOLVED).** All three
        batch tasks scored 1.0 against the genuine evaluator: **(1) RETRIEVE 15** (`detail=best`, `retrieved_data=[2]`),
        **(2) NAVIGATE 707** (base64 URL-safe path segment + `query_params` normalized to dates — BOTH AgentResponse
        AND NetworkEvent passed, GET 200), **(3) NAVIGATE 375** (HAR inspection proved it honestly serves 200 GET,
        CORRECTING run 39's stale 404 recon, so INCLUDED not dropped). Folded the five-task Hard batch (RETRIEVE 11/15 +
        NAVIGATE 157/707/375) into `report.rs`'s two-denominator ledger via new `passing_navigate_eval` helper +
        `hard_banked_batch_folds_retrieve_and_navigate_into_n` regression test; SCORE-axis doc widened RETRIEVE-only →
        RETRIEVE+NAVIGATE; `run-once-retrieve.sh` gained FILTER_B64/GRID_URL overrides + robust warm-up. 158 cdp tests
        green, clippy/fmt clean, CI success. Denominator increment (D26) shipped: only MUTATE stays config/live-state-
        gated. Next: MUTATE de-gate (the last denominator) or widen the NAVIGATE count further. Original plan below.
      - [x] **3.5b Tier 2 widen — original plan (D47 PROPOSED, research run 38).** NAVIGATE
        (map home + data-backed admin grid) and RETRIEVE (typed count) are all banked at M=1 against the GENUINE
        evaluator. Next growth is breadth. Research run 38 located the OFFICIAL Hard subset file
        `assets/dataset/webarna-verfied-hard.json` (258 = 210 single-site + 48 multi-site; both banked tasks 11 + 157
        are members) and settled the CONCRETE next batch (D47), all on the already-cached `shopping_admin` image,
        reusing `run-once-retrieve.sh` + `run-once-admin-nav.sh` verbatim:
        **(1) RETRIEVE task 15** (intent_template_id 288, SAME template as banked task 11 — swap the review filter to
        base64(`detail=best`), expect `retrieved_data == [2]`; proves cross-`instantiation_dict` generalization);
        **(2) NAVIGATE task 707** (sales order report, url `…/reports/report_sales/sales/filter` WITH
        `query_params {report_type:[created_at_order], from:[1/1/2022], to:[12/31/2022]}` — a NEW evaluator surface,
        query_params matching not just path; fallback sibling 708 tax report);
        **(3) NAVIGATE task 375** OPTIONAL theme settings — DROP if it 404s (build run 39 found theme routes 404 on
        this image). Result: 5–6 Hard tasks scored, folded into `report.rs`'s two-denominator N-scored ledger.
        Denominator increment (D26): NAVIGATE is now PROVEN offline-scorable (map 356 + sa 157 both 1.0 via HAR
        replay, no config.json), so N-scored widens to RETRIEVE+NAVIGATE; only MUTATE stays config/live-state-gated.
        Defer gitlab until disk headroom exists (the ~12 GB pull is the only blocker; the `external_url` pin path is
        designed in D46). Hold mutate tasks (live state change). Cached-image Hard type counts: shopping_admin 55
        (23r/6n/26m), shopping 56 (25r/10n/21m). Never publish "X% on 258" before the per-corpus M lands (D30
        two-denominator).
      - [x] **3.5b Tier 2 — MUTATE de-gated: HAR request-body capture — SHIPPED (build run 41, D48 RESOLVED).**
        The capture-side precondition that makes a mutating POST offline-scorable. Reading the WebArena-Verified
        evaluator source disproved D27 for the shopping_admin MUTATE class: `NetworkEventEvaluator` scores the
        *mutating request* — `url` (placeholder-normalized) + `http_method:POST` + `post_data` (a form-field subset
        via `parse_qs`) + `response_status:302` — from the HAR, NOT from live post-state. The real gap was that the
        recorder dropped the request body. **`har.rs`:** `RequestPostData{text}` input + `on_request_post_data` pure
        feeder + `post_text` on `Pending` + `HarPostData{mimeType,text}` output struct + `post_data` field on
        `HarRequest` (serde `postData`, skip-if-None) + finalize-time MIME from the request `Content-Type` header
        (`header_in_list` helper) + `body_size` = body byte length. 5 new unit tests (the emitted `postData` is
        exactly what `parse_qs(text)` reads; omitted when absent; empty MIME when undeclared; unknown-id no-op;
        survives a redirect hop). **`runner.rs`:** `record_event` issues `Network.getRequestPostData` AFTER the fold
        for any `requestWillBeSent` with `has_post_data` (the pending entry must exist first — mirror image of the
        response-body read). 163 cdp tests green (+5), clippy/fmt clean, CI success. **No live MUTATE scored yet** —
        next run drives shopping_admin task 488 (CMS title save), captures, runs the evaluator (expect 1.0), and
        folds MUTATE into `report.rs` so N spans the full RETRIEVE+NAVIGATE+MUTATE task-type matrix.
      - [x] **3.5b Tier 2 — FIRST LIVE MUTATE scored 1.0 + folded into N: task 488 (SHIPPED, build run 42, D49
        RESOLVED for 488).** Drove shopping_admin task 488 ("Change Home Page CMS title") end to end against the genuine
        evaluator → **score 1.0** (both `AgentResponseEvaluator` MUTATE/SUCCESS and `NetworkEventEvaluator`
        url+POST+302+post_data-subset). Proven twice from a clean DB title (reset + `cache:flush` + re-driven +
        re-scored 1.0, DB title confirmed mutated). The capture-path correction (D49): the save body is NOT served by
        `Network.getRequestPostData` (a navigation POST sheds its network resource on redirect) — it is inlined on
        `requestWillBeSent` as base64 `postDataEntries`, now decoded by `har::inline_post_text` as the PRIMARY body
        source (the run-41 `getRequestPostData` read demoted to an over-long-body fallback). Flakiness closed with a
        quiescence gate in `scripts/run-once-mutate.sh` (readyState + no loading mask + jQuery idle, stable 3 polls,
        then set-verify-click). `report.rs` SCORE axis widened RETRIEVE+NAVIGATE → RETRIEVE+NAVIGATE+MUTATE; banked
        batch N=6, `6 scored (6/6 pass, mean score 1.00)`. cdp lib 168 tests (+5), clippy/fmt clean, CI success.
      - [x] **3.5b Tier 2 — MUTATE M-widen: sibling task 489 SCORED 1.0 + folded → N=7 (SHIPPED, build run 43,
        D49 fully resolved).** Drove task 489 (Privacy Policy, page_id 4, "No privacy policy is needed in this dystopian
        world") through `scripts/run-once-mutate.sh` (`TASK_ID=489 PAGE_ID=4 MUTATE_TITLE=…`) → **score 1.0**, both
        evaluators pass, `actual_normalized` post_data matched `expected` exactly from a real full Magento save form.
        Same `cms/page/save/back/edit` template as 488 across a distinct `instantiation_dict` — the MUTATE analogue of
        RETRIEVE 11/15, a genuine template-generalization datapoint. NO capture-rail/harness change needed (488's
        inline-`postDataEntries` decode + quiescence gate carried 489 unchanged — that IS the generalization). `report.rs`
        banked batch now folds both MUTATEs → N=7, `7 scored (7/7 pass, mean score 1.00)`. cdp lib 168 tests, fmt/clippy
        clean, CI success. (task 490, page_id 5, same template but NOT Hard — left as fallback, not driven.) The N matrix
        (RETRIEVE+NAVIGATE+MUTATE) is now complete; Phase 4.3 (the identity-thesis post; D50) is the next build.

## Phase 4 — polish + reach (weeks 9-16)

**Sequencing note (research run 40, D50 PROPOSED):** with N=6 now spanning the
full RETRIEVE+NAVIGATE+MUTATE matrix, after 489 lands, open **4.3 first**, then
4.1/4.2. The lede is the `vercel-labs/agent-browser` contrast (36,376 stars,
pushed 2026-06-16): the field's biggest tool now ships BOTH a `snapshot` (AX tree
with `@eN` refs) AND a `diff snapshot` verb — validating the snapshot+diff premise
publicly — yet its refs are snapshot-ordinal ("Refs are invalidated when the page
changes … @e1 … ← Different element now!") and its diff is a text dump compare.
Nobody kept the element's identity across the re-render. That is the post's hook,
and it is time-sensitive.

- [ ] 4.1 Crate published to crates.io.
- [ ] 4.2 Project page + docs site on truffleagent.com.
- [ ] 4.3 Blog post + dev.to crosspost on the identity thesis with benchmark
  data. **Lede:** the 2026 field converged on snapshot+diff (agent-browser,
  Playwright-MCP, Stagehand) but every shipping impl re-mints refs on change;
  anchortree is the zero-LLM durable rebind, scored 6/6 by a zero-LLM WebArena
  evaluator (the 0-LLM-rebind-scored-by-0-LLM-eval convergence).

## Exit condition (by week 3)

If the durable-identity rebind does not measurably beat naive re-grounding on
the benchmark suite (Phase 3.3 preview), reassess the thesis before investing
in breadth.
