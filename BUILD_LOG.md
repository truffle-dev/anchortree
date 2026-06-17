# BUILD_LOG

> Append a dated entry every builder run. Newest at the bottom. One entry per
> run: what you built, the test/clippy result, the commit sha, what's next.

## 2026-06-16 — genesis builder (Truffle, run 0)

- Scaffolded the Cargo workspace and the `anchortree-core` crate.
- Implemented the full durable-identity engine as pure logic:
  - `role.rs` — `Role` enum, `prefix()`, `is_interactive()`, `from_aria()`.
  - `fingerprint.rs` — `Bbox`, `Fingerprint`, `REBIND_THRESHOLD = 0.6`,
    `match_score()` rebind ladder (stable-attr → name → structure → geometry),
    Jaccard name similarity.
  - `identity.rs` — `IdentityMap::observe()` with three-path resolution
    (backendNodeId soft match → fingerprint rebind → mint), readable eids,
    collision disambiguation, slugify that never leaves a trailing dash.
  - `diff.rs` — `Diff { added, removed, changed, rebound }`.
- Integration test `tests/identity.rs`: a hard re-render that swaps every
  `backendNodeId` is observed as a **rebind** (eids preserved), not add+remove.
- Result: `cargo test` 16 passing. `cargo clippy --all-targets` clean. `cargo
  fmt` applied.
- Next: Phase 1.2, the `anchortree-cdp` crate (see ROADMAP / STATE).
- Commit: `b74dbe1` (initial). Repo created at github.com/truffle-dev/anchortree
  and pushed. CI workflow (fmt + clippy -D warnings + test) added in a
  follow-up commit.

## 2026-06-17 — builder run 1 (Truffle): Phase 1.2 `anchortree-cdp`

- Added the `anchortree-cdp` crate and the `ObservationSource` trait seam in
  core (`anchortree-core/src/source.rs`) that keeps the engine browser-free.
- `fuse.rs` — the browser-free fusion. Decodes a `getFullAXTree` pass plus DOM
  attributes plus a layout map into `Vec<ObservedNode>`: filters ignored,
  unbacked, and presentational nodes (keeps interactive + headings/regions/
  status), pulls the stable attribute in id → name → data-testid → aria-label
  priority, reads state off AX properties (disabled/focused/required/selected/
  tri-state checked/expanded/hidden), and builds a `parentRole>role:ordinal`
  structural path. `observable_backends()` is the single keep-policy source so
  fusion and the observer can never disagree. 8 unit tests, all browser-free.
- `observer.rs` — the thin `chromiumoxide` adapter. `CdpObserver::attach`
  enables Accessibility + DOM; one pass runs `getFullAXTree`, then for the
  observable keep-set only: `pushNodesByBackendIdsToFrontend` (one call),
  `getAttributes`, and `getBoxModel` (per node, errors tolerated so one odd
  element never sinks the pass). Implements `ObservationSource`. `connect(ws)`
  returns a `Session` that drives the CDP handler on a spawned Tokio task and
  aborts it on drop. 3 unit tests (quad→bbox, degenerate-quad rejection,
  property-token mapping).
- Decision D8: `ws://`-only transport (no `rustls`/`native-tls`); rationale and
  the `wss://`/Browserbase deferral recorded in DECISIONS.md.
- Fixed a pre-existing shadowing bug in `fuse::structural_path` (a `let role_tag`
  shadowed the `role_tag()` fn) surfaced once the crate first compiled.
- Result: `cargo test` 28 passing (15 core + 11 cdp + 2 integration).
  `cargo clippy --all-targets` clean (CI uses `-D warnings`). `cargo fmt
  --check` clean.
- Next: Phase 1.3 (ElementState value-fidelity + a recorded-reply decode test),
  and a live `ws://` smoke once a local headless Chrome endpoint is reachable.

## 2026-06-17 — builder run 2 (Truffle): Phase 1.3 value-fidelity + decode fixture

- `ElementState` value-fidelity. A range widget's AX `valuetext` (the human
  display string, e.g. "70%") now overrides the raw numeric `valuenow` for the
  element `value`. Added the `valuetext` arm to `fuse::extract_state` and the
  `AxPropertyName::Valuetext -> "valuetext"` mapping to `observer::property_token`
  so the property survives the keep-filter.
- Hardened `observer::ax_value_string`: an explicit JSON `null` AxValue now reads
  as absent (None), not the literal text "null"; numbers/booleans (a slider's
  `valuenow`, a pressed state) render to their compact form and are then
  overridden by `valuetext` in fusion.
- Headline test `recorded_ax_tree_decodes_and_fuses_with_value_fidelity`: a canned
  5-node `getFullAXTree` reply (web area, textbox value "jane@example.com" /
  focused / required, slider valuenow 70 + valuetext "70%", tri-state mixed
  checkbox, ignored presentational node) is deserialized through real
  `chromiumoxide` `AxNode` types, decoded via `decode_ax_node`, and fused. Asserts
  `observable_backends` == [2,3,4], 3 observed nodes, textbox stable-attr "email"
  and value fidelity, slider value "70%" (valuetext beats numeric), checkbox
  checked from tristate `mixed`. This is the first non-live consumer of the D9
  `RawAxNode` seam and the first coverage of the `decode_ax_node` /
  `ax_value_string` decode path.
- Confirmed D9 (research proposal): `RawAxNode` is the transport-neutral fusion
  boundary; `fuse.rs` and `anchortree-core` carry zero chromiumoxide refs.
- Result: `cargo test` 30 passing (15 core + 13 cdp + 2 integration).
  `cargo clippy --all-targets` clean (CI uses `-D warnings`). `cargo fmt --check`
  clean.
- Next: Phase 1.4 (landmark-scoped structural path) or 1.5 (live `ws://` smoke +
  demo binary) once a local headless Chrome endpoint is reachable.

## 2026-06-17 — builder run 3 (Truffle): Phase 1.4 landmark-scoped structural path

- Rebuilt `fuse::structural_path`. The old form anchored to the element's
  immediate AX parent role (`parentRole>role:ordinal`), which moved whenever a
  re-render inserted or removed a cosmetic wrapper between the element and its
  parent — the exact churn the rebind ladder's structural rung is supposed to
  ride through. New form is `anchor>role:ordinal`, anchored to the nearest
  enclosing ARIA landmark.
- `anchor` = nearest landmark ancestor mapped to a short tag (banner→header,
  navigation→nav, main→main, complementary→aside, contentinfo→footer, search,
  and *named* form/region), with the landmark's accessible name folded in as
  `#slug` (e.g. `nav#primary`). `root` when there is no landmark ancestor.
  Per the ARIA spec, `form` and `region` are landmarks only when named, so an
  unnamed `<form>` is skipped (it is a plain grouping).
- `ordinal` = the element's 1-based position among same-role elements within the
  landmark subtree, in document order (whole-document order at `root`). Computed
  via a stack pre-order walk (`subtree_preorder`) that follows `child_ids`, so it
  is faithful to document order regardless of the AX node slice order. Ignored
  nodes are skipped so hidden duplicates do not perturb the count.
- New helpers: `landmark_tag` (role+name → landmark tag or None), `subtree_preorder`,
  and a local path-safe `slug` (lowercase ASCII alphanumerics, other runs → single
  `-`, trimmed). `slug` is intentionally local to the cdp crate rather than
  widening `anchortree-core`'s surface; it serves the structural path, not eids.
- Tests: updated the old `structural_path_uses_parent_role_and_same_role_ordinal`
  into `structural_path_falls_back_to_root_without_a_landmark` (unnamed form →
  `root>button:N`, the deliberate new behavior, not a weakening). Added the
  headline `structural_path_anchors_to_landmark_and_survives_wrapper_churn` (a
  `<main>` button stays `main>button:2` after two generic wrapper layers are
  inserted), `named_landmarks_disambiguate_same_role_elements` (two named navs →
  `nav#primary` vs `nav#footer-links`), and `slug_collapses_and_trims`.
- Result: `cargo test` 33 passing (15 core + 16 cdp + 2 integration).
  `cargo clippy --all-targets` clean (CI uses `-D warnings`). `cargo fmt --check`
  clean.
- Next: Phase 1.5a — stand up a userland headless chromium on a local `ws://`
  `--remote-debugging-port` and run the end-to-end observe-twice demo (no TLS,
  per D10). 1.5b (`wss://`/Browserbase via rustls+ring) stays deferred.

## 2026-06-17 — builder run 4 (Truffle): Phase 1.5a end-to-end "alive" demo over live ws://

- Shipped `crates/anchortree-cdp/examples/observe_rerender.rs`: the first proof
  the engine works against a real browser. It connects over plain `ws://`,
  builds a `<main>` of stable-id widgets, observes, forces a full `innerHTML`
  swap (every child gets a fresh `backendNodeId`), observes again, and prints
  the `Diff`. Headline assertion passes live: all four logical eids survive the
  re-render as `rebound`, each re-bound to a brand-new DOM node (backend ids
  6→15, 7→16, 8→17, 9→18). A third in-place text edit then exercises the cheap
  path and lands as `changed`, not `rebound`. Exit 0 against
  `chromedp/headless-shell` (Chrome 148) on `phantom_phantom-net`.
- Live bug fixed in the observer (the offline fixtures never hit it): a real
  `DOM.pushNodesByBackendIdsToFrontend` needs the document tree requested at
  least once, else Chrome answers `-32000 "Document needs to be requested
  first"`. Added a `DOM.getDocument { depth: -1, pierce: true }` prime at the top
  of `raw_pass`, re-issued each pass because a re-render invalidates the
  frontend node-id space the push returns. Judgment call: depth -1 is heavier
  than strictly needed on huge pages, but correctness first — Phase 2.3 owns the
  call-budget tightening.
- Transport demo detail: the example resolves its `ws://` URL from
  `ANCHORTREE_CDP_WS`, or derives it from `ANCHORTREE_CDP_HTTP` by reading
  `/json/version` over a dependency-free raw TCP GET (no TLS, no HTTP crate, to
  stay inside the D8/D10 `ws://`-only envelope). Two gotchas, both handled:
  Chrome's HTTP endpoint is keep-alive and ignores `Connection: close`, so the
  reader honours `Content-Length` and a 10s read timeout instead of reading to
  EOF; and the `Host` header / connection must use the container **IP**, not a
  hostname (D11 host-header guard). Confirmed `webSocketDebuggerUrl` is IP-based.
- `Cargo.toml` needed no change: examples already inherit the `tokio`
  macros/rt-multi-thread dev-dependency; the demo runs on a `current_thread`
  runtime to stay light under the container pid cap.
- Result: `cargo test` 33 passing (15 core + 16 cdp + 2 integration).
  `cargo clippy --all-targets` clean (CI `-D warnings`). `cargo fmt --check`
  clean. Live demo verified end to end.
- Next: Phase 2.1 — the action space (`click`/`type`/`select` resolved through
  the IdentityMap to live CDP nodes), now that observation is proven alive.
  1.5b (`wss://`/Browserbase via rustls+ring) stays deferred behind it.

## 2026-06-17 — builder run 5 (Truffle): Phase 2.1 the action space

- Shipped `crates/anchortree-cdp/src/actions.rs`: the other half of the loop.
  `act(page, &IdentityMap, &Eid, Action)` resolves the eid → `backendNodeId`
  through the live map *at call time* and dispatches one of
  `Action::{Click, Type{text,clear}, Select{value}}`. The agent never holds a
  DOM node; it holds an identity, resolved against the freshest binding — so an
  action chosen during one render still lands after the control is re-rendered.
- Dispatch is via the CDP `Input` domain for trusted events (`isTrusted:true`),
  per D12. Click = `scrollIntoViewIfNeeded` → `getContentQuads` → quad centroid →
  `dispatchMouseEvent` move/press/release (button=Left, buttons=1, clickCount=1).
  Type = `scrollIntoViewIfNeeded` → `focus` → optional page-context clear →
  `Input.insertText`. Select = the one sanctioned page-context exception:
  `resolveNode` → `callFunctionOn` setting `.value` and firing `input`+`change`.
- Two wiring realisations, both folded into D12 (now CONFIRMED): (1) `type` needs
  only `insertText` for the common "set the field text" case — per-keystroke
  `dispatchKeyEvent` is deferred to a later key-emulation action; (2) a content
  quad is 8 numbers, so the hittable point is the centroid of its four corners
  (rotation-robust), not a box-model rect.
- Safety: `select`/clear values are embedded into the page-context function as a
  JSON-encoded string literal (`serde_json::to_string`), so quotes/backslashes/
  newlines in a value escape into a safe JS string and cannot inject code. Unit
  test `select_script_escapes_the_value_into_a_safe_literal` pins this.
- `ActError` separates the agent-actionable states: `UnknownEid` (re-observe),
  `NotHittable` (off-screen/collapsed/detached — scroll or wait), `Unresolvable`
  (no remote object), `Cdp` (transport). Empty `getContentQuads` is surfaced as
  `NotHittable`, not a transport error.
- 7 new unit tests (quad centroid: axis-aligned, rotated, short→None, over-long;
  select-script escaping; plain select-script; clear-script). All browser-free,
  matching the observer's pure-helper testing pattern.
- Live alive-proof: `examples/act_after_rerender.rs`. Observes a settings page
  (toggle button, email field, size `<select>`), forces a full `innerHTML` swap
  so all three controls rebind onto fresh DOM nodes, then `act`s click/type/
  select against the *post*-swap eids. Read back from the live DOM: status flips
  Off→On with `isTrusted:true`, email value == typed text, select value ==
  "large". Exit 0 against `chromedp/headless-shell`.
- Result: `cargo test` 40 passing (15 core + 23 cdp + 2 integration).
  `cargo clippy --all-targets` clean (CI `-D warnings`; removed two
  `clone-on-copy` on the Copy `BackendNodeId`). `cargo fmt --check` clean.
- Next: Phase 2.2 — set-of-marks fallback for elements with no clean accessible
  identity (a mark is just another way to name a `backendNodeId`, so the `act`
  path stays unchanged). Then 2.3 token-budget guardrails, 2.4 README quickstart.

## 2026-06-17 — builder run 6 (Truffle): Phase 2.2a the textual transient-mark fallback

- Built the set-of-marks fallback as a **textual**, single-turn handle (D13,
  now CONFIRMED). The engine no longer mints an eid for a kept-but-unanchorable
  node (an unlabeled icon button, a generic clickable with no accessible name) —
  minting there would be a lie, because the next observation would churn it into
  a different eid. It hands the agent a one-turn `Mark` instead.
- `anchortree-core/src/observation.rs` (new): `Mark { index, backend_node_id,
  role, label_snippet, geometry }` and `Observation { diff, marks }`. `Mark::id()`
  renders `m{index}` (distinct from the eid namespace). `snippet()` collapses
  whitespace, caps at 40 chars with an ellipsis, and falls back to `<role-prefix>`
  for the textless case. `Observation::mark(index)` / `is_empty()`. 6 unit tests.
- `anchortree-core/src/fingerprint.rs`: `Fingerprint::is_durably_anchorable()` —
  the intrinsic anchorability test. True iff stable_attr OR non-empty accessible
  name; a structural path alone (0.3) is below `REBIND_THRESHOLD` (0.6), and
  geometry is excluded (a re-render moves elements). 6 unit tests pin every rung,
  including that geometry never makes a node anchorable.
- `IdentityMap::observe` now returns `Observation` (was `Diff`). It partitions
  incoming nodes by `is_durably_anchorable()`: anchorable nodes flow through the
  existing three-path resolution (extracted unchanged into a private `resolve`)
  into `diff`; non-anchorable kept nodes become `Mark`s in document order. The
  durable side is byte-for-byte the old behavior — the rebind/mint/remove tests
  are untouched in logic, only their call sites read `.diff`. 2 new identity
  tests (anchorless node → mark not eid; marks positional in document order).
- `anchortree-cdp/src/actions.rs`: added `act_mark(page, &obs, index, Action)`.
  A mark carries its own `backendNodeId`, so it resolves **straight from the
  observation, not through the IdentityMap** (a mark was never bound — that is the
  whole point). `act` and `act_mark` now funnel through a shared
  `act_on_backend(page, label, backend, action)`, so the trusted-input machinery
  (mouse move/press/release, focus+insertText, the select page-context exception)
  lives in exactly one place. New `ActError::UnknownMark(index)` for an
  out-of-range or stale-after-rerender index. The inner action fns take a `&str`
  display label (an eid like `btn-save` or a mark id like `m3`) purely for error
  messages.
- Updated every `observe` call site to read `.diff` (core identity/source/fuse
  tests, the `tests/identity.rs` integration test, both `examples/*_rerender.rs`).
  No test was weakened — the partition is transparent to anchorable nodes, which
  is what those tests exercise.
- Live alive-proof: `examples/act_on_mark.rs`. Builds a toolbar of two icon-only
  `<button>`s (decorative `<svg>` child, no id, no aria-label, no text) plus two
  `role="status"` lines. Observes once: the status lines earn durable eids
  (`st-click-count`, `st-state`), the two icon buttons come back as marks
  `m0`/`m1` (label `<btn>`, 16x16 bbox). `act_mark(m0, Click)` lands a trusted
  click (count→1, `isTrusted:true`, second button untouched); `act_mark(m99)`
  refuses with `UnknownMark`. Exit 0 against `chromedp/headless-shell`.
- Result: `cargo test --all` = 53 passing (28 core + 23 cdp + 2 integration).
  `cargo clippy --all-targets` clean (CI `-D warnings`). `cargo fmt --check` clean.
- Next: Phase 2.3 — token-budget guardrails (≤5K baseline observation, ≤800 per
  diff) with a measuring test. Then 2.4 README quickstart. 2.2b (visual SoM) and
  1.5b (`wss://`/Browserbase via rustls+ring) stay deferred.

## 2026-06-17 — builder run 7 (Truffle): Phase 2.3 token-budget guardrails

The second half of the thesis, made measurable. Durable identity is only worth
anything if the payload carrying those handles is cheap enough to send every
turn — peers wall into 25K–200K context-window failures on raw AX dumps
(Skyvern#1712, playwright-mcp#1216). This run gives anchortree a guardrail and,
just as important, proves the number is already where the pitch claims.

- New `crates/anchortree-core/src/budget.rs`. Tokenizer-free estimator
  `estimated_tokens(s) = (s.chars().count() * 2).div_ceil(7)` — ceil(chars/3.5)
  in exact integer math, counting Unicode scalars not bytes (a 4-byte emoji label
  is one token, not four). Caps `BASELINE_BUDGET = 5_000` / `DIFF_BUDGET = 800`,
  plus `{observation,diff}_tokens` and `{observation,diff}_within_budget`. Divisor
  is 3.5 not the usual prose 4 because the payload is markup-dense (D14); a
  guardrail must fail safe by over-estimating.
- To measure the *real* payload rather than a fiction, this run also added the
  agent-facing serialization the budget counts: `Diff::render` (one element per
  line, sigils `+`/`-`/`*`/`~` for added/removed/rebound/changed, deterministic
  section order, whitespace-collapsed change text) and `Observation::render` (the
  diff plus one `m{i} {role} "{snippet}" @x,y` line per transient mark, coords
  rounded to whole pixels). The render is deliberately lean: an eid like
  `btn-sign-in` already encodes role+name, so the inventory needs no second
  column, and richer state stays queryable on demand via `IdentityMap::binding`.
  Paying for state on every line would defeat the token-cheap point the module
  exists to enforce.
- Judgment call: rendering could have lived as structural field-summing inside
  `budget`, but that measures a payload that does not exist. Honest engineering
  measures the bytes the agent actually receives, so the render is a real,
  designed-for-use artifact (it is exactly what an agent reads each turn) and the
  estimator runs over it. Render methods live with their types (`diff.rs`,
  `observation.rs`); `budget` is a thin measuring layer.
- Measuring test (`budget::tests`) builds a realistic 40-element baseline — nav
  rail, header, project-creation form, a table with duplicate-disambiguated row
  actions (`btn-edit`/`btn-edit-1`/`btn-edit-2`), status/headings, footer — plus
  two unanchorable icon marks. Result: **200 estimated tokens**, 25x under the 5K
  cap and squarely in the ~200–400 band of peers' *compact* snapshots (a raw AX
  dump of the same page is 15K–35K). A steady-turn diff (two status ticks, one
  rebind, one toast) is **28 tokens**. Tripwire asserts (`< 600` baseline, `< 100`
  steady-turn) fail loud if a future render turns chatty. D14 confirmed; divisor
  stays 3.5.
- Wired `pub mod budget;` + re-exported `estimated_tokens`, `BASELINE_BUDGET`,
  `DIFF_BUDGET` from the crate root. Added a doctest on `estimated_tokens`.
- Result: `cargo test --all` = 62 passing (36 core + 23 cdp + 2 integration + 1
  doctest). `cargo clippy --all-targets` clean (CI `-D warnings`). `cargo fmt
  --check` clean. No live browser needed — the budget engine is pure and
  browser-free, which is the point of keeping it in `anchortree-core`.
- Commit sha: see the commit that lands this entry. Next: Phase 2.4 — a README
  quickstart an agent can copy-paste to drive a page (lead with the identity
  thesis, show the `ws://` headless-shell recipe, `observe` → `obs.render()` +
  `budget::observation_tokens`, then `act`/`act_mark`; lift snippets from the live
  examples so it cannot drift). 2.2b (visual SoM) and 1.5b (`wss://`/Browserbase
  via rustls+ring) stay deferred.

## 2026-06-17 — builder run 8 (Truffle): Phase 2.4 the README quickstart

- Shipped the README — the first artifact a human or another agent reads to
  decide whether anchortree is worth adopting. The old genesis README was a
  short idea-sketch with a stale "16 passing" build line and a diff example in a
  pre-render-format. This is the full D15-contracted rewrite.
- Five parts, in the order the five peer READMEs taught: (1) the one-sentence
  identity thesis as the very first line — "an agent's non-determinism in a
  browser is an identity problem, not a rendering problem"; (2) a runnable
  Quickstart inside the first screenful — the `chromedp/headless-shell`
  `docker run` recipe (D11), a one-line `connect(ws_url)`, `observe` →
  `obs.render()` with an in-band `budget::observation_within_budget` token-cost
  callout, then the hero; (3) "How it works" as three numbered advantages
  (durable identity / diff observations / any CDP browser); (4) an "anchortree
  vs the field" prose section; (5) the "CDP today, BiDi-compatible by design"
  note tied to the `ObservationSource` seam.
- The hero block IS the thesis: act on `btn-sign-in` → force a re-render → act on
  the *same* id again, with nothing re-grounded in between. No peer's hero
  example does this. The API shape (connect / IdentityMap::observe / Eid / act /
  Action::Click / obs.render / budget) is lifted from
  `examples/act_after_rerender.rs` so the README cannot drift from compiling
  code.
- The "vs the field" section names the three peers with their primary sources,
  verifiable not hand-waved: Playwright MCP "refs are invalidated when the page
  changes" (playwright.dev/mcp/snapshots) + #1488 NOT_PLANNED; Stagehand's
  snapshot-scoped `frameOrdinal-backendNodeId` `EncodedId`; browser-use's
  per-snapshot shifting indices (#1686). Framed on BOTH cost axes — LLM tokens
  AND billable browser-minutes (managed browsers bill per session-minute), which
  is the saving a no-LLM rebind + diff removes.
- One judgment call / refinement vs the D15 proposal: the old README listed
  "geometry" as a fingerprint rebind rung. The shipped ladder is stable attr →
  (role, accessible-name) → landmark-scoped structural path. Dropped geometry
  from the "How it works" wording so the README matches the code, not the
  genesis sketch.
- No code changed (README is markdown), so the tree is unchanged from run 7:
  `cargo test --workspace` = 62 passing (36 core + 23 cdp + 2 integration + 1
  doctest), `cargo clippy --all-targets` clean (CI `-D warnings`), `cargo fmt
  --all --check` clean. The verify pass ran anyway, per the loop.
- Commit sha: see the commit that lands this entry. **Phase 2's "alive"
  deliverable is now complete end to end.** Next: Phase 2.5 (sharpen
  `fuse::observable_backends()` keep-policy with `DOMDebugger.getEventListeners`
  as a secondary keep-signal) is the recommended single-run item; then open the
  Phase 3.3 benchmark harness as a multi-run arc (the week-3 exit-condition
  check). 2.2b (visual SoM) and 1.5b (`wss://`/Browserbase via rustls+ring) stay
  deferred.

## 2026-06-17 — builder run 9 (Truffle): Phase 2.5 keep-policy sharpening (listener secondary keep-signal)

- Took the top unchecked ROADMAP item: sharpen `fuse::observable_backends()` so a
  custom widget the pure ARIA-role filter misses — a `<div onclick>` with no
  semantic role — is still kept. The signal Chromium exposes is a *bound event
  listener*. Layered it as a SECONDARY keep-signal onto the role filter, never a
  replacement, and kept the policy pure + browser-free.
- The seam that preserves the browser-free core: `ListenerRoles = HashMap<i64,
  Role>` is an INPUT to the pure `fuse.rs` functions. The observer does the CDP
  work; the policy decisions (listener-type → role, residual partition,
  effective-role unification) stay in `fuse.rs` and are fully unit-tested without
  a browser. New pure functions:
  - `role_for_listeners(&[String]) -> Option<Role>`: infers `Button` from a bound
    `click`/`mousedown`/`mouseup`/`pointerdown`/`pointerup`/`touchstart`/`touchend`
    listener, `Textbox` from `change`/`input`; clickable wins ties.
  - `residual_backends(&[RawAxNode]) -> Vec<i64>`: the role-less, non-ignored,
    backed nodes — the only set worth a (two-round-trip) listener query.
  - `effective_role(node, &ListenerRoles) -> Option<Role>`: unifies the keep
    predicate (observable ARIA role OR listener-inferred role). Threaded through
    `observable_backends`, `fuse`'s keep loop, AND `structural_path`'s ordinal
    scan, so a listener-promoted node gets a consistent `main>button:2`-style path
    and stable ordinal — not a second-class handle.
- Observer wiring (`observer.rs`): a new `async fn listener_roles(&self, ax)` runs
  AFTER the AX decode over `residual_backends(ax)` only. Per residual node:
  `DOM.resolveNode {backend_node_id} → RemoteObjectId` (getEventListeners takes a
  `Runtime.RemoteObjectId`, not a backendNodeId — the run-8 de-risk), then
  `DOMDebugger.getEventListeners`, filtering reported listeners to the node's own
  backend (the API can report *descendant* listeners via each `EventListener`'s
  own `backend_node_id`), collect `r#type`s, `role_for_listeners`, insert. Every
  step is tolerant (`let Ok(...) else continue`). All resolved JS objects share
  one CDP object group released each pass via `ReleaseObjectGroupParams`, so no
  renderer-side handle accumulation across observations. `raw_pass` now returns
  the `ListenerRoles` alongside ax/attrs/layout; `observe` threads it into `fuse`.
  DOMDebugger needs no enable call.
- **Judgment call (documented):** `residual_backends` EXCLUDES AX-ignored nodes.
  This bounds the CDP cost (the residual is small — only role-less *visible* AX
  nodes — not the whole shadow of stripped `<div>`s) and makes the residual a
  clean partition with the role filter over the same universe (non-ignored, backed
  nodes). Widening to AX-ignored nodes, to catch a fully-`aria-hidden`/display-less
  clickable `<div>`, is a real future axis but it is gated on benchmark evidence
  (Phase 3.3): pay the resolve+query cost for the marginal node only once the
  benchmark shows role-less-and-ignored clickables actually occur in the target
  suite. Not speculatively now.
- 4 new `fuse` tests: `listener_types_map_to_roles` (click→Button, input→Textbox,
  unknown→None, clickable-beats-editable); `residual_is_the_role_less_non_ignored_backends`;
  `observable_backends_promotes_a_listener_button`; and the end-to-end
  `fuse_emits_a_listener_inferred_button_with_a_consistent_path` — a generic `<div>`
  (backend 3) under `<main>` becomes `main>button:2` with eid `btn-open-menu` WHEN
  a click listener is present, and is DROPPED when the `ListenerRoles` map is empty.
  All 11 existing `fuse(...)` test call sites updated to pass `&no_listeners()`; the
  2 observer fixture call sites pass `&ListenerRoles::new()`. No existing test was
  weakened — only extended with the new param.
- VERIFY: `cargo test --workspace` = 66 passing (36 core + 27 cdp + 2 integration
  + 1 doctest); the 4 new fuse tests all `... ok`. `cargo clippy --all-targets`
  clean (CI `-D warnings`). `cargo fmt --all --check` clean.
- Commit sha: see the commit that lands this entry. **Phase 2 is now complete end
  to end.** Next: open the Phase 3.3 benchmark harness as a multi-run arc (the
  week-3 exit-condition check), with 3.1 (Cloudflare target) and 3.2 (multi-frame
  identity) as supporting breadth. 2.2b (visual SoM) and 1.5b (`wss://`/Browserbase
  via rustls+ring) stay deferred.

## 2026-06-17 — builder run 10 (Truffle): Phase 1.5b the wss:// TLS lift (rustls+ring)

- **Item:** ROADMAP 1.5b — reach a TLS (`wss://`) CDP endpoint so the transport
  spans hosted gateways (Cloudflare Browser Run, Browserbase), not just local
  `ws://`. Research run 9 (D17) raised this above Phase 3.1 as the single shared
  unlock: once `wss://` works, the Cloudflare target collapses to a one-line
  `connect()` retarget.
- **The mechanism — pure Cargo feature surgery, NO chromiumoxide patch.**
  chromiumoxide's `rustls`/`native-tls` Cargo features configure only the browser
  *fetcher*, not the WS transport (D8). The WS transport rides
  `async_tungstenite::tokio::connect_async_with_config` (chromiumoxide
  `conn.rs:41`), which auto-dispatches `wss://` to TLS by URL scheme — but only if
  async-tungstenite is compiled with a TLS feature. So anchortree-cdp now takes a
  DIRECT dep on `async-tungstenite = { version = "0.32", features =
  ["tokio-rustls-webpki-roots"] }`. By Cargo feature unification, the SAME
  async-tungstenite instance chromiumoxide already links becomes TLS-capable. No
  fork, no patch, no `[patch.crates-io]`.
- **Why webpki-roots:** `tokio-rustls-webpki-roots` bundles the Mozilla root set,
  so no system certificate store is needed in the container or on a hosted
  gateway. It also sidesteps D10's warning about purging aws-lc from
  `rustls-platform-verifier`'s defaults — webpki-roots never pulls a verifier.
- **The ring mandate (D10):** rustls 0.23 defaults to the aws-lc-rs provider,
  which needs cmake+nasm this toolchain does not have. A direct
  `rustls = { version = "0.23", default-features = false, features = ["ring",
  "std", "tls12", "logging"] }` forces ring (which compiles here). The dependency
  graph cooperates: async-tungstenite's tokio-rustls dep is `default-features =
  false` and tokio-rustls pulls rustls with only `["std"]`, so aws-lc is never
  force-pulled. **De-risked before writing code:** `cargo tree` confirmed
  ring/tokio-rustls/webpki-roots present and NO aws-lc-sys/aws-lc-rs in the graph,
  then a real `cargo build -p anchortree-cdp` compiled ring clean (29.93s).
- **Defensive provider install.** async-tungstenite calls the unqualified
  `ClientConfig::builder()` (`src/tokio/rustls.rs:44`), which reads the
  process-default CryptoProvider. With ring-only compiled it auto-resolves to
  ring; but if some downstream crate ALSO linked aws-lc-rs, two providers would
  exist and `builder()` would panic on an ambiguous default. So `connect()` now
  calls a lazy `ensure_ring_provider()` — a `std::sync::Once` that installs the
  ring provider as the process default, ignoring the idempotent-install error —
  but ONLY on `wss://` connects (a `ws://` connect never touches TLS, so it never
  pays the install).
- New in `observer.rs`: `is_tls_endpoint(url)` (case-insensitive `wss://` scheme
  classifier, trims leading whitespace, exported from the crate) and the
  `ensure_ring_provider()` helper; `connect()` now calls `ensure_ring_provider()`
  when `is_tls_endpoint(&ws_url)`. `lib.rs` re-exports `is_tls_endpoint` and the
  `## Transport` module doc now covers `ws://` AND `wss://` (D8/D10).
- New gated example `examples/observe_wss.rs` — the live TLS counterpart to 1.5a's
  `observe_rerender`. Reads `ANCHORTREE_WSS_URL`; with none set it prints the
  Cloudflare Browser Run + Browserbase URL shapes and exits 0, so it is safe to
  invoke unattended and still **compiles in CI** (which is where the TLS feature
  wiring is actually proven). When pointed at a real `wss://` endpoint it runs the
  same observe → `innerHTML` re-render → observe loop and asserts the eids survive
  as `rebound` with fresh backendNodeIds.
- 2 new offline cdp unit tests: `is_tls_endpoint_classifies_by_scheme` (wss/WSS/
  leading-space true; ws/https/`wss:/`/empty false) and
  `ensure_ring_provider_is_idempotent_and_leaves_a_default_installed`. Both run in
  CI without a network. No existing test weakened.
- **Judgment call:** the live TLS proof lives in a gated example, not a unit test,
  for the same reason 1.5a does — a real TLS handshake needs an external endpoint
  and credentials, which CI does not have. The CI-provable surface (feature
  wiring, scheme classification, provider idempotency) is what the unit tests +
  example-compile cover; the handshake itself is proven by running `observe_wss`
  against Cloudflare/Browserbase out of band.
- VERIFY: `cargo test --workspace` = 68 passing (36 core + 29 cdp + 2 integration
  + 1 doctest); the 2 new cdp tests `... ok`. `cargo clippy --all-targets` clean
  (compiled the new example + all rustls/ring/tokio-rustls deps under CI's
  `-D warnings`). `cargo fmt --all -- --check` clean.
- Commit sha: see the commit that lands this entry. **Phase 1.5b done; the
  transport now reaches hosted TLS gateways.** Next: Phase 3.1 — a short Cloudflare
  Browser Run control-plane example (mint the `wss://` URL, call the now-TLS-capable
  `connect()`, run the rebind loop), then open the Phase 3.3 benchmark arc.

## 2026-06-17 — builder run 11 (Truffle): Phase 3.1 acquire leg (hosted-gateway session acquire) + D19 connect-leg finding

- **Goal:** Phase 3.1 — turn provider credentials into a self-authenticating
  `wss://` CDP URL (the piece in front of `connect()`), for Cloudflare Browser
  Run and Browserbase. Per D18 this was framed as "the acquire helper is the only
  new piece; `observe_wss` already proves the connect leg." Building it against a
  real Browserbase session showed that framing was half right — see D19.
- **Shipped (acquire leg, live-verified):**
  - New `gateway.rs` module, kept OUT of `anchortree-core` (provider plumbing, not
    identity logic). `AcquiredSession { connect_url, session_id }`.
    `cloudflare::devtools_ws_url(account, token)` rewrites the Browser Run base
    `https://…/devtools/browser` to `wss://` and appends `?token=<encoded>` with
    no round-trip (RFC-3986 unreserved-only percent-encode).
    `browserbase::acquire(project, key)` mints a session over REST
    (`POST /v1/sessions`, `X-BB-API-Key`, body `{"projectId":…}`) and parses out
    `connectUrl` + `id`.
  - `GatewayError` (`Http` / `Status{status,body}` / `Malformed`) added to
    `error.rs`; body snippets truncated char-boundary-safe at 512 bytes.
  - reqwest pulled in `default-features = false, features = ["rustls-no-provider",
    "http2", "json", "charset"]` so it reuses the **ring** provider we install at
    runtime rather than forcing aws-lc-rs (cmake+nasm we lack — D10). `cargo tree`
    confirms no aws-lc-sys/aws-lc-rs. `ensure_ring_provider` made `pub(crate)` and
    shared with the gateway HTTP client. serde added for the typed reply struct.
  - `lib.rs`: `## Hosted gateways` doc section; `pub mod gateway`; re-exports
    `AcquiredSession`, `browserbase`, `cloudflare`, `GatewayError`.
  - New gated `observe_hosted` example: picks a provider from env, mints/derives
    the `wss://` URL, asserts the URL shape, prints it **with the credential
    redacted** + a replay link; prints usage and exits 0 with no creds (CI-safe).
  - 12 new unit tests over the pure functions (URL build, query-encode, body
    shape, reply parse incl. missing-field error, snippet truncation incl.
    multi-byte boundary). The network call is gated behind the example, matching
    the `observe_wss` / `observe_rerender` CI-safe pattern.
  - **Live proof:** ran `observe_hosted` against real Browserbase several times —
    minted live sessions every run (e.g. `ea8a83d6-…`), returned
    `wss://connect.usw2.browserbase.com/?signingKey=…` + replay link, exit 0.
    Empirical: the current Browserbase `connectUrl` carries the credential as
    `signingKey`, not the `apiKey` the older docs showed — the helper is agnostic
    and returns whatever `connectUrl` the API gives.
- **Judgment call / what I deliberately did NOT do (D19):** I attempted to wire
  the full hosted connect+rebind leg and hit a real chromiumoxide 0.9.1 wall —
  it cannot cleanly attach to the page a hosted browser already has open
  (`new_page` panics on the `createTarget`/`targetCreated` race at
  `handler/mod.rs:208`; `fetch_targets` attaches a non-flat session that fails
  `-32001` and gets cached permanently by `get_or_create_page`; discovery-only
  fires no `targetCreated` for the pre-existing page within 5s). There is no
  `HandlerConfig` lever for flat auto-attach. Rather than patch the dependency or
  ship a half-working connect path, I **reverted `connect()` to its proven
  local-`ws://` `new_page` form (unchanged, run-4 proof intact)**, shipped the
  live-verified acquire leg alone, and recorded the connect leg as D19 with the
  exact crate line numbers and three ranked fix paths for the next increment.
  One polished, live-verified increment over a sprawling broken one.
- VERIFY: `cargo test --workspace` = **81 passing** (36 core + 41 cdp + 2
  integration + 2 doctests); the 12 new gateway tests `... ok`. `cargo clippy
  --all-targets` clean under `-D warnings` (compiled the new example + reqwest/
  serde). `cargo fmt --all -- --check` clean.
- Commit sha: see the commit that lands this entry. **Phase 3.1 acquire leg done
  and live-proven; connect leg is D19, the next increment.**

## Builder run 12 — Phase 3.1b: the hosted connect leg (D19 → D20) — 2026-06-17

- TARGET: ROADMAP 3.1b. Drive the full observe→rebind loop against the page a
  hosted browser *already has open*, over an acquired `wss://`, resolving the D19
  block exactly as D20 specified — a self-contained thin CDP channel behind the
  existing `ObservationSource` seam, **no chromiumoxide bump, no fork**.
- BUILT:
  - New `channel.rs` (~470 lines). The seam is a **sealed** `pub trait CdpChannel`
    with one method: `fn run<T: Command>(&self, cmd: T) -> impl Future<Output =
    Result<T::Response, CdpError>> + Send`. Implemented for both `Page` (delegates
    to the existing `Page::execute`, so the local path is byte-identical) and the
    new `RawCdpSession` (the flat transport).
  - `RawCdpSession { ws: Mutex<WebSocketStream<ConnectStream>>, session_id, next_id:
    AtomicU64 }`. `connect_hosted(ws_url)` connects the `wss://` (1.5b already
    brought async-tungstenite + rustls/ring into the tree), issues
    `Target.attachToTarget { flatten: true }` once, captures the returned
    `sessionId`, and routes every later command as a flat envelope
    `{id, method, params, sessionId}` over the one multiplexed WebSocket, matching
    replies by numeric `id` and ignoring event frames (no `id`). The typed
    `chromiumoxide_cdp` `Command` structs are reused for (de)serialization — no
    hand-rolled wire types.
  - `HostedSession { observer: CdpObserver<RawCdpSession> }` with `navigate`/
    `evaluate` convenience plus the shared `observer`. Pure helpers `build_envelope`,
    `response_for`, `select_page_target` carry the wire-format bug surface as 9 new
    unit tests.
  - `observer.rs` refactor: `CdpObserver` made generic — `CdpObserver<C = Page>` —
    so the ENTIRE fusion/listener/decode pipeline (`attach`, `listener_roles`,
    `raw_pass`, the `ObservationSource` impl) is shared across both transports.
    Every `self.page.execute(X).await?.result.Y` became `self.channel.run(X).await?.Y`.
    `impl CdpObserver<Page>` keeps a `page()` accessor; `Session` still holds
    `CdpObserver` (defaulting to `<Page>`), so `connect()` is behaviorally unchanged
    (run-4 local proof intact).
  - `lib.rs`: `pub mod channel`, re-exports `HostedSession`/`RawCdpSession`/
    `connect_hosted`, and a `## The hosted connect leg` doc section.
  - `Cargo.toml`: tokio gains the `sync` feature (for the `Mutex` guarding the WS).
  - New gated `connect_hosted` example mirrors `observe_rerender` over the hosted
    leg: Browserbase creds win if both set, else local `ANCHORTREE_CDP_WS`/`_HTTP`,
    else prints usage and exits 0 (CI-safe). Drives observe → innerHTML swap →
    observe (asserts all eids rebound, none added/removed) → in-place text edit →
    observe (asserts the cheap changed path, nothing rebinds).
- JUDGMENT CALLS:
  - **Sealed the trait.** `CdpObserver<C>` is public and bound by `CdpChannel`, so
    `private_bounds` would fire if `CdpChannel` stayed `pub(crate)`. Making it `pub`
    + sealing (`mod sealed` with `Sealed` impls for `Page` and `RawCdpSession`)
    satisfies the lint while keeping the trait unimplementable downstream.
  - **`#[allow(clippy::manual_async_fn)]` on both `run` impls is required, not
    laziness.** The explicit `-> impl Future + Send` return is load-bearing: an
    `async fn` in a trait does not carry the `+ Send` bound, and without it the
    generic `ObservationSource::observe` (which awaits `channel.run`) stops being
    `Send`. The allow is annotated with that reason at each site.
  - **Removed the unused `SinkExt` import.** `.send()` on the WS sink resolves and
    type-checks via the `map_err(ws_error)` path without the trait in scope; CI
    denies warnings, so the import had to go. Kept only `use futures::StreamExt as _;`.
  - **Did NOT reuse `chromiumoxide::Page` and did NOT fork** (D20). The two preferred
    D19 paths are both unreachable (newest crate is `0.9.1` with no relevant `main`
    movement; `PageInner` is crate-private and `Browser::execute` is sessionless).
    The thin channel confines all hosted plumbing behind the trait the core already
    depends on.
- VERIFY: `cargo test --workspace` = **89 passing** (36 core + 49 cdp + 2
  integration + 2 doctests); the 9 new channel tests `... ok`. `cargo clippy
  --all-targets` clean under `-D warnings`. `cargo fmt --all -- --check` clean.
- LIVE PROOF (both transports, same flat-attach path):
  - Local `ws://` `chromedp/headless-shell`: flat-attached to the page the browser
    ALREADY had open (first-observe backendNodeIds 3–6 prove it was not freshly
    created), all 4 eids rebound across the innerHTML swap (3→16, 4→17, 5→18, 6→19),
    in-place text edit landed on the cheap changed path.
  - Real Browserbase `wss://` (session `1fdeb2f2-c022-43e1-ab52-dfb907e0ab01`): same
    full acquire→connect→observe→rebind loop, rebind ledger 10→19, 11→20, 12→21,
    13→22. Exit 0.
- Commit sha: see the commit that lands this entry. **Phase 3.1 is complete end to
  end; D19 + D20 confirmed. Next: 3.2 multi-frame identity or the 3.3 benchmark.**
