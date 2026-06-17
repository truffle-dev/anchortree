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
  - [ ] **3.3b task-runner skeleton + `agent_response.json` emitter** (shape pinned
    by **D26**) — wire the `HarRecorder` to a live CDP event stream via
    `chromiumoxide::Page::event_listener::<T>()` → `EventStream<T>: Stream` (one
    stream per Network event type, merged; the thin `RawCdpSession` channel discards
    events, so use the local `Page` path, not the channel). For one RETRIEVE task,
    emit `{output_dir}/agent_response.json` = `{task_type, status, retrieved_data,
    error_details}` + `{output_dir}/network.har` (exact filename). Get the first real
    `result.score` from `webarena-verified eval-tasks --config <cfg> --task-ids <id>
    --output-dir <dir>`. **Keep the eval-assertion hermetic via offline HAR replay**
    (WebArena-Verified PyPI Jan-2026 feature: "evaluate without live web environments
    using network trace replay") — capture against a local `headless-shell` page, no
    full Docker site stack needed for the first score. Hosted/OOPIF HAR is out of
    scope here.
  - [ ] **3.3c re-grounding-calls instrumentation** (headline) — count durable
    `eid` rebinds vs LLM re-ground calls; anchortree = 0 re-grounds per re-render.
  - [ ] **3.3d dual real-peer baseline** — Playwright-MCP token-volume +
    Stagehand LLM-call count on the same tasks, one baseline per axis.
  - [ ] **3.3e report** over the 258-task difficulty-prioritized subset — the
    publishable headline number.
- [ ] 3.4 (guard, per D9) Keep `RawAxNode` transport-neutral so an
  `anchortree-bidi` adapter is a drop-in. No CDP types past `observer.rs`.
  WebDriver BiDi is the rising cross-browser standard; the engine must not be
  CDP-locked.

## Phase 4 — polish + reach (weeks 9-16)

- [ ] 4.1 Crate published to crates.io.
- [ ] 4.2 Project page + docs site on truffleagent.com.
- [ ] 4.3 Blog post + dev.to crosspost on the identity thesis with benchmark
  data.

## Exit condition (by week 3)

If the durable-identity rebind does not measurably beat naive re-grounding on
the benchmark suite (Phase 3.3 preview), reassess the thesis before investing
in breadth.
