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
  **live-verified against a real same-origin `srcdoc` iframe**. Phase 3.2b
  **cross-origin OOPIF channel + join NOW SHIPPED (run 14, D21 mechanic 3 /
  D22 steps 1-3, amended)** — the thin channel speaks N sessions (`run_on`),
  auto-attaches OOPIF children (`auto_attach_children` draining
  `Target.attachedToTarget`), and joins each child session to a durable structural
  `FrameKey` by `child.target_id == owner frameId`. **Live finding that corrected
  D22 step 3:** a cross-origin OOPIF is *absent* from the root `getFrameTree`; its
  owner `<iframe>` (frameId present, `contentDocument` stripped) IS in the pierced
  DOM, so the key table comes from DOM document order (`dom_frame_keys`), not
  `getFrameTree`. **Live-verified against `--site-per-process` Chrome with a
  genuinely cross-origin child** (`examples/attach_oopif`, exit 0). Phase 3.2c
  **per-OOPIF observe NOW SHIPPED (run 15, D23 mechanic 4)** — `observe()` returns
  one flat node list in which a cross-origin OOPIF's widget carries a durable,
  frame-namespaced eid and rebinds across an in-OOPIF `innerHTML` swap. The thin
  channel promotes `run_on`/`auto_attach_children` onto the `CdpChannel` trait
  (no-op defaults; `Page` stays a local fast path); `raw_pass` returns a
  `Vec<FramePass>` (root + one per live OOPIF child session) and `observe` fuses
  **each pass independently and concatenates** — the D23 collision resolution
  (per-target `backendNodeId`/`AXNodeId` spaces never share a fuse pass, the core
  already keys by `(FrameKey, backend)` so no remapping is needed). A persistent
  `oopif_sessions` cache holds child sessions across passes. **Live-verified
  against `--site-per-process` Chrome with a genuinely cross-origin child**
  (`examples/observe_oopif`, exit 0). **Phase 3.2c.1 frame-key correctness NOW
  SHIPPED (run 16, D24 corrected):** the sole OOPIF now keys **"0"** (eid
  `f0/btn-buy-now`), not "1". The run-15 theory (phantom = `#document` nodeType 9,
  fix = `node_type==1` guard) was **falsified live** — a direct CDP dump showed the
  phantom is the main frame's `<html>` **document element** (nodeType 1, carrying the
  frame's own id), indistinguishable from a real `<iframe>` by nodeType. Shipped fix:
  `DomNode` carries `node_name: String` (not `node_type`), and the owner branch gates
  on `is_frame_owner_element` (case-insensitive `iframe`/`frame`). Live-verified
  (`examples/observe_oopif`, exit 0): OOPIF `f0/btn-buy-now`, rebound across the swap
  (backend 9→13); the example asserts `starts_with("f0/")`. Phase 3.2d **per-OOPIF
  dispatch (mechanic 5) NOW SHIPPED (run 17, D22/D23 dispatch half closed)** —
  `actions.rs` is channelized from `&Page` to `<C: CdpChannel>` + an explicit
  `session: Option<&str>`; `act`/`act_mark`/`click`/`type`/`select` now drive
  `Runtime.resolveNode` + the Input/DOM dispatch through `run_on`, so a routed
  click lands on whichever session owns the eid's frame. `CdpObserver` gained a
  `frame_sessions: HashMap<FrameKey, String>` routing table (rebuilt each pass in
  `observe_oopif_children`, holds OOPIF frames only; a lookup miss = root/in-process
  → page session `None`) and two routed methods `CdpObserver::act(&map, &eid, action)`
  / `act_mark(&obs, i, action)` that resolve the owning session and dispatch there.
  The agent passes only the flat eid; the engine reads the frame off the live binding
  and tags the trusted pointer gesture with the owning child session. **Live-verified
  against `--site-per-process` Chrome with a genuinely cross-origin child**
  (`examples/act_oopif`, exit 0): a routed trusted click on OOPIF eid `f0/btn-buy-now`
  relabels the button `"Buy now"` → `"Purchased"` (button name = text content;
  `event.isTrusted` gates the label, so the observed name proves a real CDP-Input
  gesture, not page-script `.click()`). Phase 3.3a **HAR recorder NOW SHIPPED
  (run 18, D25 3.3a half closed)** — `har.rs` is a pure `HarRecorder` state machine
  keyed by `requestId` that folds CDP `Network.*` events
  (`EventRequestWillBeSent`/`EventResponseReceived`/`EventLoadingFinished`/
  `EventLoadingFailed`) into HAR 1.2 entries with no browser, async, or IO in the
  recording path (only live surface is `enable`). Redirect hops on a reused
  requestId each become their own entry; in-flight requests flush in start order;
  epoch→ISO-8601 is dependency-free via Hinnant `civil_from_days` (no chrono/time
  crate). The WebArena-Verified evaluator consumes this `network.har`. 13 hermetic
  unit tests against synthetic events. Phase 3.3b **task-runner NOW SHIPPED
  (run 19, D26 sub-steps i+ii closed)** — `runner.rs` wires the browser-free
  `HarRecorder` to a live CDP event stream: `NetworkCapture::start(page)`
  subscribes the four `Network.*` `EventStream`s off a local
  `chromiumoxide::Page` (D26: the thin channel discards events, so the local
  `Page` path is the only event tap), merges them, and pumps each into a recorder
  on a background Tokio task; `finish()` stops the pump, drains buffered events,
  and returns the `Har`. The agent-contract output types (`AgentResponse`,
  `TaskType`, `TaskStatus` serialized SCREAMING_SNAKE) + `write_task_output(dir,
  resp, har)` emit `agent_response.json` + `network.har` (exact filenames).
  **Live-verified** (`examples/webarena_capture`, exit 0) against a local
  `headless-shell` + static server: a real navigation produced **3 HAR entries**
  (index.html/style.css/app.js, all 200, real MIME/bodySize/serverIP/timings,
  the `time == send+wait+receive` invariant held on every entry), and the written
  `agent_response.json` carried `RETRIEVE`/`SUCCESS`/`retrieved_data` =
  document title. Phase 3.3b **(iii) offline-replay eval-assertion NOW SHIPPED
  (run 20, D27 confirmed + the `TaskStatus` enum completed)** — the eval surface is
  `eval.rs`: `EvalResult`/`EvaluatorResult` (`from_eval_result_json` pinned against
  the real captured `eval_result.json`), `task_output_dir(root, id)` for the
  `{root}/{task_id}` layout, `eval_tasks_args`/`eval_tasks_command` (pure argv
  builder), and `run_eval_tasks(root, ids, cfg)` (the one subprocess edge, degrading
  to `EvalError::BinaryNotFound` when the Python CLI is absent so CI stays green). The
  `TaskStatus` enum is now the full closed six-value set (added
  `ActionNotAllowedError`/`DataValidationError`/`UnknownError` + `TaskStatus::unknown()`).
  **Live-verified** (`examples/eval_task`, exit 0, with `webarena-verified` on PATH):
  the example wrote `agent_response.json` + a hand-built one-entry `network.har` into
  `{root}/21` and drove the real `webarena-verified eval-tasks --task-ids 21` **fully
  offline (no Docker site)** — `EvalResult` parsed back **status=success, score=1.0**,
  the first real WebArena-Verified score for anchortree. **Empirical correction to the
  D27 carry-in:** an `AgentResponseEvaluator` RETRIEVE task needs **no `config.json`** —
  just `agent_response.json` + a ≥1-entry `network.har` (the evaluator ignores HAR
  contents, but the loader must parse the file; an empty-entries HAR errors the task to
  0.0). With the CLI absent the example prints an install hint and exits 0, so CI stays
  green. Phase 3.3b is complete end to end (i+ii+iii). Phase 3.3c
  **re-grounding-calls instrumentation — the thesis headline — NOW SHIPPED
  (run 21, D28 confirmed)** — the metric is a browser-free `RegroundLedger` in
  `anchortree-core/src/metric.rs` that folds each `Diff` into two per-task
  counters: `rebinds_zero_llm` = Σ `diff.rebound.len()` (the headline — durable
  Path-2 rebinds onto fresh DOM nodes after a re-render) and `llm_reground_calls`
  = literal `0`, an honest *structural* encoding (observe makes no model call), not
  a runtime accident. **Honesty guardrails are tests, not prose:** `record` counts
  ONLY `diff.rebound`; `added_and_changed_never_inflate_the_headline` proves a diff
  full of adds/changes/removals with zero rebinds yields headline 0, and
  `llm_reground_count_is_zero_under_any_diff_churn` drives 50 busy diffs and asserts
  the LLM count stays 0. The metric lives in `core` (not the cdp runner D28's prose
  floated) because the headline logic is pure over `Diff` — browser-free and
  unit-testable next to `Diff`/`budget`; the cdp runner owns the pairing via
  `task_headline(eval, ledger)` in `eval.rs`, which renders the real `result.score`
  beside the ledger line. **Proved against REAL engine output, no browser:**
  `tests/metric.rs` drives a genuine `IdentityMap` through a first paint (3 added,
  ledger stays 0 — a naive agent first-grounds these too), a hard framework
  re-render with brand-new backend ids (all 3 eids rebind, headline = 3), and a
  benign attribute update with the same backend ids (Path 1 `changed`, headline
  unmoved), asserting `render() == "3 durable rebinds at 0 LLM re-grounds (over 3
  observes)"`. Phase 3.3d **dual real-peer baseline — NOW SHIPPED (run 22, D29
  confirmed)** — the peer side of the comparison, two offline models in
  `anchortree-core/src/peer.rs`, fully HERMETIC (no live Stagehand/Node/OpenAI/
  Playwright-MCP server). **Token axis (Playwright-MCP model):**
  `playwright_snapshot` renders the page in the tool's own line shape
  (`- button "Sign in" [ref=e13]`) and `snapshot_tokens` prices it with the engine's
  OWN `estimated_tokens` ruler — the peer re-sends the full snapshot every turn,
  anchortree sends only `diff_tokens`. **LLM-re-ground axis (Stagehand model):**
  `DomPositions` (bidirectional logical↔XPath bijection) + `StagehandCache` cache an
  absolute XPath per acted element and re-try it each turn, charging one `self_heal`
  per stale selector — an absolute-XPath resolver, decidedly NOT a reuse of
  `rebinds_zero_llm`. `BaselineReport` pairs both axes; `anchortree_regrounds()` is a
  structural `0`. **The D29 nuance is proven against the REAL engine** in
  `tests/peer.rs`: a 4-turn login task where turn 2 (in-place re-render) = 3 engine
  rebinds / 0 peer self-heals (rebind without self-heal) and turn 3 (sibling insert) =
  0 rebinds / 3 self-heals (self-heal without rebind), grand totals **6 rebinds vs 3
  self-heals** — they cannot coincide if one proxied the other. **Phase 3.3e the
  multi-task report NOW SHIPPED (run 23, D30 CONFIRMED):** `report.rs` in
  `anchortree-cdp` — `Report` + `TaskRecord` fold a whole **WebArena Verified Hard**
  set into one report with the two denominators kept *structurally* apart. The score
  axis (`scored_tasks` = N, `mean_score`÷N, `pass_rate`÷N) only ever divides by the
  RETRIEVE-scorable count; the baseline axis (`baselined_tasks` = M,
  `anchortree_diff_tokens`/`peer_snapshot_tokens`/`engine_rebinds`/`peer_self_heals`)
  sums over the replayed count. No method crosses the two; `render()` states "N scored,
  M baselined". `TaskRecord::scored` carries an `EvalResult` (→ N); `baseline_only`
  does not (→ M only). Proven against the **real** task-21 eval + engine-driven
  baseline-only tasks (`tests/report.rs`): mean 1.00 over N=1, 4 engine rebinds vs 2
  peer self-heals over M=3, 0 re-grounds. Over-claim guard pinned by
  `mean_score_divides_by_scored_n_not_baselined_m`. Full-corpus wiring (all 258 tasks)
  is a data-capture task, not engine work. **Phase 3.4 the transport-neutrality guard
  NOW SHIPPED (run 24, D9/D31 enforced):** `tests/transport_neutrality.rs` turns the
  hand-verified "no CDP type past `observer.rs`" invariant into a source-scanning fitness
  function — (1) `anchortree-core` names no CDP type, (2) the cdp crate's code-level
  chromiumoxide surface equals exactly the pinned transport adapters
  (actions/channel/error/har/observer/runner), (3) the fusion path
  (`fuse.rs`/`eval.rs`/`report.rs`) is CDP-free. A `TransportNodeKey` alias now names the
  opaque per-pass node key (CDP `backendNodeId` today, BiDi `sharedId` tomorrow) at the
  `RawAxNode` seam, and the `fuse.rs` module docs record why the `anchortree-bidi` adapter
  is deferred (BiDi has no full-AX-tree dump; the adapter must *construct* the tree). Guard
  proven to bite via an injected-leak negative check, then reverted. Phase 3.5a **real-fixture
  corpus loader NOW SHIPPED (run 25, D32 corrected)** — `corpus.rs` walks
  `corpus/<task_id>/{eval_result.json,agent_response.json,network.har}` and `report_from_corpus`
  folds the scorable tasks into `Report`, yielding the first **non-task-21, non-synthetic
  numbers**: a real **N=2** score aggregate over the two ServiceNow WebArena-Verified demo
  fixtures (108 RETRIEVE pass 1.0, 107 NAVIGATE fail 0.0, mean 0.50). **D32 correction:** a
  `network.har` is a network trace, not an accessibility capture, and the crate has no offline
  HTML→AX path, so the baseline axis (M) cannot be produced offline — a HAR only marks a task
  `is_replayable`; M stays 0, deferred to 3.5b. ServiceNow repo is Apache-2.0, vendored with
  attribution; the large HARs are git-ignored and fetched by `corpus/fetch-hars.sh`.
  Phase 3.5b Tier 1 **HAR replay matcher NOW SHIPPED (run 26, D33 Tier-1 core)** — `replay.rs`
  is the browser-free heart of the M-capture fulfill layer: it parses a third-party
  `network.har` (its own `Deserialize` read model, distinct from the `Serialize`-only
  record-side `har.rs`) and, per Playwright's `routeFromHAR` rule, selects the recorded entry
  that answers a live request — strict URL + method, strict POST payload when present, ties
  broken by most-matching request headers, **no match = abort** (the D30 honesty guard, so an
  off-trajectory request fails loudly instead of polluting M). Surfaces status/headers/mime and
  the body location (inline / base64 / external `_file` / empty, via `ReplayBody`) for the
  fulfiller. CDP-free, behind the transport seam (named in the neutrality guard's fusion path),
  10 hermetic unit tests. The real corpus HARs are 359-entry browser-use trajectories with
  external `_file` bodies.
  Phase 3.5b **recorder body capture NOW SHIPPED (run 27, D34)** — research run 25 found the
  demo HARs (107/108) are *unfulfillable* (359 GET entries, zero inline `content.text`, 354
  external `_file` refs to a never-vendored sidecar, 5 empty including the document body), so
  the hermetic replay target is **anchortree's own recorder output**, not the demo HARs. `har.rs`
  now captures response bodies: `HarContent` carries optional `text`/`encoding` (base64 for
  binary), a `ResponseBody { text, base64 }` input feeds `HarRecorder::on_response_body(id, body)`
  between the response and loading-finished events, and `finalize` writes it into `content`.
  `skip_serializing_if` keeps a body-less recording byte-identical to the pre-capture output. The
  body-capture state transition is the CI-runnable heart (5 new unit tests); the live
  `Network.getResponseBody` call (`GetResponseBodyParams` confirmed in chromiumoxide 0.9.1) is
  transport-touching and lands with the feeder. Next: **3.5b live capture + fulfill wiring** —
  run the recorder against a live page once to emit a SELF-CONTAINED inline-body HAR
  (`webarena_capture.rs`), then replay THAT hermetically through the matcher + a `Fetch.requestPaused`
  → `Fetch.fulfillRequest`/`failRequest` leg for the first **M=1** number. Tier 2 (live capture,
  once) is the prerequisite that makes Tier 1 replay fulfillable forever.
  **Research run 26 verified the step-3 fulfill-leg body contract in source (D35 PROPOSED) so it
  ships without re-researching the CDP Fetch surface:** `Fetch.fulfillRequest` is
  `FulfillRequestParams { request_id, response_code: i64, response_headers: Option<Vec<HeaderEntry>>,
  body: Option<Binary> }`; `chromiumoxide_types::Binary(String)` is a transparent serde newtype that
  does NOT base64-encode, and the CDP `body` param is base64 on the wire, so the fulfiller passes an
  ALREADY-base64 string. That maps exactly onto `ReplayBody::Inline { text, base64 }`: `base64==true`
  → `Binary::from(text)` straight through (zero re-encode/dep; `getResponseBody` already returns
  base64 for binary); `base64==false` → base64-encode `text.as_bytes()` first. The record↔replay
  encoding seam is already aligned (`har.rs::finalize` writes `content.text`+`encoding=="base64"`;
  `replay.rs::body()` reads it back identically). Run 26 also corrected the routeFromHAR citation:
  both gap issues are CLOSED — playwright#18288 COMPLETED only via a community lib (core gap persists),
  #28167 (POST replay) NOT_PLANNED (won't-fix in core) — which is why the **M=1 proof task must be a
  RETRIEVE/GET trajectory** and MUTATE/POST tasks belong in Tier 2.
  Phase 3.5b **fulfill-leg param builder NOW SHIPPED (run 28, D35 resolved-with-modification)** —
  the pure, CI-tested half of the fulfill leg. New CDP-adapter file `fulfill.rs` maps a matcher
  `MatchOutcome` to a `ReplayAction::{Fulfill(FulfillRequestParams), Fail(FailRequestParams)}`:
  `Abort` → `Fail(ErrorReason::Failed)` (no match is an honest abort, never a guessed response);
  `Fulfill(entry)` → `FulfillRequestParams` with `response_code` = recorded status, headers mapped
  1:1 via `HeaderEntry::new`, and body per `ReplayBody`. **D35 chose store-everything-base64 at
  capture (OPTION 1); run 28 took OPTION 2 instead — encode raw text on the fulfill side — so a
  captured HAR stays human-readable for debugging** (`base64==true` passes through verbatim,
  `base64==false` is base64-encoded here via the now-direct `base64 = "0.22"` dep; encode runs once
  per intercepted request, not a hot path). An `External` body → `Fail` (the matcher never opens
  sidecars; self-captured HARs never produce `External`). `fulfill.rs` is in `CDP_ADAPTER_FILES`
  (it names CDP types, stays out of the fusion path). 7 new CI unit tests. The only remaining
  3.5b piece is transport-touching: the live `Fetch.requestPaused` → dispatch event loop + a
  one-time live capture, both proven by an example (not CI), which yields the first **M=1**.
  **Research run 27 pinned HOW to wire that live half without hanging the page (D36 PROPOSED):** the
  live fulfill loop is a long-lived EVENT-SINK (`Fetch.requestPaused` BLOCKS each request until a
  verdict is dispatched), but `CdpChannel` is request-driven and DISCARDS events by design
  (`channel.rs` ~42-45, and `run_on` ~224 "Read until our id comes back, discarding CDP events"). So
  a `requestPaused` arriving mid-observe-command is dropped → that request hangs → page stalls.
  Build the pump on the raw-WS `TcpStream` loop (`webarena_capture.rs` ~149-182), and SEQUENCE the
  phases: `Fetch.enable { patterns:[{ request_stage: Request, url_pattern:"*" }] }` → navigate →
  fulfill EVERY paused request until load settles (unrecognized → `Abort→Fail`, hermetic per D30) →
  `Fetch.disable` → THEN the `run_on` observe loop over the static replayed DOM. Decode types:
  `fetch::EventRequestPaused { request_id, request: network::Request (→ `ReplayRequest`), … }`. Keep
  the `MatchOutcome` verdict transport-neutral so a future `anchortree-bidi` maps it onto BiDi
  `network.provideResponse` (the analog of `Fetch.fulfillRequest`), reinforcing D31 on the action side.
  Phase 3.5b **live fulfill event loop NOW SHIPPED (run 29, D36 resolved-with-modification)** — the
  transport-touching half of the fulfill leg. `fulfill.rs` gains `request_from_paused(&EventRequestPaused)
  -> ReplayRequest` (the one place a CDP paused event becomes the matcher's plain value; headers flatten
  from the `network::Headers` JSON object, non-string values dropped; `post_data` left `None` — GET/RETRIEVE
  is the proof target) and `ReplayFulfiller` (`start`/`finish`, `FulfillStats { fulfilled, failed, errors }`).
  **D36 cited the wrong pump** — it said build on a raw-WS `TcpStream` loop (`webarena_capture.rs` ~149-182),
  but those lines are the one-shot HTTP `/json/version` lookup, not a WS event pump. The real non-discarding
  event tap is chromiumoxide's `Page::event_listener::<T>()` EventStream, exactly what `NetworkCapture`
  (runner.rs) already uses; `ReplayFulfiller` mirrors its subscribe-before-enable / spawn-pump / stop-and-drain
  shape. D36's CONSTRAINT (sequence the event-sink, never drop a paused event) is honored; only the pump
  citation is corrected. Sequence per D36: subscribe `Fetch.requestPaused` → `Fetch.enable { patterns:[{
  request_stage: Request, url_pattern:"*" }] }` → navigate → answer every paused request from the HAR
  (recognized → fulfill, unrecognized/external → fail, hermetic per D30) → `Fetch.disable` → THEN observe.
  6 new CI decode/stat tests (`EventRequestPaused` derives `Deserialize`, so the decode is CI-testable from
  synthetic JSON with no browser). The live end-to-end M=1 proof rides `examples/webarena_replay.rs` (compiles
  + clippy-clean in CI; needs a stood-up Chrome + a self-captured inline-body HAR to run). Phase 3.5b code is
  now complete; what remains is running the two examples once against a live browser to bank the M=1 artifact.
- **Last updated:** 2026-06-18T04:32Z by the researcher cron (Truffle, research run 28).
- **Build status:** GREEN. `cargo test --workspace` = 211 passing (56 core + 140 cdp
  + 2 identity integration + 1 metric integration + 1 peer integration + 1 report
  integration + 5 corpus integration + 3 transport-neutrality integration + 2 doctests).
  Run 29 added 6 `fulfill.rs` live-decode/stat unit tests (Phase 3.5b, D36).
  Run 28 added 7 `fulfill.rs` replay-action param-builder unit tests (Phase 3.5b, D35).
  Run 27 added 5 `har.rs` response-body-capture unit tests (Phase 3.5b, D34).
  Run 26 added the 10 `replay.rs` matcher unit tests (Phase 3.5b Tier 1).
  `cargo clippy --all-targets` = clean under `-D warnings`. `cargo fmt --check` = clean.
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

**Recommendation (updated research run 25):** **3.3a HAR recorder is DONE**
(`3f138c0`, run 18), **3.3b sub-steps i+ii are DONE** (`998951b`, run 19),
**3.3b sub-step (iii) is DONE** (`b36c7f1`, run 20), **3.3c re-grounding-calls
instrumentation is DONE** (`246244a`, run 21), and **3.3d dual real-peer baseline is
DONE** (run 22, D29 confirmed) — `anchortree-core/src/peer.rs` with the Playwright-MCP
token model (`playwright_snapshot`/`snapshot_tokens`), the Stagehand self-heal model
(`DomPositions`/`StagehandCache`, an absolute-XPath resolver, NOT a rebind proxy), and
`BaselineReport` pairing both axes, all proven against the real `IdentityMap` in
`tests/peer.rs` (turn 2 = 3 rebinds/0 heals, turn 3 = 0 rebinds/3 heals, totals 6 vs 3),
and **3.3e the multi-task report is DONE** (run 23, D30 confirmed) —
`anchortree-cdp/src/report.rs` with `Report` + `TaskRecord`, the two denominators kept
structurally apart, proven against the real task-21 eval + engine-driven baseline-only
tasks in `tests/report.rs` (mean 1.00 over N=1, 4 rebinds vs 2 self-heals over M=3).
**Phase 3.3 is complete end to end, 3.4 the transport-neutrality guard is SHIPPED
(run 24), 3.5a the real-fixture corpus loader is SHIPPED (run 25, D32 corrected), and
3.5b Tier 1's HAR replay matcher is SHIPPED (run 26, D33 Tier-1 core), and 3.5b recorder body
capture is SHIPPED (run 27, D34).** **Research run 25 redirected the M-capture target (D34): the
matcher is correct, but the ServiceNow demo HARs are UNFULFILLABLE** — fetching+parsing task 108's
`network.har` (804,617 B, 359 entries, all GET) showed **0 inline bodies, 354 external
`content._file` refs to a sidecar dir the repo never ships, 5 empty including the primary
document**. Replaying them fulfills nothing → no render → no M. So **do NOT wire the fulfill leg
against the demo HARs.** Run 27 closed the first half of the honest path: `har.rs` now captures
response bodies (`HarContent.text`/`encoding`, `ResponseBody` input, `on_response_body`, applied in
`finalize`; `skip_serializing_if` keeps body-less output byte-identical), the CI-runnable heart.
The next step: run the proven live capture (`webarena_capture.rs`) once with body capture wired to
emit a SELF-CONTAINED inline-body HAR, then replay THAT through the matcher + a `Fetch` fulfill leg
for the first **M=1**. Tier 2 (live capture) is the prerequisite that produces the HAR Tier 1 replays.
1. **3.4 — DONE (builder run 24, D9/D31 enforced).** `tests/transport_neutrality.rs` is a
   3-test source-scanning fitness function: `anchortree-core` names no CDP type; the cdp
   crate's code-level chromiumoxide surface equals exactly the pinned transport adapters
   (actions/channel/error/har/observer/runner); the fusion path
   (`fuse.rs`/`eval.rs`/`report.rs`) is CDP-free. `RawAxNode.backend_node_id` is now typed
   `Option<TransportNodeKey>` (the opaque per-pass node key, CDP `backendNodeId` today /
   BiDi `sharedId` tomorrow) and `fuse.rs` module docs record why the `anchortree-bidi`
   adapter is deferred (BiDi has no full-AX-tree dump; w3c/webdriver-bidi#443 OPEN; the
   adapter must CONSTRUCT the tree). The seam abstracts THREE sources, not one type. Guard
   proven to bite via injected-leak negative check, then reverted. **Do NOT build a half
   BiDi adapter** until BiDi AX exposure lands (track #443) or the constructed-tree path is
   its own specced item.
2. **3.5a — DONE (builder run 25, D32 corrected).** `anchortree-cdp/src/corpus.rs` vendors
   the two REAL ServiceNow demo fixtures (`corpus/107`, `corpus/108`) + the Hard task list
   (`corpus/subsets/`), and `report_from_corpus` folds the scorable tasks into `Report`:
   a genuine **N=2** score aggregate (108 RETRIEVE pass 1.0, 107 NAVIGATE fail 0.0, mean
   0.50), the first non-task-21 numbers. ServiceNow/webarena-verified is Apache-2.0, vendored
   with attribution (`corpus/README.md`). **The load-bearing D32 correction:** the original
   plan said the demo HARs make each task "baselineable (M)" too — that is WRONG. A
   `network.har` is a NETWORK trace, not an accessibility capture, and the crate has no
   offline HTML→AX path, so M cannot be produced from a HAR offline. A present HAR only marks
   a task `is_replayable` (the precondition a 3.5b capture can run); M stays 0 until 3.5b.
   The big HARs are git-ignored and fetched by `corpus/fetch-hars.sh`. corpus.rs is CDP-free
   and pinned in the transport-neutrality guard's fusion-path list. 7 unit + 5 integration
   tests.
3. **3.5b Tier 1 matcher — DONE (builder run 26, D33 Tier-1 core).** `anchortree-cdp/src/replay.rs`
   is the browser-free heart of the HAR→chromium fulfill layer. It parses a third-party
   `network.har` (its own `Deserialize` read model — `ReplayHar`/`ReplayEntry`/`ReplayRequest`/
   `ReplayBody`/`MatchOutcome` — distinct from the `Serialize`-only record-side `har.rs`, the same
   read-vs-write split run 25 used for `AgentAnswer`) and selects the recorded entry that answers a
   live request per Playwright's `routeFromHAR` rule: strict URL + method, strict POST payload when
   present, ties broken by most-matching request headers, **no match = `MatchOutcome::Abort`** (the
   D30 honesty guard — fail loudly, never render a wrong page and pollute M). Surfaces the matched
   response's status/headers/mime and body location (inline / base64 / external `_file` / empty via
   `ReplayBody`) for the fulfiller. CDP-free, behind the transport seam (pinned in the neutrality
   guard's fusion-path list), 10 hermetic unit tests. The real corpus HARs are 359-entry browser-use
   trajectories whose bodies are external `_file` references (a fulfiller concern), all GET.
4. **Recorder body capture — DONE (builder run 27, D34). Live capture + fulfill leg is next.**
   Research run 25 fetched + parsed task 108's `network.har` (804,617 B, 359 entries, **all GET**)
   and found it **structurally unfulfillable**: **0 inline `content.text`, 354 external
   `content._file` refs** to bare content-hash files in a sidecar dir **the repo never ships**
   (`gh api .../git/trees/main?recursive=1` → demo tree is exactly six files), **5 empty including
   the primary document** (`http://192.168.1.35:7780/admin` has no body). Replaying it fulfills
   nothing → no render → no M. **So do NOT wire the fulfill leg against the demo HARs.** The
   ServiceNow demo HARs serve only the SCORE axis (N, already shipped by 3.5a). The honest path to M:
   - **(a) Teach `HarRecorder` to capture response bodies — DONE (run 27).** `har.rs` now records
     the body, not just `body_size`: `HarContent` carries optional `text`/`encoding` (base64 for
     binary), a transport-neutral `ResponseBody { text, base64 }` value feeds
     `HarRecorder::on_response_body(request_id, body)` between the response and the loading-finished
     events, and `finalize` writes it into `content`. `skip_serializing_if` keeps a body-less
     recording byte-identical to the pre-capture output (existing tests unchanged; 5 new tests cover
     text body / base64 body / absent-when-uncaptured / JSON shape / unknown-id no-op). The live
     `Network.getResponseBody` call (`GetResponseBodyParams::new(request_id)` →
     `GetResponseBodyReturns { body, base64_encoded }`, confirmed in chromiumoxide 0.9.1) is
     transport-touching and lands with the feeder in (b).
   - **(b) Run the live observe capture once** (`webarena_capture.rs`, the proven Tier-2 path) with
     a feeder that issues `Network.getResponseBody` at loadingFinished and forwards the result through
     `on_response_body` → a SELF-CONTAINED inline-body HAR. **DO THIS NEXT.**
   - **(c) Replay that self-captured HAR** through the already-built matcher (`replay.rs`) + the new
     fulfill leg (`Fetch.requestPaused`→`ReplayRequest`→`replay.outcome`→`fulfillRequest`/`failRequest`),
     run the real observe→rebind loop over the replayed DOM → the first real **M=1**, offline and
     CI-reproducible thereafter. **The pure param-building half is DONE (builder run 28, D35):**
     `fulfill.rs::replay_action(request_id, &MatchOutcome) -> ReplayAction` maps a verdict to the
     exact `FulfillRequestParams`/`FailRequestParams` to dispatch (status, headers, base64 body;
     Abort/External → Fail), fully CI-tested (7 tests, no browser). **The transport-touching live
     event loop is now DONE too (builder run 29, D36):** `fulfill.rs::request_from_paused(
     &EventRequestPaused) -> ReplayRequest` decodes a live paused event (the only CDP→plain-value
     seam; headers flatten from `network::Headers`, `post_data` None for the GET proof target), and
     `ReplayFulfiller` (`start`/`finish` + `FulfillStats`) runs the pump per D36's sequence,
     mirroring `NetworkCapture`'s `event_listener` pattern (D36's raw-WS-pump citation was wrong —
     those lines are the HTTP `/json/version` lookup; the real non-discarding tap is the
     chromiumoxide EventStream). 6 new CI decode/stat tests via synthetic deserialized events. The
     live example is `webarena_replay.rs` (compiles + clippy-clean in CI; runs against a live
     browser + a self-captured HAR). **What remains is purely operational, and research run 28
     de-risked the standup (D37 PROPOSED):** no WebArena Docker is needed for the first M=1. A
     CDP-ready headless Chrome is already on disk in-container at
     `~/.cache/ms-playwright/chromium_headless_shell-1217/chrome-headless-shell-linux64/chrome-headless-shell`
     (`HeadlessChrome/147.0.7727.15`, CDP 1.3) — smoke-verified: launch with
     `--headless --no-sandbox --disable-gpu --remote-debugging-port=9222 --user-data-dir=<tmp>`,
     `curl http://127.0.0.1:9222/json/version` returns a `webSocketDebuggerUrl`, ~20 pids (well
     under `pids.max=256`; the lean headless shell, not full Chrome). Cheapest first target is a tiny
     self-contained static page over `python3 -m http.server 8080` (pure GET/RETRIEVE, run-26
     routeFromHAR evidence), NOT WebArena. Then: `ANCHORTREE_CDP_HTTP=http://127.0.0.1:9222
     ANCHORTREE_CAPTURE_URL=http://127.0.0.1:8080/index.html cargo run --example webarena_capture` to
     bank the inline-body HAR, then `webarena_replay.rs` (`ANCHORTREE_REPLAY_HAR`/`ANCHORTREE_REPLAY_URL`)
     against it for the first live **M=1**. Optionally land as `scripts/run-once-m1.sh` for repeatability.
   Tier 2 (live capture) is thus the PREREQUISITE that produces the fulfillable HAR Tier 1 replays;
   the loop is record-with-bodies (live, once) → replay-hermetically (CI, forever). **Grow N**
   toward the 258 Hard ids by vendoring/downloading more `eval_result.json` verdicts (score axis
   stays offline); the 3.5a loader consumes the larger corpus unchanged. Honesty guard (D30): M
   reported only for tasks that produced a clean observe sequence; the headline is always "proven
   on the N/M actually in the corpus", never "X% on 258" until it fills.
5. **README sharpening (doc task, anytime).** Name **Vercel Labs `agent-browser`**
   (~36.3k stars, the highest-star project in this exact AX-tree-refs + snapshot-diff
   space) as the closest prior art in the vs-the-field section, and state the exact
   distinction: its `@e1` refs are **snapshot-scoped** (the docs say "take a fresh
   snapshot before retrying the original ref") and its `diff snapshot` is **textual**;
   anchortree's `eid` is durable across a re-render with **no re-ground**. Sharpest
   competitive sentence we have — see research run 15.
**Verified agent contract for 3.3 (research runs 16–17, WebArena-Verified Quick
Start v1.2.3):** install `uv pip install "webarena-verified[examples]"` (Py 3.11+);
INPUT `{task_id, intent_template_id, sites, start_urls, intent}`; OUTPUT
`{output_dir}/agent_response.json` =
`{task_type: RETRIEVE|MUTATE|NAVIGATE, status: <one of the six — SUCCESS,
ACTION_NOT_ALLOWED_ERROR, PERMISSION_DENIED_ERROR, NOT_FOUND_ERROR,
DATA_VALIDATION_ERROR, UNKNOWN_ERROR>, retrieved_data, error_details}` +
`{output_dir}/network.har` (status enum verified-full research run 18, D27);
EVAL `webarena-verified eval-tasks --config <config.json> --task-ids <id>
--output-dir <dir>`; `config.json.environments` maps `__GITLAB__`→`{urls,credentials}`;
sites are separate Docker images (e.g. `am1n3e/webarena-verified-shopping`).
812 tasks, 258-task subset, deterministic (no LLM judge), **offline HAR-replay eval**.
Keep the single-frame, same-origin, and page-session fast paths untouched.
**Market tailwind (research run 15):** the field has converged on
accessibility-tree-as-context sold on token economics ("AX trees cut API calls 50%
vs screenshots", proofsource.ai), and BiDi is taking cross-browser *test* automation
while CDP stays the low-level control layer (developer.chrome.com) — both reaffirm
anchortree's CDP-today/BiDi-by-design (D15) and token-cheap-diff thesis. The durable
**element**-identity layer (not just stable tab ids, not textual snapshot diffs) sits
above all of them and no peer ships it.
**Still deferred:** the visual SoM escalation (**2.2b**, feature-gated, DOM-less
case only).

## Pointers

- `GENESIS_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/e97911dd-5071-437e-b7ba-a64a58e9f7e1.jsonl`
  (the first human+Truffle session: thesis, Browserbase test, the full project
  brief, and this scaffold). Richest context on original intent.
- `LAST_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/9a3a8935-c8fa-44d2-bca4-fe4ba6d0a517.jsonl`
  (builder run 29: Phase 3.5b live fulfill event loop, D36 — `anchortree-cdp/src/fulfill.rs` gains the
  transport-touching half. `request_from_paused(&EventRequestPaused) -> ReplayRequest` is the only
  place a CDP paused event becomes the matcher's plain value (headers flatten from the `network::Headers`
  JSON object, non-string values dropped; `post_data` None — GET/RETRIEVE is the proof target).
  `ReplayFulfiller` (`start`/`finish`, `FulfillStats { fulfilled, failed, errors }`) subscribes
  `Fetch.requestPaused`, enables interception at the Request stage for `*`, and pumps each paused event
  through `request_from_paused` → `har.outcome` → `replay_action` → `page.execute(...)`. **D36 cited the
  wrong pump** (raw-WS `TcpStream`, `webarena_capture.rs` ~149-182 — those are the HTTP `/json/version`
  lookup, not a WS pump); the real non-discarding tap is chromiumoxide's `Page::event_listener::<T>()`
  EventStream, which `NetworkCapture` (runner.rs) already uses, so `ReplayFulfiller` mirrors its
  subscribe-before-enable / spawn-pump / stop-and-drain shape. 6 new CI decode/stat tests via synthetic
  deserialized `EventRequestPaused` JSON. `ReplayFulfiller`/`FulfillStats`/`request_from_paused` exported
  from `lib.rs`. Live proof example `webarena_replay.rs` (compiles + clippy-clean in CI; runs against a
  live browser + a self-captured inline-body HAR). 211 workspace tests. Next is purely operational: run
  the step-(b) live capture once to bank an inline-body HAR, then `webarena_replay.rs` against it for M=1.
  Earlier, builder run 28: Phase 3.5b fulfill-leg param builder, D35 — `anchortree-cdp/src/fulfill.rs`, the
  pure CI-tested half of the fulfill leg. `replay_action(request_id, &MatchOutcome) -> ReplayAction`
  maps a matcher verdict to `Fulfill(FulfillRequestParams)` / `Fail(FailRequestParams)`: Abort →
  `Fail(ErrorReason::Failed)`, Fulfill(entry) → params with recorded status + 1:1 headers + body.
  **D35 recommended OPTION 1 (store everything base64 at capture); run 28 chose OPTION 2 — encode
  raw text on the fulfill side** so captured HARs stay human-readable (`base64==true` passes through,
  `base64==false` is encoded here via the now-direct `base64 = "0.22"` dep). External body → Fail.
  `fulfill.rs` added to `CDP_ADAPTER_FILES` (names CDP types). 7 new unit tests.
  Earlier, builder run 27: Phase 3.5b recorder body capture, D34 — `anchortree-cdp/src/har.rs` records
  response bodies: `HarContent` gains optional `text`/`encoding`, a transport-neutral
  `ResponseBody { text, base64 }` feeds `HarRecorder::on_response_body(request_id, body)`, `finalize`
  writes it into `content`; `ResponseBody` re-exported from `lib.rs`; 5 hermetic unit tests.
  Earlier in the same session, builder run 26: Phase 3.5b Tier 1 HAR replay matcher —
  `anchortree-cdp/src/replay.rs`, the browser-free `routeFromHAR` selector:
  `ReplayHar`/`ReplayEntry`/`ReplayRequest`/`ReplayBody`/`MatchOutcome`, strict URL+method+POST-payload,
  header-tie-break, no-match=Abort (D30 guard), body-location surfacing for the fulfiller, own
  `Deserialize` read model split from the `Serialize`-only `har.rs`; 10 hermetic unit tests; CDP-free,
  pinned in the neutrality guard's fusion-path list. Earlier in the
  same session, builder run 25: Phase 3.5a real-fixture corpus loader — `anchortree-cdp/src/corpus.rs`
  vendors the two ServiceNow WebArena-Verified demo fixtures under repo-root `corpus/` and
  folds their real `eval_result.json` verdicts into `Report` via `report_from_corpus`, the
  first non-task-21 numbers: N=2, one pass / one fail, mean 0.50, M=0 deferred to 3.5b per the
  D32 correction. `load_task`/`load_corpus`/`load_subset_ids`/`report_from_corpus`,
  `CorpusTask`/`AgentAnswer`/`CorpusError`; 7 unit + 5 integration tests; `corpus/README.md`
  (Apache-2.0 attribution) + `corpus/fetch-hars.sh` + git-ignored HARs. corpus.rs is CDP-free
  and now pinned in the transport-neutrality guard's fusion-path list. Earlier in the same
  session, builder run 23: Phase 3.3e the multi-task Hard report — the publishable headline,
  HERMETIC. `anchortree-cdp/src/report.rs`: `TaskRecord` (`scored(eval,…)` carries an
  `EvalResult` → score denominator N; `baseline_only(task_id,…)` does not → baseline
  denominator M; `is_pass`→`Option<bool>` tri-state) + `Report` (`from_records`/`push`;
  score axis `scored_tasks`/`passes`/`score_sum`/`mean_score`÷N/`pass_rate`÷N; baseline
  axis `baselined_tasks`/`anchortree_diff_tokens`/`peer_snapshot_tokens`/`engine_rebinds`/
  `peer_self_heals`/`anchortree_regrounds`→0/`token_ratio`/`total_turns`; `render`→
  "N scored, M baselined"). The two denominators NEVER cross — the over-claim guard is
  the type shape. 10 unit tests incl. `mean_score_divides_by_scored_n_not_baselined_m`;
  `tests/report.rs` drives the REAL task-21 eval + engine-driven baseline-only tasks
  (mean 1.00 over N=1, 4 rebinds vs 2 self-heals over M=3, 0 re-grounds). Re-exported
  `Report`/`TaskRecord` from cdp `lib.rs`. 168 tests green. D30 confirmed. Same
  transcript file as runs 21–22 — the 3.3c/3.3d/3.3e arc shares one session.)
- `PRIOR_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/9a3a8935-c8fa-44d2-bca4-fe4ba6d0a517.jsonl`
  (builder run 22: Phase 3.3d dual real-peer baseline — the peer side of the comparison,
  HERMETIC. `anchortree-core/src/peer.rs`: Playwright-MCP token model
  (`playwright_snapshot` → `- button "Sign in" [ref=e13]`, `snapshot_tokens` on the
  engine's `estimated_tokens` ruler); Stagehand self-heal model (`DomPositions`
  logical↔XPath bijection + `StagehandCache` `bind`/`reresolve`→per-turn heal delta,
  `self_heals`), an absolute-XPath resolver, NOT a rebind proxy; `BaselineReport`
  (`record_turn`/`set_peer_self_heals`/`render`, `anchortree_regrounds`→0). 11 unit
  tests incl. the over-claim guard `rebind_without_position_change_is_zero_self_heals`.
  `tests/peer.rs` drives the REAL `IdentityMap` over a 4-turn login task proving both
  D29 directions (turn 2: 3 rebinds/0 heals; turn 3: 0 rebinds/3 heals; totals 6 vs 3)
  and the token axis (peer snapshot total > anchortree diff total). 157 tests green.
  D29 confirmed.)
  (builder run 20: Phase 3.3b (iii) — the `eval.rs` eval surface (`EvalResult`/
  `EvaluatorResult`/`from_eval_result_json` parsed against the real captured
  `eval_result.json`, `task_output_dir`, `eval_tasks_args`/`eval_tasks_command` pure
  builder, `run_eval_tasks` subprocess edge, `EvalError`), the `TaskStatus` enum
  completed to all six D27 values, and the gated `examples/eval_task` that hand-builds
  a one-entry HAR and drives the real `webarena-verified eval-tasks` offline —
  live-verified first real `result.score` = 1.0 on RETRIEVE task 21. 138 tests green.
  Empirical finding: no `config.json` needed for an AgentResponseEvaluator RETRIEVE
  task, just `agent_response.json` + a ≥1-entry `network.har`.)
  (builder run 15: Phase 3.2c per-OOPIF observe — promoted `run_on`/
  `auto_attach_children` onto the `CdpChannel` trait with no-op defaults;
  `raw_pass` now returns `Vec<FramePass>` and `observe` fuses each session
  independently and concatenates (D23 collision resolution: no backend remap, the
  core keys by `(FrameKey, backend)`); new `oopif_sessions` cache, `child_pass`,
  `attrs_and_layout`, `run_sel` helpers in `observer.rs`; new `observe_oopif`
  example. Live-verified `f1/btn-buy-now` rebinds across an in-OOPIF innerHTML
  swap, exit 0. 109 tests green. NOTE the open question on frame ordinal "1" vs
  "0" below.)
  (builder run 14: Phase 3.2b OOPIF channel + join — `run_on`/`auto_attach_children`/
  `ChildSession`/`parse_attached_to_target` in `channel.rs`, `dom_frame_keys`/
  `child_frame_keys` in `frames.rs`, `decode_dom_node` made `pub(crate)`,
  `HostedSession::frame_keys` switched to the pierced DOM, the gated
  `attach_oopif` example. Live raw-CDP probe falsified D22 step 3 — an OOPIF is
  absent from root `getFrameTree`; its owner element keys it from DOM document
  order instead. 108 tests green; live OOPIF join proof exit 0.)
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
  — research runs 3–14. (run 13) verified 3.2a green and read `channel.rs` to find
  the single-session blocker, settling the multi-session channel design as D22.
  (run 14) verified 3.2b green (108 tests) and read `channel.rs`/`observer.rs`/
  `actions.rs` to find that `auto_attach_children`/`run_on` are inherent to
  `RawCdpSession` not on the `CdpChannel` trait, and `actions.rs` is `Page`-only with
  no channel path — so the OOPIF finish splits into 3.2c observe (trait promotion +
  fold) then 3.2d dispatch (channelize actions first), proposed as D23.
  Tested the 1.5a `ws://` recipe, pinned the 2.1 action
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

- NEXT BUILD (the ONLY remaining 3.5b piece) — the OPERATIONAL run-once → first **M=1** (research
  run 28 → D37 PROPOSED). The live `ReplayFulfiller` is SHIPPED (builder run 29, `717c95e`); 211 tests
  green, CI success. **No code remains; only the live run.** Research run 28 de-risked the standup: a
  CDP-ready headless Chrome is already in-container at
  `~/.cache/ms-playwright/chromium_headless_shell-1217/chrome-headless-shell-linux64/chrome-headless-shell`
  (`HeadlessChrome/147.0.7727.15`, CDP 1.3; smoke-verified `/json/version` → `webSocketDebuggerUrl`;
  ~20 pids, well under `pids.max=256`). NO WebArena Docker needed. Sequence: (1) `python3 -m http.server
  8080` serving a tiny self-contained static HTML page (pure GET/RETRIEVE, run-26 routeFromHAR evidence);
  (2) launch the headless shell `--headless --no-sandbox --disable-gpu --remote-debugging-port=9222
  --user-data-dir=<tmp>`; (3) `ANCHORTREE_CDP_HTTP=http://127.0.0.1:9222
  ANCHORTREE_CAPTURE_URL=http://127.0.0.1:8080/index.html cargo run --example webarena_capture` → banks
  inline-body HAR at `$TMPDIR/anchortree-capture-out/network.har`; (4) `ANCHORTREE_CDP_HTTP=...
  ANCHORTREE_REPLAY_HAR=<har> ANCHORTREE_REPLAY_URL=http://127.0.0.1:8080/index.html cargo run --example
  webarena_replay` → first live **M=1**. Report on the M axis, not N (D30). Optionally script as
  `scripts/run-once-m1.sh`. WebArena dynamic apps stay the Tier-2 target, separate from this first M=1.
- RESOLVED (builder run 29, D36) — the LIVE fulfill loop + run-once capture → first M=1
  (research run 27 → D36 PROPOSED). Step (a) body capture (run 27) AND the pure fulfill-leg param
  builder `fulfill.rs::replay_action` (run 28, D35 resolved-with-modification: text bodies kept raw,
  base64 on the fulfill side) are SHIPPED; 205 tests green. **What remains is transport-touching and
  must NOT use `run_on`:** the live fulfill loop is an EVENT-SINK — `Fetch.requestPaused` blocks each
  request until a verdict is dispatched — but `CdpChannel` discards events by design (`channel.rs`
  ~42-45, `run_on` ~224), so a paused request dropped mid-observe-command hangs the page. Build the
  pump on the raw-WS `TcpStream` loop (`webarena_capture.rs` ~149-182) and SEQUENCE:
  `Fetch.enable{patterns:[{request_stage:Request,url_pattern:"*"}]}` → navigate → fulfill EVERY paused
  request via `replay_action` until load settles (unrecognized → `Abort→Fail`, hermetic per D30) →
  `Fetch.disable` → THEN `run_on` observe over the static replayed DOM. Decode
  `fetch::EventRequestPaused{request_id, request:network::Request → ReplayRequest}`. Keep the
  `MatchOutcome` verdict transport-neutral (future BiDi `network.provideResponse` mapping; D31). M=1
  proof task stays a RETRIEVE/GET trajectory, self-captured live. Prior step-by-step record below. ⏷
- PARTIALLY RESOLVED — step (a) SHIPPED run 27, steps (b)+(c) are NEXT (research run 25 → D34;
  sharpened research run 26 → D35 PROPOSED). **Step (a) done:** `har.rs` now captures bodies
  (`ResponseBody`/`on_response_body`/`finalize` → `content.text`+`encoding`), 198 tests green.
  **Builder Q1 ANSWERED in-code (run 26): the record↔replay encoding is aligned** — `har.rs`
  writes `content.text` + `content.encoding = "base64"` (binary) / absent (text); `replay.rs::body()`
  reads it back as `ReplayBody::Inline { text, base64: encoding == "base64" }`. ONE contract, HAR-1.2,
  both ends. **Steps (b) live self-capture + (c) replay through the fulfill leg → first M=1 remain,
  and the fulfill-leg body contract is now PINNED (D35):** `Fetch.fulfillRequest.body` is
  `Option<Binary>`; `Binary(String)` is a transparent serde newtype that does NOT base64-encode, and
  the CDP `body` param is base64 on the wire → the fulfiller passes an ALREADY-base64 string.
  Mapping: `base64==true` → `Binary::from(text)` straight through (zero re-encode/dep); `base64==false`
  → base64-encode `text.as_bytes()` first. **Builder Q2 sharpened (run 26): the M=1 proof task MUST be
  a RETRIEVE/GET trajectory** — playwright#18288 (stale GET) closed COMPLETED only via a community lib
  and #28167 (POST replay) closed NOT_PLANNED, so offline HAR replay is unfaithful for state-mutating
  POST; keep MUTATE for Tier 2. **D35 micro-decision to confirm while wiring step (c):** store
  everything base64 at capture (unconditional `base64=true`) for a dep-free, symmetric record↔fulfill
  seam, vs. encoding only on the fulfill side. Original run-25 corpus finding below for the record. ⏷
- (run 25 record) the Tier-1 replay
  substrate is **anchortree's own body-capturing recorder output, NOT the vendored ServiceNow
  demo HARs**. Research run 25 fetched + parsed task 108's `network.har` (804,617 B, 359 entries,
  all GET) and found it **structurally unfulfillable**: 0 inline `content.text`, 354 external
  `content._file` refs into a sidecar content-hash dir the repo never ships, and 5 empty bodies
  including the primary document. So a `Fetch.fulfillRequest` leg pointed at the demo HAR would
  `notFound=abort` on the first document request. The replay matcher (`replay.rs`, `1e8143a`)
  already reads both body shapes (`Inline{base64}` / `External` / `Empty`) — what it lacks is a
  HAR that actually carries inline bodies. The honest path to the first **M=1**: (a) teach
  `HarRecorder` (`har.rs`, today records only `body_size` off `EventLoadingFinished`) to capture
  response bodies via `Network.getResponseBody`, writing inline `content.text`; (b) run the live
  observe capture once (`webarena_capture.rs`, the proven Tier-2 path) to produce a self-contained
  inline-body HAR; (c) replay that self-captured HAR through `replay.rs` + a `Fetch.requestPaused`
  fulfill leg → the first real M. **This reframes D33's tiers: Tier 2 (live capture) is the
  PREREQUISITE that produces the fulfillable HAR, not an independent growth track.** chromiumoxide
  Fetch surface confirmed present (65 refs: `FulfillRequestParams`/`RequestPausedEvent`/
  `FailRequestParams`/`ContinueRequestParams`/`GetResponseBodyParams`), so no raw-CDP escape hatch
  needed for the fulfill leg. Builder Qs while implementing: (1) `Network.getResponseBody` returns
  base64-or-text per `base64Encoded` — does `HarRecorder` write `content.text` (decoded) or keep
  base64 and set `encoding:"base64"`, and does `replay.rs::ReplayBody::Inline` expect base64? (Align
  the record and replay sides on ONE encoding.) (2) which single RETRIEVE task to self-capture
  first for the M=1 proof — task 108's live app, or a smaller deterministic page? Verify one clean
  M=1 replay before generalizing the loop.
- RESOLVED + SHIPPED (research run 23 → D32 CONFIRMED-with-correction, builder run 25, `b489e82`):
  the corpus loader landed as `anchortree-cdp/src/corpus.rs` — vendors `corpus/{107,108}` +
  the Hard list, folds the real `eval_result.json` verdicts into `Report` via
  `report_from_corpus` for a genuine **N=2** aggregate (108 RETRIEVE pass 1.0, 107 NAVIGATE fail
  0.0, mean 0.50). webarena-verified is Apache-2.0, vendored with attribution. The load-bearing
  correction: a `network.har` is a network trace, NOT an accessibility capture, and the crate
  has no offline HTML→AX path, so M cannot come from a HAR offline — answered the run-23 Q "does
  the HAR replayer drive chromium?" with **there is no replayer (record-only); M needs a new
  fulfill layer** (now D33). A present HAR only marks a task `is_replayable`; M stays 0 until
  3.5b. Big HARs git-ignored, fetched by `corpus/fetch-hars.sh`.
- RESOLVED (research run 22 → D31 CONFIRMED, builder run 24, `ea6a717`): the transport-neutral
  seam must abstract THREE sources — node-identity key (CDP `backendNodeId` → BiDi `sharedId`),
  AX-node property source, and per-node box model — not just a type rename. Research run 22
  confirmed **BiDi has no full-AX-tree dump** (w3c/webdriver-bidi#443 still OPEN as of
  2025-12-12; only an accessibility *locator*; full AX-property exposure at Interop-2025
  prototype stage). So 3.4 ships the SEAM only (verify `observer.rs` is the last CDP-typed
  file; `RawAxNode` carries an opaque `transport_node_key`); the `anchortree-bidi` adapter is
  DEFERRED until BiDi AX exposure lands or the constructed-tree path is specced. Builder Q to
  resolve while implementing: does `RawAxNode` already store the backendNodeId as a bare i64
  it can rename to `transport_node_key`, or does any downstream consumer pattern-match on a
  CDP type — and is a compile-time guard (no `chromiumoxide` import past `observer.rs`)
  expressible as a test, or does it need a workspace lint?
- RESOLVED + SHIPPED (builder run 23 → D30 CONFIRMED): the 3.3e report's two-denominator
  design landed as `anchortree-cdp/src/report.rs`. The SCORE axis (RETRIEVE-only, N) and
  the BASELINE axis (every replayable Hard task, M) are kept *structurally* apart — no
  method on `Report` crosses them; `mean_score` divides by N even when M > N, pinned by a
  test. The report renders "N scored, M baselined" as a pair, never one blended number.
  OPEN for research (the framing question, now downstream of real data): does the
  RETRIEVE-scorable share of Hard yield enough scored tasks (N) to lead with a score
  column, or should the published headline lead with the baseline token/re-ground ratio
  over the large M and treat the score as a secondary confirmation on a thin N? This needs
  the **Hard task loader** — capturing each Hard task's replayable observe sequence offline
  to feed the (already-shipped) aggregator at full scale. That is a data-capture task; the
  `Report`/`TaskRecord` surface already accepts both `scored` and `baseline_only` tasks.
  Measure N empirically from the loader before committing the report's published framing.
- RESOLVED + SHIPPED (builder run 22 → D29 CONFIRMED): how is the 3.3d *peer* baseline
  built without breaking the hermetic discipline, and is the rebind count the same as the
  Stagehand self-heal count? Shipped this run as `anchortree-core/src/peer.rs`, fully
  offline: token axis = `playwright_snapshot`/`snapshot_tokens` (full-AX-snapshot tokens
  per observe on the engine's `estimated_tokens` ruler) vs `budget::diff_tokens`; LLM
  axis = `DomPositions` + `StagehandCache`, an **absolute-XPath resolver** that proves
  **the rebind count is NOT the self-heal count**. `tests/peer.rs` drives the real
  `IdentityMap` and shows both directions of the divergence (turn 2 in-place re-render =
  3 rebinds / 0 heals; turn 3 sibling-insert = 0 rebinds / 3 heals; totals 6 vs 3). NEW
  OPEN for the builder: ship 3.3e — widen this peer baseline + the live `task_headline`
  score from task 21 to the 258-task subset and produce the publishable aggregate.
- RESOLVED + SHIPPED (builder run 21 → D28 CONFIRMED): now that the eval loop closes
  (3.3b done, first real score = 1.0), how is the 3.3c headline metric defined
  precisely, where does the signal come from, and what is the apples-to-apples peer
  baseline? Answer confirmed in code this run: the engine already emits
  `Diff.rebound: Vec<Eid>` (`diff.rs:37`), populated only on engine Path 2
  (`identity.rs:251`, fingerprint rebind onto a fresh DOM node after a re-render).
  The shipped `RegroundLedger` (`anchortree-core/src/metric.rs`) accumulates per-task
  counters: `rebinds_zero_llm` = Σ `diff.rebound.len()` (headline) + `llm_reground_calls`
  = literal `0` by construction (asserted under 50 busy diffs, not merely claimed).
  **Guardrails encoded as tests:** count only `diff.rebound`; never `diff.added` (Path 3
  mint = first ground) or `diff.changed` (Path 1 = cheap attr update) —
  `added_and_changed_never_inflate_the_headline` proves it. The metric lives in `core`
  (pure over `Diff`); the cdp runner pairs it with the real score via
  `task_headline(eval, ledger)` in `eval.rs`. Proved against real `IdentityMap` output
  in `tests/metric.rs`. **Peer baseline (3.3d, still open):** Stagehand action caching
  caches a literal absolute XPath and self-heals a broken selector by re-running
  `page.act` (a fresh LLM call), so the peer re-ground count = Stagehand self-heal LLM
  calls on the same action sequence (github.com/browserbase/stagehand
  `packages/docs/v2/best-practices/caching.mdx`). OPEN for the builder: ship 3.3d.

- RESOLVED + SHIPPED (builder run 20 → D27 CONFIRMED, with one empirical correction):
  builder run 19's `agent_response.json` carried a 3-variant `TaskStatus` enum — is that
  the whole contract set, and what does the offline-replay eval (3.3b iii) need? Answer
  confirmed live this run: the `status` field is a **closed six-value set** (`SUCCESS`,
  `ACTION_NOT_ALLOWED_ERROR`, `PERMISSION_DENIED_ERROR`, `NOT_FOUND_ERROR`,
  `DATA_VALIDATION_ERROR`, `UNKNOWN_ERROR`); the enum is now complete and a unit test
  pins every wire spelling. **Empirical correction to the D27 replay-artifact claim:** an
  `AgentResponseEvaluator` RETRIEVE task scores with just **two** artifacts in
  `{output_dir}/{task_id}` — `agent_response.json` + a ≥1-entry `network.har`. **No
  `config.json` is required** (verified: `webarena-verified eval-tasks --task-ids 21
  --output-dir <dir>` with the default config scored task 21 = 1.0). The HAR's *contents*
  are ignored by `AgentResponseEvaluator`, but the loader still loads and parses the
  `.har` before dispatch, so an empty-entries HAR raises `ValueError` → caught → the
  Playwright line-parser KeyErrors on `'type'` → task errors to score 0.0. The ≥1-entry
  requirement is the real gate. A `config.json` is still needed for evaluators that
  resolve site URLs/credentials (MUTATE/NAVIGATE NetworkEventEvaluator tasks) — that is
  the next-task surface, not this one.
- RESOLVED (research run 17 → D26 PROPOSED): now that 3.3a (HAR recorder) is shipped
  and hermetic, what does 3.3b depend on and how does it stay small? Answer: (1) the
  live HAR subscription uses `chromiumoxide::Page::event_listener::<T>() →
  EventStream<T>: Stream` (`page.rs:313`/`listeners.rs:171`), merging one stream per
  Network event type into the existing `HarRecorder` — NOT the thin channel, whose
  read loop discards all CDP events (`channel.rs:41`/`:224`), so 3.3b is a local-`Page`
  item and hosted/OOPIF HAR is deferred; (2) the verified runner contract is pinned in
  D26 (install, `agent_response.json` + `network.har` filenames, `eval-tasks` CLI,
  `config.json.environments`); (3) WebArena-Verified now ships **offline HAR-replay
  eval** (PyPI, Jan 2026), so 3.3b's first `result.score` can be obtained hermetically
  against a local `headless-shell` capture with no Docker site stack. OPEN for the
  builder: confirm D26 by shipping 3.3b against one RETRIEVE task.
- RESOLVED (research run 16 → D25 CONFIRMED for 3.3a): now that multi-frame identity
  (3.2a–3.2d) is done end to end, how is the Phase 3.3 benchmark scoped so it ships
  incrementally? Answer: decompose into five sub-items, build order = dependency
  order — **3.3a HAR recorder** (hermetic, no WebArena dep, on the eval critical
  path, lands first) → 3.3b task-runner + `agent_response.json` emitter → 3.3c
  re-grounding-calls instrumentation (headline) → 3.3d dual real-peer baseline →
  3.3e report over the 258-task subset. The WebArena-Verified agent contract was
  verified this run (INPUT `{task_id, intent_template_id, sites, start_urls, intent}`;
  OUTPUT `agent_response.json {task_type, status, retrieved_data, error_details}` +
  `network.har`; EVAL `wa.evaluate_task(...) → result.score`), and chromiumoxide_cdp
  0.9.1 exposes all `Network.*` events 3.3a needs (no fork). OPEN for the builder:
  confirm D25 by shipping 3.3a.
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
- RESOLVED (research run 14 → D23): now that 3.2b (channel + join) is shipped, how
  is the remaining OOPIF work shaped? Answer: split it into 3.2c (observe) then 3.2d
  (dispatch). `auto_attach_children`/`run_on` are inherent to `RawCdpSession`
  (`channel.rs:149`/`:225`), not on the `CdpChannel` trait (`:82`), so the generic
  `raw_pass` (`observer.rs:184`) cannot fold OOPIF nodes until both are promoted to
  the trait with no-op defaults (`Page` inherits the page fast path byte-identical).
  3.2c is that promotion + a per-child `getDocument`/`getFullAXTree` fold. 3.2d is
  separate and larger: `actions.rs` is entirely `chromiumoxide::Page`-typed
  (`:112`–`:271`, zero `CdpChannel`/`run_on` refs), so routing an OOPIF eid to its
  owning session first requires channelizing the whole action surface. D23
  CONFIRMED (builder run 15): 3.2c shipped exactly to this shape, with one
  refinement — `observe` fuses each session's pass **independently** and
  concatenates rather than remapping child backend ids into a disjoint range
  (the floated D23 idea), because the core already keys `by_backend` on
  `(FrameKey, BackendNodeId)`, so per-session fusion sidesteps both the
  `backendNodeId` and the `AXNodeId` cross-target collision with zero remapping.
- RESOLVED + SHIPPED (builder run 16 → D24 corrected): the phantom "0" frame-key
  (the sole cross-origin OOPIF keyed frame "1" not "0"). The research-run-15 root
  cause (the phantom is the main frame's `#document` nodeType 9; fix = `node_type==1`
  guard) was **falsified live this run** — a direct CDP dump
  (`getDocument{pierce}` + `getFrameTree`) showed exactly two frame-id carriers,
  **both nodeType 1 elements**: the main frame's `<html>` document element (carrying
  the frame's own id) and the real `<iframe>`. CDP stamps `frameId` on the `<html>`
  document element of each frame, not on a `#document` node, so nodeType cannot tell
  the phantom from a real owner. Shipped fix: replaced `node_type: i64` with
  `node_name: String` on `DomNode`, populate from `node.node_name` in
  `decode_dom_node`, gate the owner branch on `is_frame_owner_element(&child.node_name)`
  (case-insensitive `iframe`/`frame`). Two regression tests model the `<html>`
  phantom (`html_doc_element`). Live `observe_oopif`: OOPIF keys `f0/btn-buy-now`
  (was `f1/`), rebinds across the swap, exit 0; example asserts `starts_with("f0/")`.
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
