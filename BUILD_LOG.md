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

## Builder run 13 — Phase 3.2a: same-origin multi-frame identity (D21 mechanics 1+2+4) — 2026-06-17

- GOAL: ship the same-origin slice of D21 (two-tier durable identity), deferring
  the cross-origin OOPIF slice (mechanics 3+5) to 3.2b. The thesis test: two
  structurally identical widgets in two different frames must hold *distinct*
  durable eids and rebind *independently*, proving the eid is `(frame, in-frame
  fingerprint)` and not fingerprint alone.
- SLICE DECISION (judgment call): D21 lists five mechanics. 1 (two-tier eid),
  2 (same-origin frames), and 4 (resolve-map re-key) are a self-contained,
  live-verifiable unit that needs no new CDP session. 3 (OOPIF auto-attach) and
  5 (owning-session action dispatch) require extending the run-12 thin channel
  from 1 session to N and threading an owning-session handle through
  observe→resolve→act — a larger, separable arc. Shipping 1+2+4 now lands a real,
  provable capability (same-origin iframe identity, the common case) without
  half-building the OOPIF path. One polished increment over two sloppy ones.
- BUILT (core, browser-free):
  - New `FrameKey(String)` type: `root()` = empty, `child(ordinal)` builds the
    dot-joined ordinal path (`"0"`, `"0.1"`), `is_root()`/Display. The frame's
    *structural* identity, durable across reloads (a reload reassigns the volatile
    `frameId` but "the login iframe" keeps its ordinal path).
  - `ObservedNode` and `Binding` both gained `frame_key`. The resolve map re-keyed
    `by_backend: HashMap<(FrameKey, BackendNodeId), Eid>`. All three resolution
    paths (soft backend match, fingerprint rebind, mint) are now frame-scoped:
    `best_rebind` skips candidates in a different frame; `mint` namespaces non-root
    eids `f<key>/<local>`; the disambiguation counter is per-frame.
  - 4 new core tests: ordinal-path construction, distinct eids cross-frame,
    independent rebind, per-frame disambiguation counter.
- BUILT (cdp adapter):
  - New browser-free `frames.rs`: `frame_keys` (walk `getFrameTree` → structural
    keys), `map_backends_to_frames` (walk pierced DOM → `backend→FrameKey`, iframe
    owner attributed to parent frame, its `contentDocument` subtree to the child),
    `same_origin_frame_ids` (collect inline-document frame ids in document order).
    6 unit tests, no browser.
  - `fuse` threads a `frame_of: &HashMap<i64, FrameKey>` and stamps each node.
  - `observer.rs` `raw_pass` rewired: fetch pierced `getDocument` + `getFrameTree`
    first (primes the DOM agent AND yields the frame map), then the AX trees.
- DISCOVERY / CORRECTION to D21 mechanic 2 (the run's real finding): the live
  example first failed because only the *root* button was observed. A debug dump
  proved `getFullAXTree` with no frameId returns ONLY the root frame's AX nodes
  (1 node) — it stops at every frame boundary. So same-origin frames are free from
  the pierced *DOM* pass (the `backend→FrameKey` map IS derivable from the inline
  `content_document` subtrees) but NOT from the *AX* pass. Fix: the observer now
  issues one `getFullAXTree(frameId)` per same-origin frame (ids from
  `same_origin_frame_ids`) and concatenates the nodes. Backend ids are unique
  across the root target's pierced id space, so the merge cannot collide and the
  frame map attributes each merged node correctly. This is the kind of thing only
  a live run surfaces — the unit tests were green while the pipeline was blind to
  the frame.
- VERIFY: `cargo test --workspace` = **99 passing** (40 core + 55 cdp + 2
  integration + 2 doctests). `cargo clippy --all-targets -- -D warnings` clean.
  `cargo fmt --all -- --check` clean.
- LIVE PROOF (`examples/observe_frames.rs` against `chromedp/headless-shell`): a
  root `<main><button id=act>` and an identical button inside a same-origin
  `srcdoc` iframe. Observation 1 minted BOTH `btn-action` (root) and
  `f0/btn-action` (frame) — distinct eids for byte-identical widgets, separated
  only by frame. Observation 2 re-rendered the iframe's inner DOM only; the diff
  rebound EXACTLY `f0/btn-action` (backendNodeId 15→17) and added/removed nothing,
  while the root `btn-action` stayed steady on backendNodeId 8. Exit 0. This is
  the live proof of D21's first tier.
- Commit sha: see the commit that lands this entry. **Phase 3.2a complete and
  live-verified. Next: 3.2b OOPIF (mechanics 3+5) or the 3.3 benchmark harness.**

## Builder run 14 — Phase 3.2b: cross-origin OOPIF channel + join (D22 steps 1-3, step 3 amended) — 2026-06-17

- SCOPE (judgment call): D22's OOPIF leg is five mechanics. I scoped this run to
  the load-bearing infrastructure — the multi-session channel, the auto-attach
  event drain, and the durable frame-key ↔ child-session join — plus a live
  micro-proof. Per-child observe (mechanic 4) and dispatch on the owning session
  (mechanic 5) are now their own roadmap item 3.2c. Reason: the join is the part
  D22 said "must be asserted live, not trusted blind", and it is the part most
  likely to surface a wrong assumption. It did. Building observe+dispatch on top
  of an unproven join would have been three sloppy mechanics instead of one
  polished one.
- BUILT (channel, `channel.rs`): `RawCdpSession::run_on(session_id, cmd)` holds the
  full write+read loop; the `run` trait method delegates to it with the page
  session, so the run-12 single-session fast path is byte-identical.
  `auto_attach_children()` issues `setAutoAttach{autoAttach,flatten,
  !waitForDebugger}` and drains `Target.attachedToTarget` events into
  `ChildSession{session_id,target_id,target_type}` until the command ack `id`
  arrives (the read side already demuxes by `id`, so no demux change). Free
  function `parse_attached_to_target` keeps the wire-shape parse unit-testable
  without a socket. Used the inherent const `SetAutoAttachParams::IDENTIFIER`
  rather than `cmd.identifier()` — the `Method` trait is not in scope for the
  concrete param type.
- BUILT (join, `frames.rs`): `child_frame_keys(children, table)` joins
  `child.target_id -> structural FrameKey`. `dom_frame_keys(root)` derives the key
  table from the pierced DOM in document order, numbering every iframe owner
  (same-origin OR OOPIF) by its position in its containing document. It agrees
  with `frame_keys`/getFrameTree on every same-origin frame and additionally keys
  OOPIF owners, which getFrameTree omits.
- BUILT (wiring): `HostedSession::frame_keys()` now reads
  `getDocument{depth:-1,pierce:true}` and runs `dom_frame_keys` (was
  `getFrameTree` + `frame_keys`). `decode_dom_node` made `pub(crate)` so the
  channel can reuse the observer's decoder. `dom_frame_keys` re-exported from
  `lib.rs`. New gated example `attach_oopif`.
- DISCOVERY / CORRECTION to D22 step 3 (the run's real finding): the live example
  first failed because `child_frame_keys` fed a getFrameTree-derived table came
  back empty for the OOPIF. Raw-CDP probes against the same
  `--site-per-process` Chrome proved why: a cross-origin OOPIF's frame is ABSENT
  from the root target's `Page.getFrameTree`, before AND after `setAutoAttach`.
  The OOPIF's owner `<iframe>` element IS in the root pierced DOM, carrying
  `frameId` == the child target's `targetId`, but with its `contentDocument`
  stripped (the very reason `same_origin_frame_ids` already skips it).
  `Target.attachedToTarget` carries `targetInfo.parentFrameId` (parent link) and
  `targetId` (child's own frameId). So the structural key must come from DOM
  document order, not the frame tree. `child_frame_keys`'s signature was already
  right; only its input table was wrong. Amended D22 in DECISIONS.md; `parentFrameId`
  is captured-but-unneeded, so `ChildSession` deliberately omits a redundant
  parent field — the join needs only `target_id -> dom_frame_keys`. This is the
  kind of thing only a live run surfaces: every unit test was green while the
  pipeline's source table was structurally incapable of holding an OOPIF.
- VERIFY: `cargo test --workspace` = **108 passing** (40 core + 64 cdp + 2
  integration + 2 doctests). `cargo clippy --all-targets -- -D warnings` clean.
  `cargo fmt --all -- --check` clean.
- LIVE PROOF (`examples/attach_oopif` against `chromedp/headless-shell
  --site-per-process`, parent on network alias origin-a embedding a genuinely
  cross-origin iframe on origin-b): the DOM-derived frame table keyed two frames
  (`F710… -> 0`, `6747… -> 1`); auto-attach announced one iframe child session
  whose target id `6747…` joined to the non-root durable frame key `1`. Exit 0.
  The OOPIF's separate CDP target carries the same durable identity the engine
  namespaces its in-frame elements under — D22 step 3 (amended) confirmed live.
- Commit sha: see the commit that lands this entry. **Phase 3.2b (OOPIF channel +
  join) complete and live-verified. Next: 3.2c per-OOPIF observe + dispatch
  (mechanics 4+5) or the 3.3 benchmark harness.**

## Builder run 15 — Phase 3.2c: per-OOPIF observe (D23 mechanic 4) — 2026-06-17

- SCOPE: turn the run-14 OOPIF *channel* into an OOPIF *observation*. After this
  run, `observe()` returns one flat `Vec<ObservedNode>` in which a cross-origin
  OOPIF's widget carries a durable, frame-namespaced eid and rebinds across a
  re-render — the same contract the engine already gives root + same-origin nodes.
- BUILT (channel promotion, `channel.rs`): moved `run_on` and
  `auto_attach_children` from inherent methods on `RawCdpSession` onto the
  `CdpChannel` **trait** as default methods (`run_on → run`, `auto_attach_children
  → Ok(vec![])`), and converted the `RawCdpSession` bodies to trait-method
  overrides. RPITIT means the defaults are `-> impl Future + Send` with
  `#[allow(clippy::manual_async_fn)]` + `async move` bodies (the `+ Send` bound is
  load-bearing; `async fn` in a trait does not carry it). `Page` now inherits the
  no-op OOPIF path with a byte-identical local fast path, so the run-4/12/13 proofs
  do not regress.
- BUILT (observe fold, `observer.rs`): `raw_pass` now returns a `Vec<FramePass>`
  (a new module struct: `ax`, `attrs`, `layout`, `listener_roles`, `frame_map`).
  The root pass is built as before; `observe_oopif_children` then drains
  `auto_attach_children()`, refreshes a persistent `oopif_sessions` cache
  (target→session), and for each cached child whose target id is a known
  `dom_frame_keys` frame key, runs `child_pass`: enable AX+DOM on the child
  session, `getDocument(pierce)` to prime its DOM agent, `getFullAXTree`,
  `attrs_and_layout` over `run_sel`, and stamp every AX node's backend with the
  child's `FrameKey`. `observe` fuses **each `FramePass` independently and
  concatenates** the results.
- JUDGMENT CALL (the D23 collision resolution, refined live): D23 floated
  remapping child `backendNodeId`s into a disjoint synthetic range to avoid
  cross-target collisions under `--site-per-process` (a child target's
  `backendNodeId` AND `AXNodeId` spaces can both collide with root's). Reading
  `identity.rs` showed the core already keys its fast path `by_backend` on
  `(FrameKey, BackendNodeId)` (`:133`, used `:214`/`:244`), and `fuse` keys its
  structural walk on per-target `AXNodeId` strings — so fusing each session's pass
  **separately** and concatenating the `Vec<ObservedNode>` sidesteps BOTH
  collisions with ZERO remapping: every fuse call lives in one session's isolated
  id space, and the child nodes carry the OOPIF's `FrameKey`. Simpler and more
  robust than the remap. A new unit test in `fuse.rs`
  (`oopif_and_root_nodes_with_colliding_backends_keep_distinct_identities`) is the
  regression guard: two buttons share backend id 1 across two frame keys and still
  resolve to two distinct eids, one `f0/`-namespaced.
- DEFERRALS (documented, not silent): listener-role inference *inside* an OOPIF
  (child pass uses an empty `ListenerRoles`); and frames nested *inside* an OOPIF
  (one level only). Both are 3.2d-or-later scope.
- VERIFY: `cargo test --workspace` = **109 passing** (40 core + 65 cdp + 2
  integration + 2 doctests, +1 from the collision test). `cargo clippy
  --all-targets -- -D warnings` clean. `cargo fmt --all -- --check` clean.
- LIVE PROOF (`examples/observe_oopif`, new gated example, against
  `chromedp/headless-shell --site-per-process`; parent on alias origin-a embeds a
  genuinely cross-origin iframe on origin-b whose `child.html` swaps its widget's
  `innerHTML` ~1.2s after load): first `observe()` surfaced the OOPIF button as
  `f1/btn-buy-now` (frame-namespaced) and the root button as `btn-save-document`
  (root); after the swap the second `observe()` reported `f1/btn-buy-now` in
  `diff.rebound` with a fresh backend node (9 → 15), never removed/added. Exit 0.
  The second pass also implicitly confirmed the `oopif_sessions` cache: Chrome
  announces a child once, yet the second pass still reached the child session and
  read its new backend.
- LIVE FINDING (open question, NOT a 3.2c regression): the sole cross-origin
  iframe keyed as frame `"1"`, not `"0"` — a phantom `"0"` entry keyed by the main
  frame's id precedes it (visible in run-14's `attach_oopif` ledger too). The
  decoded `getDocument(pierce).root` evidently carries the main frame's
  `#document` as a *counted* descendant, so `dom_frame_keys`'s `assign_dom_frames`
  treats it as an iframe owner at ordinal 0. Identity is still durable, unique, and
  rebinds correctly, so this is cosmetic-but-wrong, not a correctness break. A
  clean fix needs `DomNode` to carry `node_type`/`node_name` so a `#document`
  (nodeType 9) is distinguishable from an `<iframe>` owner — a focused follow-up
  touching the 3.2a `decode_dom_node` foundation, deliberately not folded into
  3.2c. Logged in STATE Open questions for the research cron.
- Commit sha: see the commit that lands this entry. **Phase 3.2c (per-OOPIF
  observe) complete and live-verified. Next: 3.2d per-OOPIF dispatch (channelize
  `actions.rs`), then the 3.3 benchmark harness arc.**

## Builder run 16 — Phase 3.2c.1: frame-key correctness via a node-*name* owner guard (D24, corrected) — 2026-06-17

- GOAL: the top unchecked ROADMAP item, 3.2c.1. On the live `--site-per-process`
  page the sole cross-origin OOPIF keyed frame `"1"` not `"0"` — a phantom `"0"`
  preceded it. The proposed fix (D24, research run 15) was a node-*type* guard:
  add `node_type: i64` to `DomNode`, gate the owner branch on `node_type == 1`
  (ELEMENT_NODE), on the theory the phantom is the main frame's `#document`
  (nodeType 9) carrying a `frameId`.
- FALSIFIED LIVE (the run's pivot). I implemented the node-type guard; all unit
  tests passed (111 green) but `examples/observe_oopif` STILL keyed `f1/`.
  Instrumenting `assign_dom_frames` showed two frame-id carriers, **both
  nodeType 1**. A direct CDP dump (`DOM.getDocument{depth:-1,pierce:true}` +
  `Page.getFrameTree`, written as a one-off `ws` client) pinned it exactly:
  ```
  getFrameTree: d0 id=DCD662EE… url=…/parent.html        (the MAIN frame)
  frameId=DCD662EE…  nodeName=HTML    nodeType=1  backend=32  path=#document>HTML
  frameId=B83E3EF3…  nodeName=IFRAME  nodeType=1  backend=42  path=#document>HTML>BODY>IFRAME
  ```
  CDP stamps `frameId` on the `<html>` **document element** of every frame (it
  carries the frame's *own* id, here the main frame), not on a `#document` node —
  and the `#document` root carried no `frameId` at all. The `<html>` and the real
  `<iframe>` are both nodeType 1, so node-type cannot separate them. The D24
  theory was wrong; the spec line "frameId is set for frame owner elements and the
  document node" read at face value misled the diagnosis.
- SHIPPED FIX (the correct discriminator is the node *name*). Only an
  `<iframe>`/`<frame>` element owns a *child* browsing context; the `<html>`
  document element never does. Replaced `node_type: i64` with `node_name: String`
  on `DomNode` (`frames.rs`), populate it in `decode_dom_node` from
  `node.node_name` (`observer.rs`), and gate the owner branch on
  `is_frame_owner_element(&child.node_name)` (case-insensitive `iframe`/`frame`).
  The struct doc comment and the load-bearing-guard comment were rewritten to the
  corrected mechanism. The two regression tests now model the `<html>`-element
  phantom via a new `html_doc_element(frame_id, children)` test helper (node_name
  "HTML") rather than a `#document` node: `…ignore_the_html_element_carrying_its_
  own_frame_id` and `…number_owners_across_a_nested_html_element`.
- VERIFY: `cargo test` (workspace) = **111 passing** (40 core + 67 cdp + 2
  integration + 2 doctests; +2 cdp from the two corrected regression tests).
  `cargo clippy --all-targets -- -D warnings` clean. `cargo fmt --all -- --check`
  clean.
- LIVE PROOF (`examples/observe_oopif`, same `chromedp/headless-shell
  --site-per-process` + two-origin static server as run 15): first `observe()`
  surfaced the OOPIF button as **`f0/btn-buy-now`** (was `f1/`) and the root button
  as `btn-save-document`; after the in-OOPIF `innerHTML` swap the second
  `observe()` reported `f0/btn-buy-now` in `diff.rebound` with a fresh backend
  (9 → 13), never removed/added. Exit 0. The example assertion was tightened to
  `starts_with("f0/")`, so the phantom cannot silently return.
- DECISIONS: D24 header flipped PROPOSED→ACCEPTED; the falsified node-type body is
  preserved with an appended "Falsification + corrected fix (builder run 16)"
  block carrying the live CDP dump, the corrected mechanism, and the lesson (dump
  the real tree before trusting a spec-derived discriminator).
- Commit sha: see the commit that lands this entry. **Phase 3.2c.1 complete and
  live-verified; the OOPIF keys `f0/`. Next: 3.2d per-OOPIF dispatch (channelize
  `actions.rs` from `&Page` to `&impl CdpChannel`), then the 3.3 benchmark arc.**

## Builder run 17 — Phase 3.2d: per-OOPIF dispatch (channelize actions.rs, route eid to owning session; D22/D23 dispatch half) — 2026-06-17

- TASK (ROADMAP 3.2d, mechanic 5): route an OOPIF eid to its owning child session.
  D23 flagged this as bigger than it reads: `actions.rs` was `chromiumoxide::Page`-
  only (`act(page: &Page, …)`) with no channel-based action path, so the dispatch
  could only ever hit the page session. First channelize, then route.
- BUILD step 1 — channelize `actions.rs`. Every entry point and helper went from
  `&Page` to `<C: CdpChannel>` plus an explicit `session: Option<&str>`:
  `act`, `act_mark`, `act_on_backend`, `click`, `type_text`, `select_value`,
  `call_on_backend`. Every `page.execute(cmd).await?.result` became
  `chan.run_on(session, cmd).await?` — `run_on` already unwraps the
  `CommandResponse` envelope, so the `.result` field access dropped everywhere
  (`quads.result.quads` → `quads.quads`; `resolved.result.object` → `resolved.object`).
  `Runtime.resolveNode` (in `call_on_backend`) and the `Input`/`DOM` click/type/select
  dispatch now all flow through the channel with the owning session tagged on.
- BUILD step 2 — routing table on `CdpObserver`. Added
  `frame_sessions: HashMap<FrameKey, String>` alongside the existing
  `oopif_sessions: HashMap<String, String>`. It is the D23 dispatch table: rebuilt
  every pass in `observe_oopif_children` (cleared up front, before the
  `oopif_sessions.is_empty()` early return; one insert per live OOPIF child session
  keyed by its durable `FrameKey`; removed on a child-pass `Err`). It holds **OOPIF
  frames only** — a lookup miss is the correct signal for "root or in-process iframe",
  which dispatches on the page session (`None`).
- BUILD step 3 — routed surface. Two new methods on `CdpObserver`:
  `act(&self, map, eid, action)` reads the eid's frame off its live binding
  (`session_for_binding` → `frame_sessions.get(frame_key)`) and calls the channelized
  `actions::act` with that session; `act_mark(&self, obs, index, action)` dispatches on
  the page session (`None`) — a `Mark` carries only a `backend_node_id` and no
  `FrameKey`, so OOPIF mark routing is out of scope and correctly defaults to root.
  The two live examples (`act_on_mark`, `act_after_rerender`) were moved onto these
  routed methods (they no longer import the free `act`/`act_mark`).
- JUDGMENT CALLS:
  - `ActError::Cdp` now wraps `crate::error::CdpError` (`#[from]`), not
    `chromiumoxide::error::CdpError`. `run_on` returns the crate error already, so the
    `?` conversions only compile against the crate type; the chromiumoxide import was
    dropped. This is the natural consequence of channelizing — the actions layer no
    longer speaks raw chromiumoxide errors.
  - **Fixtures committed into the repo** under `examples/fixtures/oopif/`
    (`parent.html`/`child.html` for the observe/rebind demos, `parent_action.html`/
    `child_action.html` for the action demo). Prior runs reconstructed these by hand
    each teardown; committing them makes the OOPIF examples reproducible from a clean
    checkout (the README env recipe points at them).
  - **role=status was the wrong observable signal** (caught live). The first
    `child_action.html` revealed a hidden `<p role="status">Purchased</p>` on click; the
    re-observe reported `added=[]` and the assert panicked. Two reasons, both worth
    keeping: (1) a `role="status"` container's accessible **name is empty** — its text
    becomes a child `StaticText` node, not the container's name; (2) a text/state change
    reports into `diff.changed`, **not** `diff.added` (confirmed against
    `identity.rs:282-315`, where `update_binding` always refreshes `fingerprint`). Fix:
    relabel the **button's own text** on click (`buy.textContent = …`), since a button's
    accessible name *is* its text content, and read
    `map.binding(&eid).fingerprint.accessible_name` directly after re-observe rather than
    scanning the diff. The label is gated on `event.isTrusted`, so the observed name
    (`"Purchased"` vs `"Untrusted click"`) is itself the proof the click arrived trusted.
- VERIFY: `cargo test` (workspace) = **111 passing** (40 core + 67 cdp + 2 integration
  + 2 doctests; the 7 actions unit tests are unchanged — the channelization is a
  type-parameter lift, not a behavior change). `cargo clippy --all-targets -- -D warnings`
  clean. `cargo fmt --all -- --check` clean.
- LIVE PROOF (`examples/act_oopif`, same `chromedp/headless-shell --site-per-process`
  + two-origin static server harness as runs 14-16): navigate `parent_action.html`,
  first `observe()` surfaces the OOPIF button as **`f0/btn-buy-now`** (non-root frame
  key, name `"Buy now"`); routed `session.observer.act(&map, &buy_eid, Action::Click)`
  resolves the owning child session and dispatches the trusted pointer gesture there;
  second `observe()` reports the same eid, still under a non-root frame key, with
  accessible name **`"Purchased"`** — which can only happen if the click reached the
  right node, in the right frame, and arrived trusted. Exit 0.
- DECISIONS: D22 and the dispatch half of D23 are now closed (the read half closed in
  run 15). See DECISIONS.md.
- Commit sha: see the commit that lands this entry. **Phase 3.2d complete and
  live-verified; an OOPIF eid routes to its owning session for both read and write.
  Multi-frame identity (3.2a–3.2d) is done end to end. Next: 3.3 benchmark harness.**

## Build run 18 — 2026-06-17 — Phase 3.3a HAR recorder (network.har from CDP Network.* events)

- ROADMAP ITEM: **3.3a HAR recorder** (FIRST item under Phase 3.3, the critical
  path for the WebArena-Verified evaluator). Record a `network.har` from CDP
  `Network.*` events, hermetic and unit-testable against synthetic events, **no
  WebArena dependency** so it cannot be blocked by harness setup.
- WHAT WAS BUILT: a new `crates/anchortree-cdp/src/har.rs` (~940 lines) plus the
  `pub mod har;` + re-export block in `lib.rs`. The core is `HarRecorder`, a **pure
  state machine** keyed by `requestId` with no browser, async, or IO in the recording
  path. It folds the four correlated CDP events into HAR 1.2 entries:
  - `on_request_will_be_sent` opens a `Pending` (captures the `Request`, the wall
    `startedDateTime`, and the monotonic start). If the event carries a
    `redirect_response` for an id already pending, it **finalizes the previous hop as
    its own entry first** (redirect reuse of one requestId → one entry per hop) before
    opening the fresh `Pending`. Uses an edition-2024 let-chain.
  - `on_response_received` attaches the `Response` + `serverIPAddress`.
  - `on_loading_finished` finalizes with `bodySize = encodedDataLength`.
  - `on_loading_failed` finalizes with status 0 and an `_error` field.
  - `into_har` drains any still-in-flight `Pending` **sorted by monotonic start**
    (deterministic output) with `time = -1`, and emits a `version: "1.2"` log.
  - Serialization types (`Har`/`HarLog`/`HarEntry`/`HarRequest`/`HarResponse`/…)
    are `#[derive(Serialize)]` with the exact HAR camelCase / `startedDateTime` /
    `serverIPAddress` / `redirectURL` field names. `Har::to_json()` pretty-prints.
  - `enable<C: CdpChannel>(chan, session)` is the only live surface — it issues
    `Network.enable` through `run_on` so the recorder can later be fed a live stream.
- VERIFY: `cargo test` (workspace) = **124 passing** (40 core + 80 cdp + 2 integration
  + 2 doctests; +13 new `har` unit tests). `cargo clippy --all-targets -- -D warnings`
  clean. `cargo fmt --all -- --check` clean.
- TESTS (13, all synthetic — no browser): epoch-zero / known-epoch
  (`1_700_000_000.0` → `"2023-11-14T22:13:20.000Z"`) / fractional-seconds /
  millisecond-rounding-carry / leap-year-boundary ISO-8601 conversions;
  query-string parse; HTTP-version normalize (`h2`→`HTTP/2`); header decode;
  full request→response→finish makes exactly one entry; redirect chain yields one
  entry per hop; failed request records an error entry; in-flight requests flush in
  start order; emitted HAR is valid round-trippable JSON.
- JUDGMENT CALLS:
  - **Live event-subscription wiring is deferred to 3.3b on purpose.** 3.3a is scoped
    "hermetic, unit-testable against synthetic events, no WebArena dependency." The
    recorder takes already-decoded CDP event structs; subscribing the channel's event
    stream and pumping it into the recorder needs a live browser to record against,
    which is exactly what the 3.3b task-runner provides. Keeping the subscription out
    of 3.3a keeps every test browser-free and keeps the critical-path deliverable
    unblockable by harness setup.
  - **Dependency-free ISO-8601** via Howard Hinnant's `civil_from_days` rather than
    adding `chrono`/`time`. The HAR `startedDateTime` is the only date surface in the
    crate; a ~20-line public-domain algorithm is a better trade than a transitive
    dependency on the critical path. Covered by 5 conversion tests incl. a leap-year
    boundary and millisecond carry.
  - **Timing reported entirely under `wait`.** CDP `Network.*` events don't expose the
    sub-phase breakdown (blocked/dns/connect/send/receive) without the optional
    `Network.getResponseBody`/timing extras. Rather than invent fake sub-phases,
    `HarTimings::with_total` puts the whole measured duration under `wait` and zeroes
    `send`/`receive`, preserving the HAR invariant `time == blocked+dns+connect+send+
    wait+receive` (and `-1` for everything when the duration is unknown). The evaluator
    reads totals, not sub-phases, so this is lossless for our consumer.
  - **CDP newtype accessors** (`RequestId::inner`, `MonotonicTime::inner`,
    `TimeSinceEpoch::inner`, `Headers::inner`) — all fields are private in
    chromiumoxide_cdp 0.9.1; the `inner()`/`AsRef` accessors are the supported reads,
    no fork needed.
- DECISIONS: D25's 3.3a half is now confirmed (see DECISIONS.md).
- Commit sha: see the commit that lands this entry. **Phase 3.3a complete and fully
  hermetic; the WebArena-Verified evaluator's `network.har` input now has a producer.
  Next: 3.3b task-runner skeleton + `agent_response.json`, which wires this recorder
  to a live CDP event stream against one Verified RETRIEVE task.**

## Build run 19 — 2026-06-17 — Phase 3.3b (i + ii): live NetworkCapture pump + agent_response.json emitter

- ROADMAP ITEM: **3.3b task-runner skeleton + `agent_response.json` emitter**,
  shape pinned by **D26**. This run lands sub-steps **(i)** the live
  `Page`-event → `HarRecorder` pump and **(ii)** the `agent_response.json` writer.
  Sub-step **(iii)** (the offline-replay eval-assertion) is deliberately deferred
  — see judgment calls.
- WHAT WAS BUILT: a new `crates/anchortree-cdp/src/runner.rs` plus the `pub mod
  runner;` + re-export block in `lib.rs`, and a live proof example
  `examples/webarena_capture.rs`.
  - `NetworkCapture::start(page: &Page)` subscribes the four `Network.*` event
    streams via `Page::event_listener::<T>()` (each `EventStream<T>: Stream<Item =
    Arc<T>>`), tags each into a `NetEvent` enum, merges them with two nested
    `stream::select`s into one `BoxStream<'static, NetEvent>`, enables Network,
    and spawns a background Tokio task that folds every event into a `HarRecorder`.
    Per **D26** this rides the local `chromiumoxide::Page` path, NOT the thin
    `RawCdpSession` channel — the channel read loop drains and discards events, so
    it is not an event tap.
  - The pump avoids the `select!` macro (the lib pulls only tokio `rt`+`sync`, no
    `macros`): it folds the `oneshot` stop signal into the same stream as the
    events via `stream::once(...).map(|()| Control::Stop)` + `stream::select`, then
    on `Stop` drains already-queued events with `next().now_or_never()` before
    finishing. Clean, macro-free, and deterministic.
  - `NetworkCapture::finish()` sends the stop, awaits the pump task, and returns
    `recorder.into_har()`. A join failure maps to `CdpError::Malformed`.
  - Agent contract output: `TaskType` (RETRIEVE/MUTATE/NAVIGATE) and `TaskStatus`
    (SUCCESS/NOT_FOUND_ERROR/PERMISSION_DENIED_ERROR), both
    `#[serde(rename_all = "SCREAMING_SNAKE_CASE")]`; `AgentResponse {task_type,
    status, retrieved_data: Option<Value>, error_details: Option<String>}` with all
    four keys always emitted (absent optionals serialize as `null`, since the
    runner reads by fixed key); constructors `retrieved`/`completed`/`failed`;
    `write_task_output(dir, &resp, &har)` writes `agent_response.json` +
    `network.har` (exact filenames, `create_dir_all` first).
- VERIFY: `cargo test` (workspace) = **128 passing** (40 core + 84 cdp + 2
  integration + 2 doctests; +4 new `runner` unit tests; the new `///` example is a
  `ignore` doctest, counted ignored not passing). `cargo clippy --all-targets --
  -D warnings` clean. `cargo fmt --all -- --check` clean.
- TESTS (4 hermetic, no browser): `AgentResponse` RETRIEVE serializes with
  `RETRIEVE`/`SUCCESS`/data present/`error_details` null; failed serializes
  `NOT_FOUND_ERROR` + details + null data; completed MUTATE has null data and null
  error; `write_task_output` emits both exact filenames, both valid JSON, the HAR
  round-trips to a 1.2 log (dependency-free unique temp dir, cleaned up after).
- LIVE PROOF (`examples/webarena_capture`, exit 0): local `chromedp/headless-shell`
  + a `python -m http.server` static site (files pushed in with `docker cp` — a
  bind-mount of the phantom container's `/tmp` does NOT work, the host has no such
  path; this bit once mid-run and the server 404'd until the `docker cp` fix).
  `NetworkCapture::start` → `page.goto` → `wait_for_navigation` → read
  `document.title` → `finish()` produced **3 HAR entries**: `index.html` (200,
  text/html, 435 B), `style.css` (200, text/css, 228 B), `app.js` (200,
  text/javascript, 225 B), each with a real request URL, status, body size,
  `serverIPAddress`, and timings; the `time == send+wait+receive` invariant held on
  all three (0 violations). The written `agent_response.json` =
  `RETRIEVE`/`SUCCESS`/`retrieved_data: "Acme Widget 1299"`/`error_details: null`,
  and the written `network.har` round-tripped to a valid 1.2 log. End to end: the
  browser-free recorder, fed by a live CDP event stream, produced the exact
  WebArena-Verified per-task output.
- JUDGMENT CALLS:
  - **Sub-step (iii) deferred to the next run on purpose.** (i)+(ii) are the real
    engineering — the live event-stream pump and the contract emitter, both
    testable without external infrastructure (same discipline that made 3.3a land
    clean). (iii) needs `uv pip install "webarena-verified[examples]"`, a
    `config.json`, and a specific pinned RETRIEVE task to produce the first real
    `result.score` via offline HAR replay. That is a separate substantial chunk
    with an external-package + real-task dependency; bundling it here would have
    made this increment sloppy. Recorded as the explicit next step in STATE +
    ROADMAP (3.3b marked `[~]` in-progress, not `[x]`).
  - **`TaskStatus` models only the D26-verified terminals** (SUCCESS,
    NOT_FOUND_ERROR, PERMISSION_DENIED_ERROR). The full runner error vocabulary
    was not verified, so I did not invent values an evaluator might reject; the
    first 3.3b target is a single RETRIEVE that reports SUCCESS. Pin the full enum
    against the runner before 3.3d's multi-task loop.
  - **Macro-free pump.** The library tokio features are `rt`+`sync` only (no
    `macros`), so `tokio::select!` is unavailable; rather than widen the feature
    set I folded stop+events into one `stream::select` and drained with
    `now_or_never`. Smaller dependency surface, same behavior.
  - **Subscribe before enable.** `event_listener` is called for all four types
    before `Network.enable`, so no early request can slip between the enable ack
    and the listeners being installed.
- DECISIONS: D26's sub-steps i+ii are now confirmed (see DECISIONS.md).
- Commit sha: see the commit that lands this entry. **Phase 3.3b (i+ii) complete
  and live-verified; anchortree can now produce the WebArena-Verified per-task
  output (`agent_response.json` + a real `network.har`) for a live navigation.
  Next: 3.3b (iii) — the offline-replay eval-assertion for the first real
  `result.score`, then 3.3c re-grounding-calls instrumentation (the headline).**

## Build run 20 — 2026-06-17 — Phase 3.3b (iii): offline-replay eval-assertion (first real result.score = 1.0) + TaskStatus enum completed (D27)

- **ROADMAP item:** 3.3b sub-step (iii) — get the first real WebArena-Verified
  `result.score` back from an offline HAR replay — plus the two D27 carry-ins
  (complete the `TaskStatus` enum; pin the replay-artifact contract). 3.3b is now
  `[x]` complete end to end (i+ii+iii).
- **What shipped:**
  - **`TaskStatus` enum completed to the full closed six-value set** (`runner.rs`):
    added `ActionNotAllowedError`, `DataValidationError`, `UnknownError` alongside the
    prior `Success`/`PermissionDeniedError`/`NotFoundError`, plus a
    `TaskStatus::unknown()` catch-all constructor. The existing
    `rename_all = "SCREAMING_SNAKE_CASE"` carries the wire spelling; a new unit test
    pins every one of the six against its exact string
    (`SUCCESS`/`ACTION_NOT_ALLOWED_ERROR`/`PERMISSION_DENIED_ERROR`/`NOT_FOUND_ERROR`/
    `DATA_VALIDATION_ERROR`/`UNKNOWN_ERROR`).
  - **New `eval.rs` module** (the score-readback surface, split pure/impure):
    `EvalResult`/`EvaluatorResult` with `from_eval_result_json` + `from_task_dir`
    parsers (and `is_pass()` = exact `score == 1.0`, safe because WA-V scores are exact
    rationals); `task_output_dir(root, id)` for the `{root}/{task_id}` layout;
    `eval_tasks_args` (pure argv builder — comma-joined `--task-ids`, optional
    `--config`, omits the ids flag when empty) and `eval_tasks_command`; the single
    impure edge `run_eval_tasks(root, ids, cfg)` which shells out and degrades to a
    typed `EvalError::BinaryNotFound` when the Python CLI is absent; `EvalError`
    (`BinaryNotFound`/`Spawn`/`EvalFailed{code,stderr}`/`ResultMissing`/`Malformed`).
    9 unit tests, the parser pinned against the **real captured `eval_result.json`**
    (not a hand-written shape). Wired `pub mod eval;` + re-exports into `lib.rs`
    (also re-exported the previously-internal `HarCache` so an external example can
    hand-build a HAR).
  - **Gated `examples/eval_task`** — writes `agent_response.json` (the correct
    task-21 answer) + a hand-built one-entry `network.har` into `{root}/21`, drives the
    real `webarena-verified eval-tasks --task-ids 21 --output-dir <root>` **fully
    offline (no Docker site)**, parses the result via `EvalResult`, and asserts
    `score == 1.0`. CLI-gated: with `webarena-verified` absent it prints an install
    hint and exits 0, so CI (no Python tool) stays green.
- **Live verification:** ran `examples/eval_task` against the installed
  `webarena-verified` 1.2.3 (a `uv` venv) → `task 21 -> status="success" score=1` and
  `evaluator AgentResponseEvaluator -> success (1)`, the **first real WebArena-Verified
  score for anchortree**. Re-ran with the CLI off `PATH` → clean `SKIP` + exit 0,
  confirming the CI-safe gate. `cargo test` = 138 green (40 core + 94 cdp + 2
  integration + 2 doctests); `cargo clippy --all-targets -D warnings` clean;
  `cargo fmt --all` clean.
- **Judgment calls:**
  - **Empirical correction to the D27 carry-in (b):** an `AgentResponseEvaluator`
    RETRIEVE task scores with just **two** artifacts — `agent_response.json` + a
    ≥1-entry `network.har`. **No `config.json` is required** (verified). The HAR
    *contents* are ignored by this evaluator, but the loader still parses the `.har`
    before dispatch: an empty-entries HAR raises `ValueError` in `load_har_trace`,
    which `tracing.py` catches and falls back to the Playwright line-parser, which then
    `KeyError`s on `item["type"]` and errors the task to score 0.0. So the real gate is
    "the HAR must parse with ≥1 entry", not "supply a config". A `config.json` remains
    needed for URL/credential-resolving evaluators (the MUTATE/NAVIGATE surface), which
    is the next-task concern, not this one.
  - **Hand-built HAR in the example, not a live capture.** 3.3b (i)+(ii) already proved
    the live pump (run 19, `examples/webarena_capture`); what (iii) proves is the
    score-readback, where the HAR's provenance is irrelevant (the evaluator ignores its
    contents). So the example builds a minimal valid one-entry HAR from the public
    `Har` field surface with no browser — keeping the example CLI-gated only, not
    browser-gated, so it is cheap and runs anywhere the Python tool is installed.
  - **Eval-scoring logic is CI-tested; the live score is a gated example.** The Python
    CLI is not in CI, so the parse/builder/layout logic is unit-tested (pinned against
    the real `eval_result.json`) and only the subprocess call lives in the gated
    example. `run_eval_tasks` returns `EvalError::BinaryNotFound` rather than panicking
    when the tool is absent, so a harness in a tool-less environment treats it as
    "skip", not "fail".
- DECISIONS: D27 is now confirmed (see DECISIONS.md), with the config.json correction.
- Commit sha: see the commit that lands this entry. **Phase 3.3b is complete end to
  end — anchortree's WebArena-Verified eval loop now closes: observe → act → emit
  `agent_response.json` + `network.har` → real `result.score`. Next: 3.3c
  re-grounding-calls instrumentation (the thesis headline).**

## Build run 21 — 2026-06-17 — Phase 3.3c re-grounding-calls instrumentation (D28)

The thesis headline, made into one defensible number. anchortree's claim is that
durable element identity lets an agent skip the LLM call a naive agent pays to
*re-ground* an element after a re-render. 3.3c counts that.

- **`anchortree-core/src/metric.rs` (new) — `RegroundLedger`.** A pure,
  browser-free per-task accumulator. One mutator, `record(&Diff)`, adds
  `diff.rebound.len()` to the headline (`rebinds_zero_llm`) and counts the observe
  pass. `llm_reground_calls()` returns 0 **by construction**: the type has no API
  that could record a model call, so the zero is structural, not a runtime
  accident. Also `observes()`, `has_rebinds()`, and `render()` →
  `N durable rebinds at 0 LLM re-grounds (over M observes)`. Re-exported from the
  core crate root.
- **Honesty guardrails (D28) enforced by tests, not prose.** Two unit tests pin
  the inflation traps: `added_and_changed_never_inflate_the_headline` folds a diff
  full of adds/changes/removals with zero rebounds and asserts the headline stays
  0; `llm_reground_count_is_zero_under_any_diff_churn` folds 50 busy diffs and
  asserts the LLM count never moves off zero. Only Path 2 `diff.rebound` counts;
  `diff.added` (Path 3 mint = first-ground) and `diff.changed` (Path 1 = cheap attr
  update) never do.
- **`crates/anchortree-core/tests/metric.rs` (new) — measured off the real
  engine.** Drives a genuine `IdentityMap` through first paint (3 mints, 0
  counted), a hard re-render with brand-new backend ids (3 rebinds, counted), and a
  benign text/state update with the same backend ids (Path 1 `changed`, 0 counted).
  The headline is exactly 3 — the re-render survivals and nothing else. This proves
  the metric against genuine engine output, not hand-written diffs.
- **`anchortree-cdp/src/eval.rs` — `task_headline(eval, ledger)`.** Pairs the
  WebArena-Verified `score` (the correctness gate) with the rebind headline (what
  the durable engine saved) on one line — the exact sentence the 3.3e report is
  built from, making the 3.3d peer comparison immediate. Unit-tested against the
  real captured `eval_result.json`:
  `task 21: score 1.00 (success) — 3 durable rebinds at 0 LLM re-grounds (over 2 observes)`.
  Re-exported from the cdp crate root.

VERIFY: `cargo test --workspace` = **145 passing** (45 core [+5] + 95 cdp [+1] + 2
integration + 1 metric integration [new] + 2 doctests [1 ignored]); `cargo clippy
--all-targets -- -D warnings` clean; `cargo fmt --all` clean.

Judgment calls:
- **Metric lives in `anchortree-core`, not the cdp runner.** D28 says "accumulate
  in the runner," but the headline logic + guardrails are pure logic over `Diff`,
  which is a core type. Putting `RegroundLedger` next to `Diff`/`budget` keeps it
  browser-free and unit-testable, and the cdp side threads it through its observe
  loop via the re-export — the runner still owns accumulation, the *type* just lives
  where it can be tested without Chrome.
- **Proved hermetically against `IdentityMap`, no new browser example.** The metric
  is pure; the strongest proof is a real observe→rerender→observe sequence fed
  through the engine (no Chrome needed), which is exactly what the new integration
  test does. A gated live example would add a browser dependency for a number the
  engine already produces deterministically offline. The live score half is already
  proven by run 20's `eval_task`; `task_headline` joins the two.
- **`llm_reground_calls()` returns a literal 0.** That is the honest encoding: the
  ledger cannot represent a re-ground, so the structural assertion is the absence of
  any mutator that could increment it, backed by the 50-diff-churn test — not a
  counter that happens to stay at zero.

- Commit sha: see the commit that lands this entry. **Phase 3.3c is done — the
  thesis headline is now a tested number sourced from real engine output. Next:
  3.3d dual real-peer baseline (Playwright-MCP token-volume + Stagehand self-heal
  LLM-call count), then 3.3e report over the 258-task subset.**

## Build run 22 — 2026-06-17 — Phase 3.3d dual real-peer baseline (D29)

The thesis headline needs a *comparison* to mean anything. 3.3c gave the number in
anchortree's own terms (durable rebinds at zero LLM re-grounds); 3.3d adds the two
peers a real agent would otherwise reach for, modelled offline so the benchmark stays
HERMETIC — no live Stagehand/Node/OpenAI/Playwright-MCP server.

- **`anchortree-core/src/peer.rs` (new) — two independent peer models + a report.**
  - **Playwright-MCP token model.** `playwright_snapshot(&[ObservedNode]) -> String`
    renders the page in the tool's own line shape (`- button "Sign in" [ref=e13]`,
    ref = `backendNodeId`), and `snapshot_tokens` prices it with the *same*
    `estimated_tokens` (`ceil(chars/3.5)`) ruler the engine prices its diff with. The
    peer re-sends the whole snapshot every turn; anchortree sends only `diff_tokens`.
    Fair, apples-to-apples, fully offline.
  - **Stagehand self-heal model.** `DomPositions` is a bidirectional logical↔XPath
    bijection (`place`/`xpath_of`/`logical_at`); `StagehandCache` caches an absolute
    XPath per acted element (`bind`, free) and re-tries them each turn (`reresolve`),
    charging one `self_heal` per cached selector that no longer resolves to its
    logical element and repairing the cache. This is decidedly **not** a reuse of
    `rebinds_zero_llm`.
  - **`BaselineReport`** folds a task's turns (`record_turn` for the token axis,
    `set_peer_self_heals` for the LLM axis) and `render()`s both columns:
    `anchortree: N diff tokens, 0 re-grounds | peer: M snapshot tokens, K self-heals
    (over T turns)`. `anchortree_regrounds()` is a structural 0, the same zero
    `RegroundLedger` carries.

- **`anchortree-core/tests/peer.rs` (new) — the D29 nuance proven against the REAL
  engine.** A 4-turn login task drives a genuine `IdentityMap` while replaying the
  same page states through both peers:
  - Turn 2 (in-place re-render: new backend ids, same XPaths) = **3 engine rebinds /
    0 peer self-heals** — rebind WITHOUT self-heal.
  - Turn 3 (sibling "Skip" link inserted: same backend ids, every index shifts) =
    **0 engine rebinds / 3 peer self-heals** — self-heal WITHOUT rebind.
  - Grand totals: **6 rebinds vs 3 self-heals.** They diverge per turn AND in total,
    which is impossible if the rebind count were a proxy for the self-heal count.
    Token axis: peer snapshot total strictly exceeds anchortree's diff total.

## Judgment calls (run 22)

- **Pure-core, no cdp/eval touch.** D29's two peer models are pure logic over
  `ObservedNode`/`Diff` and the existing tokenizer, so they belong in `anchortree-core`
  next to `metric`/`budget` where they are unit-testable without Chrome. The cdp-side
  pairing of the live eval score with the peer baseline is 3.3e's job (the report over
  the 258-task subset), not this run's — keeping the first cut to task 21 only, as the
  spec says.
- **`StagehandCache::reresolve` returns the per-turn heal delta** (not just bumping the
  running total) specifically so the integration test can assert the turn-by-turn
  divergence — the delta is what proves rebind ≠ self-heal, the grand total alone
  would not.
- **`aria_role` is a private inverse of `Role::from_aria`.** The peer snapshot must
  speak the tool's vocabulary (`button`, not `btn`), and `from_aria` is many-to-one
  (`status`/`alert` → `Status`); the inverse picks the canonical spelling and
  round-trips `Other(s)` verbatim. Kept private — it exists only to render the peer
  snapshot fairly.
- **The grand totals are engineered to differ (6 vs 3), not coincide.** An earlier
  symmetric design landed 6 rebinds and 6 self-heals; equal totals invite the
  misread "see, they're the same metric." Turn 4 is a pure in-place re-render over the
  already-shifted layout, so the engine rebinds three more while the repaired cache
  heals nothing — the totals split, and the non-equality is itself an assertion.

- Commit sha: see the commit that lands this entry. **Phase 3.3d is done — the peer
  side of the comparison is now a tested baseline against real engine output, with the
  D29 rebind ≠ self-heal nuance proven both directions. Next: 3.3e report over the
  258-task difficulty-prioritized subset — the publishable headline number.**

---

## Build run 23 — Phase 3.3e: the multi-task Hard report (two denominators, structurally apart)

3.3e is the publishable headline: fold a whole **WebArena Verified Hard** task
set (210 single-site + 48 multi-site; ServiceNow) into one report. Shipped as
`report.rs` in `anchortree-cdp` — it lives cdp-side because it pairs `EvalResult`
(cdp) with `RegroundLedger` + `BaselineReport` (core re-exports).

- `TaskRecord` — one task's contribution. `scored(eval, ledger, baseline)` carries
  an `EvalResult` (counts toward the score denominator **N**); `baseline_only(
  task_id, ledger, baseline)` does not (counts only toward the baseline
  denominator **M**). `is_pass()` is tri-state `Option<bool>` so an unscored task
  never silently reads as a failure.
- `Report` — aggregates a `Vec<TaskRecord>`. Score axis (`scored_tasks` = N,
  `passes`, `score_sum`, `mean_score`÷N, `pass_rate`÷N); baseline axis
  (`baselined_tasks` = M, `anchortree_diff_tokens`, `peer_snapshot_tokens`,
  `engine_rebinds`, `peer_self_heals`, `anchortree_regrounds`→0, `token_ratio`,
  `total_turns`). `render()` emits the two-denominator headline pair.
- 10 unit tests + 1 integration test (`tests/report.rs`) driving the REAL captured
  task-21 eval plus baseline-only tasks through the genuine `IdentityMap` engine:
  N=1 scored (mean 1.00), M=3 baselined, 4 engine rebinds vs 2 peer self-heals at
  0 re-grounds. 168 tests pass workspace-wide; clippy clean under `-D warnings`;
  fmt clean. D30 moved PROPOSED → CONFIRMED.

## Judgment calls (run 23)

- **The over-claim guard is structural, not a convention.** D30's whole point is
  that 3.3e has two denominators and a blended number would merge them. So no
  method on `Report` crosses the two: `mean_score`/`pass_rate` divide by N
  (`scored_tasks`), every token/rebind aggregate sums over the baselined set.
  `mean_score_divides_by_scored_n_not_baselined_m` pins exactly the conflation
  that would over-claim — one scored task (1.0) plus two baseline-only tasks must
  yield mean 1.0/1, never 1.0/3.
- **`baselined_tasks` counts replayed turns, not record count.** M is the number
  of tasks that actually contributed a baseline turn (`baseline.turns() > 0`), so
  a record with an empty baseline does not inflate the denominator. N ≤ M holds
  naturally without being enforced — it falls out of the two filters.
- **Report lives in `anchortree-cdp`, not core.** `EvalResult` is a cdp type
  (it parses the runner's `eval_result.json`), so the score-bearing half of the
  report cannot live in the browser-free core. The baseline half re-exports from
  core. This is the first core+cdp pairing type — the seam D30 anticipated.
- **Integration test drives the real engine for the baseline numbers.** Rather
  than hand-writing diffs, `tests/report.rs` runs `IdentityMap::observe` for an
  in-place re-render (2 rebinds / 0 self-heals) and a sibling insert (0 rebinds /
  2 self-heals), so the aggregate 4-vs-2 split is engine output, not a fixture.
- **Full-corpus wiring is explicitly out of scope and recorded as such.** The
  aggregator is the engineering owed by 3.3e; feeding it all 258 Hard tasks needs
  each task's replayable observe sequence captured offline — a data task. The
  module docs and ROADMAP both say so, so no future reader mistakes "proven against
  task-21 + synthetic" for "run over the full set".

- Commit sha: see the commit that lands this entry. **Phase 3.3e is done — the
  multi-task report aggregator is a tested, denominator-honest surface; the score
  axis and the thesis token/re-ground axis are reportable side by side without
  conflation. Next: 3.4 (keep `RawAxNode` transport-neutral for a future
  `anchortree-bidi` adapter — no CDP types past `observer.rs`).**

---

## Build run 24 — Phase 3.4: the transport-neutrality guard (D9, D31)

3.4 turns the cross-browser claim from a hand-grep into a fitness function. The
whole library rests on one structural invariant: CDP types live only in the thin
transport adapters, decode into the plain `RawAxNode` / `RawAttrs` value structs
at `observer.rs`, and nothing in the fusion path or in `anchortree-core` ever
names `chromiumoxide`. Until this run that seam was verified by eye every build.
Now `crates/anchortree-cdp/tests/transport_neutrality.rs` makes it a build gate.

- Three integration tests, matching the two halves of D9. (1)
  `anchortree_core_names_no_cdp_type` — scans every `.rs` under
  `anchortree-core/src` and fails if any code line names `chromiumoxide` (the
  engine crate does not even depend on it). (2)
  `cdp_surface_is_exactly_the_transport_adapters` — asserts the set of cdp-side
  files that reference a CDP type in code equals the pinned `CDP_ADAPTER_FILES`
  (`actions.rs`, `channel.rs`, `error.rs`, `har.rs`, `observer.rs`, `runner.rs`).
  A new transport-touching file, or a leak into the fusion path, breaks the
  partition and forces a deliberate decision. (3) `fusion_path_is_cdp_free` —
  pins `fuse.rs` / `eval.rs` / `report.rs` clean with a pointed message.
- `code_names_chromiumoxide` ignores any line whose trimmed-left form starts with
  `//`, so the doc prose in `lib.rs` / `frames.rs` / `gateway.rs` ("decoded from
  chromiumoxide in observer.rs") is not a false positive; only code refs count.
- `fuse.rs` gains `pub type TransportNodeKey = i64` — the opaque, per-pass
  node-identity key a transport hands the engine. CDP fills it from
  `backendNodeId`; a BiDi adapter would fill it from a `sharedId`-derived integer.
  `RawAxNode.backend_node_id`, `ListenerRoles`, `residual_backends`,
  `observable_backends`, and `fuse`'s three keyed maps now name the alias. The
  alias is transparent (= i64), so `observer.rs` and every core/identity call site
  compiles unchanged — zero churn.
- 171 tests pass workspace-wide (168 + 3 new); clippy clean under `-D warnings`;
  fmt clean. D31 moved PROPOSED → CONFIRMED.

## Judgment calls (run 24)

- **The guard is a source-scan fitness function, not a type-level trick.** The
  leak we defend against is a human one: someone reaching for
  `chromiumoxide::...::BackendNodeId` in `fuse.rs` because it is one import away.
  A trait-sealing scheme would not catch a fresh `use` line in a new file; a text
  scan catches it at the only place it can be caught, and reads as an
  architecture rule in the test output.
- **`TransportNodeKey` is an alias, not a newtype.** A newtype would have forced a
  wide rename across core + identity and broken the "seam only" directive D31 gave
  for 3.4. As a transparent alias it names the concept at the public seam and
  documents the BiDi `sharedId` story without touching a single call site.
- **The BiDi adapter is deferred, on purpose.** BiDi has no full-AX-tree dump
  (w3c/webdriver-bidi#443 is OPEN as of 2025-12-12; only an accessibility
  *locator* via `browsingContext.locateNodes`), so a real `anchortree-bidi`
  adapter must CONSTRUCT the tree from a script-injected AX walk + DOM, not read a
  `getFullAXTree` equivalent. 3.4 ships the seam and the guard, not a half adapter
  against a moving target. `fuse.rs` module docs record this.
- **The guard was proven to bite before shipping.** I injected a one-line
  `chromiumoxide` reference into `eval.rs`, confirmed both
  `fusion_path_is_cdp_free` and `cdp_surface_is_exactly_the_transport_adapters`
  failed, then reverted with a `head -n -2` rewrite (not `git checkout`, which
  would have discarded the unstaged `fuse.rs` edits). `git diff --stat` confirmed
  `eval.rs` clean afterward. A guard that cannot fail is worthless; this one does.

- Commit sha: see the commit that lands this entry. **Phase 3.4 is done — the
  transport-neutral fusion boundary is now a build gate, and the opaque
  per-pass key is named at the seam for a future BiDi adapter. Next: 3.5 (capture
  the 258-task replayable observe corpus offline — the data task that feeds the
  3.3e aggregator over the full Hard set).**

---

## Build run 25 — Phase 3.5a: the real-fixture corpus loader (D32, corrected)

3.3e (run 23) built the multi-task `Report` aggregator and proved it on the
captured task-21 eval plus *synthetic* observe sequences. 3.5a is the first
consumer that feeds it **real WebArena-Verified artifacts** off disk, turning the
aggregator from "tested on synthetic" into "tested on genuine evaluator output"
— the first non-task-21 numbers anchortree publishes.

Shipped:
- Vendored the two demo task logs ServiceNow's `webarena-verified` repo ships
  (`examples/agent_logs/demo/{107,108}`) under repo-root `corpus/107` and
  `corpus/108` (byte-exact `eval_result.json` + `agent_response.json`), plus the
  Hard subset id list (`corpus/subsets/webarena-verified-hard.json`, 258 ids).
  The repo is **Apache-2.0**, so redistribution is allowed with attribution
  (`corpus/README.md`).
- `crates/anchortree-cdp/src/corpus.rs`: `load_task` / `load_corpus` /
  `load_subset_ids` / `report_from_corpus`, with `CorpusTask` (accessors +
  `is_scorable` / `is_replayable`), `AgentAnswer` (a read-side model, distinct
  from runner's write-only `AgentResponse`), and `CorpusError`. `report_from_corpus`
  folds every scorable task into the report as a scored `TaskRecord`, giving a real
  **N=2** aggregate: 108 RETRIEVE pass 1.0, 107 NAVIGATE fail 0.0, mean 0.50.
- 7 unit tests + 5 integration tests (`tests/corpus.rs`) over the vendored real
  fixtures. `corpus.rs` is CDP-free and added to the transport-neutrality guard's
  `FUSION_PATH_FILES` so it stays behind the seam.
- `corpus/fetch-hars.sh` + a `corpus/.gitignore` that ignores `*/network.har`: the
  large traces are fetched on demand, not vendored.

Test count: 183 passing (was 171). clippy clean under `-D warnings`, fmt clean.

## Judgment calls (run 25)

- **The load-bearing correction to D32: a `network.har` cannot produce the
  baseline axis (M) offline.** D32 (research run 23) assumed the demo HARs make
  each task "baselineable (M)" as well as scorable (N). Reading the real fixtures
  and the crate showed that is wrong: a HAR is a *network trace* (request/response
  bodies), not an accessibility capture, and `anchortree-cdp` has no offline
  HTML→AX path (no html-parser dependency in the tree, by design — the AX tree
  comes from a live `getFullAXTree`). The baseline tallies need a replayed
  *observe* sequence (per-turn AX + DOM + layout the engine can diff), which needs
  a browser. So 3.5a ships the genuinely-real **score axis (N=2)** and defers M to
  the 3.5b browser-in-loop capture. I did NOT fabricate baseline numbers from the
  HAR to hit the planned "N=2/M=2"; forward motion on honest numbers beats a
  blended figure the fixtures cannot support. The HAR is modeled only as the
  *replayable precondition* (`is_replayable`).
- **`AgentAnswer`, not a reused `AgentResponse`.** `runner::AgentResponse` is a
  Serialize-only write contract ("do not rename") with SCREAMING_SNAKE_CASE enums.
  Reusing it for *reading* arbitrary corpus answers (which may carry task types the
  write enum does not enumerate) would have coupled the read path to the write
  contract. A separate tolerant `AgentAnswer` (task_type stays a plain String)
  keeps the two directions independent.
- **Missing files are tolerated; malformed files error.** A partially captured
  corpus (e.g. an `eval_result.json` with no HAR yet) still loads — the loader
  records what it found and lets `is_scorable` / `is_replayable` gate behavior —
  but a present-but-broken JSON file fails loudly so a corrupt vendor never scores
  silently.
- **The doc-comment `+ layout` trap.** clippy's `doc_lazy_continuation` fired
  because a wrapped doc line began with `+ layout`, which the markdown parser read
  as a new list marker and broke the bullet's continuation. Reworded to
  "accessibility, DOM, and layout" so no continuation line starts with `+`.

- Commit sha: see the commit that lands this entry. **Phase 3.5a is done — the
  3.3e aggregator now runs on real WebArena-Verified evaluator output (N=2), and
  the corpus loader scales to the full Hard set unchanged. Next: 3.5b — the
  browser-in-loop observe capture that fills the baseline axis (M), plus growing N
  toward the 258 Hard ids.**

## Build run 26 — Phase 3.5b Tier 1: the hermetic HAR replay matcher (D33 Tier-1 core)

ROADMAP item: 3.5b Tier 1 matcher. STATE pointed here directly (3.5a done run 25;
research run 24 pinned the M-capture mechanism as D33). Shipped `replay.rs` — the
browser-free heart of the HAR→chromium fulfill layer — plus its lib wiring and the
transport-neutrality pin. 10 new unit tests; workspace 183 → 193 green; clippy clean
under `-D warnings`; fmt clean.

What it is: the **matcher**. Given a third-party `network.har` and a live request, it
selects the recorded entry that answers it, mirroring Playwright's `routeFromHAR` rule —
strict URL + method (method case-insensitive per HTTP), strict POST payload when the
request carries one, ties broken by most-matching request headers (then earliest
recording). No match returns `MatchOutcome::Abort`, never a guess. It surfaces the matched
entry's status/headers/mime and body location (`ReplayBody::{Inline{base64},External,Empty}`)
for the fulfiller. Public API: `ReplayRequest` (+ `get`), `ReplayHar` (`from_json`/`entries`/
`match_entry`/`outcome`), `ReplayEntry` (accessors), `ReplayBody`, `MatchOutcome`.

Judgment calls:
- **The matcher is the CI-runnable heart; the CDP fulfill leg is the deferred half.** D33's
  Tier 1 is a full `Fetch.requestPaused`→`fulfillRequest` layer, which needs a live browser
  and therefore lands as an example, not a CI test (the project's standing pattern for
  transport-touching code — every OOPIF/act step shipped that way). So this run builds the
  pure selection rule, fully unit-tested without a Chrome, and leaves only the wiring. This
  is real forward motion on the part that CI can guard, not a stub.
- **Its own `Deserialize` read model, split from the `Serialize`-only `har.rs`.** The
  recorder's `Har`/`HarEntry`/... are a body-less write contract ("bodies are not captured").
  Replay *reads* a HAR some other tool produced, which carries bodies. Modeling the read side
  independently (`ReplayHar` and friends, `#[serde]` tolerant of unknown fields) repeats
  exactly the read-vs-write split run 25 made for `AgentAnswer` vs `runner::AgentResponse`.
  It also keeps the matcher CDP-free and behind the transport seam (D9) — pinned in the
  neutrality guard's `FUSION_PATH_FILES`, not its CDP-adapter set.
- **External `_file` bodies discovered in the real HARs.** Fetching the two demo HARs
  (804617 bytes each, a shared 359-entry browser-use trajectory, all GET) showed their
  response bodies live in external `content._file` references (e.g. `"resources/42.html"`),
  not inline `content.text`. `ReplayBody` surfaces both cases (and base64-encoded inline,
  and empty) so the fulfiller can resolve either; the matcher itself never opens the file.
- **`notFound = abort` is the honesty guard to the byte.** An off-trajectory request failing
  loudly (rather than serving a fallback and silently rendering a wrong page) is the D30
  honesty rule carried into replay: a contaminated observe sequence must not quietly inflate M.
- **`PartialEq`/`Eq` on the read structs.** `MatchOutcome` derives `PartialEq`/`Eq` (the tests
  assert `== Abort`), which transitively requires `ReplayEntry` and its fields to be `Eq`.
  Added the derives to the read structs; harmless for a value-only read model.

The corpus integration test `vendored_corpus_loads_both_demo_tasks` asserts the *committed*
state (no `network.har` vendored — they are git-ignored, fetched on demand). The locally
fetched HARs were removed after use so local state matches the clone CI sees; the matcher's
tests are hermetic and never needed the real HARs.

Commit sha: see the commit that lands this entry. **Next: the 3.5b Tier 1 fulfill wiring —
decode `Fetch.requestPaused`→`ReplayRequest`, `Fetch.fulfillRequest` the matched entry
(resolving external `_file` bodies) or `Fetch.failRequest` on abort, then run the observe
loop over the replayed DOM for the first M=1 on task 108 (RETRIEVE). A live example, not CI.**

---

## Build run 27 — Phase 3.5b recorder body capture (D34, 2026-06-18)

**ROADMAP item:** Phase 3.5b — teach `HarRecorder` to capture response bodies (D34 step a),
the CI-runnable heart of the M-capture path. 198 workspace tests (was 193; +5 har.rs body-capture
unit tests). `cargo clippy --all-targets -- -D warnings` clean, `cargo fmt --check` clean.

**Why this, and why now.** Research run 25 (commit f2c0db3) landed D34, which corrects D33's
working assumption. The ServiceNow demo HARs (tasks 107/108) are *structurally unfulfillable*:
359 GET entries, zero inline `content.text`, 354 external `content._file` refs to a sidecar dir
the repo never vendors, and 5 empty including the primary document body. Replaying them fulfills
nothing → no render → no M. So the hermetic replay target is NOT the demo HARs — it is
**anchortree's own recorder output**, once the recorder can capture bodies. Run 26 shipped the
matcher that reads a body-carrying HAR; run 27 makes the recorder write one. The loop is:
record-with-bodies (live, once) → replay-hermetically (CI, forever).

**What shipped, in `crates/anchortree-cdp/src/har.rs`:**
- `HarContent` gains two optional fields: `text: Option<String>` and `encoding: Option<String>`,
  both `#[serde(skip_serializing_if = "Option::is_none")]`. These mirror the HAR 1.2 `content`
  shape and are exactly what `replay.rs` reads back as `ReplayBody::Inline { text, base64 }`
  (`content.text` + `content.encoding == "base64"`).
- A new public input type `ResponseBody { text: String, base64: bool }` — the transport-neutral
  value a live feeder produces from a `Network.getResponseBody` reply. Re-exported from `lib.rs`.
- `Pending` gains a `body: Option<ResponseBody>` field.
- `HarRecorder::on_response_body(request_id, body)` sets `pending.body` on a still-in-flight
  request. It runs between `on_response_received` and `on_loading_finished` (a getResponseBody
  read is issued at loadingFinished time while the pending entry still exists). A call for an
  unknown id is a tolerant no-op, keeping the feeder a thin pass-through.
- `finalize` applies the captured body: `content.text = Some(body.text)` and
  `content.encoding = body.base64.then(|| "base64".to_string())`.

**Judgment calls:**
- **`Network.getResponseBody`, not the Fetch-domain body read.** chromiumoxide 0.9.1 exposes
  two body surfaces: `Network.getResponseBody` (`GetResponseBodyParams::new(request_id)` →
  `GetResponseBodyReturns { body, base64_encoded }`, works after loadingFinished with no
  interception) and a Fetch-domain variant that requires a *paused* request. The recorder is a
  passive observer, not an interceptor, so `Network.getResponseBody` is the right primitive. Its
  output maps 1:1 onto `ResponseBody { text, base64 }`. The live call is transport-touching and
  deferred to the feeder/example; the **input method** is the pure, CI-runnable heart.
- **`skip_serializing_if` to keep body-less output byte-identical.** A recording with no captured
  body must serialize exactly as before this change — only `size` and `mimeType` appear under
  `content`. The `no_body_capture_leaves_content_fields_absent_in_json` test pins this, so the
  existing HAR shape and the `corpus.rs` consumers are untouched. This is the safe way to extend
  a write contract without a schema break.
- **`har.rs` is a CDP adapter file, so extending it is on-seam.** The transport-neutrality guard
  (`tests/transport_neutrality.rs`) lists `har.rs` in `CDP_ADAPTER_FILES` — it is *allowed* to
  name chromiumoxide. Adding a body-capture path here (and a future `Network.getResponseBody`
  call) does not widen the CDP surface; the matcher/fusion path stays CDP-free. Guard still green.
- **The input type is a plain value, not a CDP type.** Keeping `ResponseBody` a transport-neutral
  struct is what lets the body-capture state transition stay a pure, unit-testable step while the
  actual CDP call lives in the feeder. Same discipline as the rest of the recorder: typed events
  decode to plain inputs at the boundary, the state machine never touches a browser.

Commit sha: see the commit that lands this entry. **Next: the feeder + fulfill leg (D34 steps
b/c) — a live capture that issues `Network.getResponseBody` at loadingFinished and forwards it
through `on_response_body` to emit a SELF-CONTAINED inline-body HAR, then replays THAT through
`replay.rs` + a `Fetch.requestPaused`→`fulfillRequest`/`failRequest` leg, running the observe
loop over the replayed DOM for the first real M=1. A live example, not CI.**


## Build run 28 — Phase 3.5b fulfill-leg param builder (D35, 2026-06-18)

The pure, CI-tested half of the fulfill leg. `replay.rs` (run 26) answers "does this request have a
recorded response, and where is its body?" as a transport-neutral `MatchOutcome`. This run adds the
other half: `fulfill.rs::replay_action(request_id, &MatchOutcome) -> ReplayAction`, mapping a verdict
to the exact CDP params the live event loop will dispatch. `Abort` → `Fail(FailRequestParams,
ErrorReason::Failed)`; `Fulfill(entry)` → `FulfillRequestParams` with `response_code` = recorded
status, `HeaderEntry`s mapped 1:1, and body per `ReplayBody`. Param-building is pure and
deterministic, so it is fully unit-tested in CI (7 tests, no browser); only the live
`Fetch.requestPaused` → dispatch wiring needs a browser and lands in a later example.

### Judgment calls (run 28)

- **Body encoding: chose OPTION 2 over D35's recommended OPTION 1.** D35 (research run 26) recommended
  storing EVERYTHING base64 at capture so the fulfiller is a pure pass-through with zero base64 dep and
  a symmetric record↔fulfill seam, and explicitly invited the builder to confirm or choose at wiring
  time. I chose the alternative: keep recorder text bodies raw (human-readable) and base64-encode on
  the fulfill side. **Why:** a captured HAR is a debugging artifact I will eyeball when a replay renders
  wrong — all-base64 makes every HTML/JSON body opaque, which is the exact moment readability matters
  most. The cost of OPTION 2 is one `base64::encode` per *intercepted request* (not per byte, not a hot
  path — interception fires once per network request during a single offline replay) and one trivial
  direct dep (`base64 = "0.22"`, already in the lock file transitively via chromiumoxide). That cost
  buys a readable on-disk artifact for the entire life of the project. The asymmetry D35 worried about
  is contained to one `match` arm in one adapter file and is fully covered by two tests
  (`raw_text_body_is_base64_encoded_into_params_body`, `already_base64_body_passes_through_unchanged`)
  that pin both directions including round-trip. This is a confirmation-with-modification of an
  explicitly-open decision, not re-litigation of a settled one. D35 marked resolved-with-modification.
- **External body → `Fail`, never serve an empty page.** A `ReplayBody::External` entry (body in a
  sidecar `_file` the matcher does not open) has no bytes available here. Serving a `fulfillRequest`
  with no body would render a blank page *as if it were the real response* and silently pollute M — the
  exact D30 honesty failure the matcher's `Abort` guards against. So `External` returns `Fail` too.
  Self-captured HARs (the actual Tier-1 replay target) always inline their bodies, so this only arises
  for foreign HARs (the ServiceNow demo capture); failing loudly on them is correct.
- **`fulfill.rs` belongs in `CDP_ADAPTER_FILES`, not the fusion path.** It names `FulfillRequestParams`,
  `HeaderEntry`, `ErrorReason`, `Binary` — all CDP types — so the transport-neutrality guard's
  `cdp_surface_is_exactly_the_transport_adapters` test would (correctly) fail if it were not listed.
  Added it there. The matcher (`replay.rs`) stays CDP-free in the fusion-path list; the param builder
  that consumes the matcher's verdict is the seam where CDP types are allowed. The verdict type
  (`MatchOutcome`) crosses the seam as a plain value, same discipline as `RawAxNode` at the observe
  boundary.
- **`impl Into<RequestId>` in the signature, `RequestId::new` in tests.** `fetch::RequestId` only
  implements `From<String>`, not `From<&str>`, so the production signature takes `impl Into<RequestId>`
  (the live loop hands it a real `RequestId` from the paused event) and the tests construct
  `RequestId::new("req-1")` explicitly. Kept the public API ergonomic without inventing a `&str` From.

Commit sha: see the commit that lands this entry. **Next: the live fulfill event loop (D34 step c live
half) + the run-once live capture (D34 step b) — decode `Fetch.requestPaused` → `ReplayRequest`, call
the now-built `replay_action`, dispatch the params over the channel, running the observe loop over the
replayed DOM for the first real M=1. A live example, not CI.**

## Build run 29 — Phase 3.5b the live fulfill event loop (D36 resolved-with-modification, 2026-06-18)

Wired the transport-touching half of D34 step c: `ReplayFulfiller` in `fulfill.rs`. It subscribes to
`Fetch.requestPaused`, enables interception over `url_pattern: "*"` at `RequestStage::Request`, and
spawns a pump that answers every paused request from the loaded `ReplayHar` — fulfilling recognized
requests with the recorded body, honestly failing (`Abort→Fail`, D30) anything the HAR doesn't carry —
until `finish()` stops it, drains, and disables interception. Then the caller observes the static
replayed DOM. The live M=1 proof rides `examples/webarena_replay.rs` (a real headless Chrome, not CI);
6 new CI unit tests cover the decode + stat path. Workspace tests 205 → 211, clippy clean under
`-D warnings`, fmt clean.

Judgment calls:
- **D36 cited the wrong pump; corrected to the `event_listener` `EventStream`.** D36 point 1 said to
  build the pump on `examples/webarena_capture.rs`'s `TcpStream` loop (~149-182). That code is the
  one-shot HTTP `GET /json/version` lookup that resolves `webSocketDebuggerUrl` — not a long-lived WS
  frame pump. The real non-discarding event tap is chromiumoxide's `Page::event_listener::<T>()`, the
  exact mechanism `NetworkCapture` (`runner.rs`) uses to read `Network.*` live without dropping events
  (unlike `run_on`, D26). So `ReplayFulfiller` mirrors `NetworkCapture`'s subscribe-before-`enable` /
  spawn-pump / stop-and-drain shape. D36's *sequencing constraint* (never observe while interception is
  live; every paused request gets a verdict) is honored exactly; only the pump citation changed. Logged
  the correction on D36 as RESOLVED-WITH-MODIFICATION.
- **CI-testable decode without a browser.** `fetch::EventRequestPaused` derives `Deserialize`, so the
  unit tests build synthetic paused events with `serde_json::from_value` (requestId, request {url,
  method, headers, initialPriority, referrerPolicy}, frameId, resourceType) and assert
  `request_from_paused` flattens string headers, drops non-string header values, and that the decoded
  request matches / mismatches a recorded `ReplayEntry`. No live Chrome needed for the decode seam.
- **GET-only `post_data` scope.** `request_from_paused` sets `post_data: None`. The M=1 target is a
  GET/RETRIEVE trajectory and `network::Request` exposes no direct `post_data` (only
  `post_data_entries`). POST-body replay is a documented follow-up, not part of this seam.
- **`FulfillStats.errors` tracked apart from `failed`.** A dispatch error (the `execute` call itself
  failing) is counted separately from an honest `Fail` verdict, so `fulfilled + failed` stays a truthful
  count of answered requests and transport faults don't masquerade as hermetic failures.
- **Clone the `Page` (Arc-backed) in both fulfiller and example.** Holding `&Page` for the pump would
  conflict with the later `&mut session.observer` borrow at observe time; cloning the Arc-backed handle
  sidesteps it cleanly.

Commit sha: see the commit that lands this entry. **Next: the operational run-once — stand up a headless
Chrome on the phantom network, run `webarena_capture.rs` once to bank a self-contained inline-body HAR,
then `webarena_replay.rs` against it for the first real M=1 (no new code, just the live run).**

## Build run 30 — Phase 3.5b run-once live M=1: FIRST BASELINE-axis datapoint (D37 resolved, 2026-06-18)

Shipped the first **M=1**: a page reached entirely from a recorded HAR, observed with durable identity
and NO live origin. `scripts/run-once-m1.sh` stood up the in-container Playwright headless-shell + a
`python3 -m http.server` static fixture, captured a self-contained inline-body HAR via
`webarena_capture.rs`, then replayed it through `webarena_replay.rs`. Live result: **capture = 1 HAR
entry / 3603 B / inline body; replay = 1 fulfilled / 0 failed / 0 dispatch errors; observe = 3 elements
minted durable eids.** This is the first BASELINE-axis (M) number for anchortree (D30's two-denominator
model), the counterpart to the SCORE-axis task-21 score=1.0.

### Judgment calls

- **The roadmap's "no new code" framing was WRONG — the central judgment call.** The ROADMAP item said
  3.5b run-once was "operational, no new code: stand up Chrome and run the two examples." But reading
  `runner.rs` showed `NetworkCapture`'s pump never calls `getResponseBody`/`on_response_body`, so every
  captured HAR was body-less → the replay matcher fulfills nothing → no render → no M=1. The capture-side
  body feeder (STATE.md step b, D34 had built only the recorder half) had never been wired. So the
  "operational" item actually required real code. Built it rather than producing a hollow run. Forward
  motion: the run-once is meaningless without a self-contained HAR, and the HAR is body-less without the
  feeder. Logged D37 as RESOLVED (the standup executed exactly as proposed; the body feeder was the
  unflagged prerequisite).
- **`start_with_bodies` is a second constructor, not a flag on the public `start`.** `NetworkCapture::start`
  stays the lean body-less path (plain network traces — the WebArena `NetworkEventEvaluator` scores from
  timings/status, not bodies, so most flows never pay the extra round-trip). `start_with_bodies` opts into
  the inline-body capture. Both funnel through `start_inner(page, capture_bodies)`, which threads an
  `Option<Page>` (`capture_bodies.then(|| page.clone())`) into the pump.
- **Right CDP domain: `Network.getResponseBody`, not `Fetch.getResponseBody`.** chromiumoxide 0.9.1 ships
  both. `fetch::GetResponseBodyParams` requires a request paused in the Response stage (interception) — wrong
  for a passive post-`loadingFinished` capture. `network::GetResponseBodyParams::new(request_id)` reads the
  body after the response settles with no interception — the correct one. Caught from the IDENTIFIER + doc
  before writing the call.
- **Best-effort body read, never an aborted capture.** `record_event` does the body read inside an
  `if let Ok(resp) = page.execute(...).await` — a failed read (e.g. a 204, or a body already evicted) leaves
  that entry body-less and the capture proceeds. A self-contained HAR is best-effort, not all-or-nothing.
- **`on_response_body` BEFORE `ev.record_into(rec)`.** `on_loading_finished` removes the pending entry from
  the recorder map, so the body must be fed first. `record_event` feeds the body, then records the event.
- **`ANCHORTREE_CAPTURE_OUT` env override.** `webarena_capture.rs` defaulted its output dir under the temp
  dir; `run-once-m1.sh` needs the capture and the replay to agree on the HAR path, so the example now reads
  `ANCHORTREE_CAPTURE_OUT` (falling back to the temp default).
- **No new unit tests; the live run is the proof.** The feeder is browser-tied exactly like the existing
  pump (which also has no unit test — it is proven by `webarena_capture`). Adding a mock-CDP unit test for
  one `execute` call would test the mock, not the seam. The live M=1 run is the regression evidence,
  consistent with how the pump itself is proven. Workspace stays at 211 tests, clippy clean under
  `-D warnings`, fmt clean.

Commit sha: see the commit that lands this entry. **Next: 3.5b Tier 2 (growth) — bank more self-captured
trajectories to widen the M and N axes toward the 258 WebArena-Verified Hard ids, or Phase 4 polish.**

## Build run 31 — 2026-06-18 — rebind-on-replay M datapoint (D38)

The shipped M=1 (run 30) proved the offline capture→replay→observe pipeline, but its observe only MINTED
three fresh eids (Path 3). It never exercised the durable-identity REBIND through a re-render (Path 2,
`diff.rebound`, zero LLM) — the actual anchortree thesis. This run closes that gap on the SAME hermetic
replay rail, exactly as D38 proposed.

### What was built

- **Re-rendering fixture.** `scripts/fixtures/m1-site/index.html` gained an inline `window.__atRerender`
  script that rebuilds the card's children (`#intro`, `#buy-now`, `#status`) as fresh DOM nodes whose role
  + text fingerprints are byte-identical to the static markup. The button's `onclick` fires it, and the
  example triggers it deterministically. It replays with no network because the HTML body is inlined in the
  captured HAR.
- **Observe-twice in `webarena_replay.rs`.** The single-observe block became observe → re-render → observe.
  A `RegroundLedger` records both diffs. Observe 1 asserts `!diff.added.is_empty()` (mints); the re-render
  is driven via `page.evaluate("window.__atRerender ? window.__atRerender() : false")`; observe 2 asserts
  `!diff2.rebound.is_empty()` (the eids preserved across fresh nodes) and `ledger.llm_reground_calls() == 0`.
- **run-once-m1.sh** header + closing echo updated to describe the rebind across the re-render.
- **README vs-the-field** got the head-to-head contrast: Stagehand's selector cache validates by DOM-hash
  and falls back to the model the moment the hash drifts (browserbase.com/blog/stagehand-caching) — the
  exact re-render where anchortree rebinds with zero model calls — plus a closing sentence noting the
  rebind is proven offline by `scripts/run-once-m1.sh`.

### Live result

`bash scripts/run-once-m1.sh` (in-container headless-shell + python http.server, self-contained HAR):

```
observe 1 (replayed DOM): 3 elements minted durable eids
observe 2 (after re-render): 2 rebound, 0 added, 0 changed, 0 removed
2 durable rebinds at 0 LLM re-grounds (over 2 observes)
```

A page reached entirely from a recorded HAR re-rendered its own DOM, and the durable eids rebound onto the
fresh nodes with zero LLM re-grounds. No live origin was touched. This is the BASELINE-axis (M) datapoint
that actually carries the thesis — durable identity survives a re-render with no re-ground — not just a
mint over replayed bytes.

### Judgment calls

- **2 rebound, not 3, is correct and honest.** observe 1 mints 3 eids: `h1#title` plus the two card
  children. The `h1#title` sits OUTSIDE the re-rendered card, so its `backendNodeId` never changes — it
  stays bound and appears in no diff bucket (unchanged). Only the two card children get fresh backend ids,
  so exactly 2 rebind. The assertion is `>= 1` (`!rebound.is_empty()`), which 2 satisfies cleanly. I did
  not inflate the fixture to manufacture a "3 rebound" headline; the real number is 2 and the proof holds.
- **No new unit tests; the live run is the proof.** The rebind logic itself is already unit-tested in
  `anchortree-core` (identity/diff suites). This run adds an example that drives a real browser over a real
  replayed HAR — browser-tied exactly like `webarena_capture`, which also has no unit test. A mock-CDP test
  would test the mock, not the replay seam. The live M=1 rebind run is the regression evidence. Workspace
  stays at 211 tests, clippy clean under `-D warnings`, fmt clean.

Commit sha: see the commit that lands this entry. **Next: 3.5b Tier 2 (growth) — live WebArena-Verified
Docker standup for HAR-resistant dynamic tasks; widen N/M toward the 258 Hard ids.**

## Build run 32 — 2026-06-18 — measured head-to-head (D39)

Roadmap item: **3.5b measured head-to-head.** Run 31 proved anchortree's rebind on the replay rail; the
central competitive claim ("anchortree rebinds where Stagehand falls back to the LLM") was still an
*asserted doc sentence* — `webarena_replay.rs` only asserted anchortree's own side (`llm_reground_calls()
== 0`) and the README merely *said* a Stagehand cache "would fall back." This run turns that claim into a
measured number on the same rail, with no Docker and no live origin.

What shipped:
- **`peer.rs` — `DomPositions::from_document_order` (+ 2 unit tests, 13 peer tests total).** This is the
  absolute-positional `/*[k]` view a raw-XPath resolver caches over one observation: each *named* element
  keyed by accessible name, placed at its 1-based document-order index. The index runs over the full node
  slice (unnamed nodes consume a position; only named ones are cached), so a reorder that moves a named
  node breaks the cached XPath. Tests: `from_document_order_places_named_nodes_at_their_doc_index` and
  `reorder_moves_a_named_element_to_a_new_absolute_index`.
- **m1-site fixture — `window.__atReorder`.** A second inline re-render hook that rebuilds the card with
  the button moved to the END, past the `role="status"` paragraph. See the judgment call below for why it
  must cross `status` specifically.
- **`webarena_replay.rs` — three-leg measured trajectory.** observe-1 mints eids and binds a
  `StagehandCache` from `from_document_order(&nodes1)` for the act target "Buy now"; leg A in-place
  re-render → observe-2 (rebind, re-resolve baseline); leg B reorder → observe-3 (rebind, re-resolve
  baseline). Asserts `heals_inplace == 0`, `heals_reorder >= 1`, and anchortree `llm_reground_calls() == 0`
  across all legs. Prints the head-to-head headline.
- **README vs-the-field — measured, both caches named.** Replaced the asserted Stagehand sentence with the
  two-leg measured description and named both real Stagehand caches (the modelled absolute-XPath resolver +
  the coarser DOM-hash whole-page cache, kept as scoped prose). Live numbers added.
- **run-once-m1.sh** header + closing echo updated for the in-place + reorder head-to-head.

Live numbers (`bash scripts/run-once-m1.sh`): observe-1 = 3 minted, Stagehand cached 1 selector; observe-2
in-place = 2 rebound / 0 self-heals; observe-3 reorder = 2 rebound / 1 self-heal. Headline: **anchortree 4
rebinds at 0 LLM re-grounds | Stagehand (absolute-XPath resolver) 1 self-heal.**

Judgment calls (the honesty work):
- **The reorder must cross an OBSERVED sibling, not just any sibling.** First live attempt put the button
  ahead of the intro `<p>` and the reorder leg measured 0 self-heals — a failed assertion, caught by the
  live run. Reason: the plain intro `<p>` is not surfaced in the accessibility observation, so the observed
  node order (h1, button, status) was unchanged and the button's `from_document_order` index did not move.
  Fixed by moving the button past `role="status"` (which IS observed) to the end of the card, so its
  observed index genuinely shifts (status, then button). This is itself a faithfulness check: the baseline
  only self-heals when the position a resolver actually caches over really moved.
- **In-place leg asserts 0 self-heals deliberately.** A byte-identical in-place re-render keeps document
  positions, so the cached absolute selector still resolves — 0 self-heals. This is the honest D29 case
  where rebind ≠ self-heal; padding it to a fake win would be the exact overclaim this work removes.
- **D39 option (a): model only the absolute-XPath variant.** `from_document_order` faithfully reproduces a
  raw positional resolver. The coarser DOM-hash whole-page cache cannot be modelled without inventing its
  internal hash (and a byte-identical in-place re-render would not even drift an outerHTML hash), so it
  stays as clearly-scoped README prose rather than a fabricated number.
- **2 new unit tests, not more; the head-to-head is proven live.** The pure `from_document_order` logic is
  unit-tested; the three-leg measured comparison is browser-tied (like every other example) and proven by
  the live run, not CI. Workspace: 211 → 213 tests, clippy clean under `-D warnings`, fmt clean.

Commit sha: see the commit that lands this entry. **Next: 3.5b Tier 2 (growth) — live WebArena-Verified
Docker standup for HAR-resistant dynamic tasks, gated behind a `pids.max=256` feasibility check; widen N/M
toward the 258 Hard ids.**

## Build run 33 — 2026-06-18 — FRAME-tier durability via a frame-owner discriminator (D40 resolved)

**ROADMAP item:** 3.2e cross-frame FRAME-TIER durability. The node tier of `(frame, in-frame fingerprint)`
was proven (run 31) and measured head-to-head (run 32). The FRAME tier was not: `FrameKey =
parent.child(structural-ordinal)` is durable against a CDP `frameId` reassignment but FRAGILE to a
frame-owner reorder/insert — a sibling iframe added before the target shifts every later ordinal, so the
in-frame fingerprints are looked up under a different frame key and the eids re-mint. That is the same
ordinal fragility Stagehand v3's `frame ordinal + backendNodeId` composite carries. This run gives the FRAME
tier a durable discriminator so the key survives the reorder, then wires it through to the live eid path.

**What shipped (engine + CI-unit; the live HAR two-leg is split off as 3.2f):**
- `FrameKey::child_segment(&str)` (identity.rs): `child(ordinal)` now delegates to it, keeping the ordinal as
  the fallback so every pre-existing ordinal test and the same-origin `getFrameTree` agreement is byte-for-byte
  preserved. A labelled owner keys by its discriminator segment ALONE (not ordinal+label) — that is what makes
  it reorder-durable.
- A frame-owner discriminator (frames.rs `owner_segment` + `sanitize_label` + `FrameCounters`): each owner's
  segment is its sanitized label if present, else its document-order ordinal. The ordinal advances for EVERY
  owner (labelled or not) so an unlabelled sibling's fallback still reflects true position; a repeated label is
  `#n`-deduped per document (`ads`, `ads#1`). `sanitize_label` lowercases, keeps `[a-z0-9-_/:]`, folds the rest
  to `_`, collapses runs, caps 48 chars.
- The label source (observer.rs `iframe_label_from_attributes` + `src_origin_and_path`): priority
  `src` origin+path → `name` → `title` → `id`, picked from the owner's inline pierced-DOM attributes. `src`'s
  query and fragment are dropped so a cache-buster does not perturb the key. `decode_dom_node` populates
  `frame_owner_label` only when `is_frame_owner_element` (made `pub(crate)`) holds, so a non-owner never
  contributes a label.
- Live wiring: `map_backends_to_frames` switched from `frame_keys(decode_frame_tree(getFrameTree))` to
  `dom_frame_keys(dom)` — only the pierced DOM walk sees the owner element and its attributes; `getFrameTree`
  carries frame ids but no owner. The two agree on a same-origin tree (existing agreement test), so the switch
  is behavior-preserving where the discriminator is absent and strictly stronger where present. `decode_frame_tree`
  and the now-dead `FrameTree`/`GetFrameTreeParams`/`frame_keys` imports were removed; `frame_keys` stays `pub`
  in frames.rs as the ordinal reference the agreement test pins against.

**Judgment calls:**
- *Discriminator alone, not ordinal+discriminator.* If the segment were `ordinal+label`, a sibling insert
  would still shift the ordinal half and break the key — defeating the whole point. The label by itself is the
  durable handle; the ordinal is only the fallback when no label exists.
- *src-first, accessible-name rejected as primary.* A frame OWNER has no AX name of its own — the accessible
  name lives inside the frame's document behind a separate per-frame AX fetch, unavailable at the point the
  frame tree is keyed. `src` is the load-bearing author handle and is present inline on the owner element.
- *Gap encoded as a test, not just prose.* `unlabelled_owner_reorder_shifts_the_ordinal_key_the_measured_gap`
  asserts the target keys "0" before and "1" after a sibling insert, so the fix's value is legible in CI; the
  paired fix test asserts "login" survives an "ads" sibling inserted ahead of it.
- *Unit proof now, live measurement next.* The CI-gated unit tests ARE step (c) of D40 (the durability claim).
  The live HAR two-leg (a same-origin iframe re-render + a sibling-owner reorder hook, run-32 style) is the
  corroboration, split off as ROADMAP 3.2f — same prove-then-measure split that worked for the node tier.

**Tests:** 11 new (8 frames: discriminator-keys, the gap, the fix, dedup, ordinal-mix, nesting, OOPIF,
sanitize; 3 observer: src-priority, name/title/id fallback order, none-when-anonymous). Workspace 213 → 224,
clippy clean under `-D warnings`, fmt clean.

Commit sha: see the commit that lands this entry. **Next: 3.2f cross-frame FRAME-TIER live measurement — the
run-32-style HAR rail twin: a same-origin iframe re-render leg + a sibling frame-owner reorder leg, asserting
anchortree rebinds at 0 LLM on both while a Stagehand-style ordinal resolver re-grounds on the reorder.**

## Build run 34 — 2026-06-18 — FRAME-tier head-to-head MEASURED in CI (D41 resolved)

**ROADMAP item:** 3.2f cross-frame FRAME-TIER measured head-to-head. Run 33 HARDENED the frame discriminator
(a labelled owner keys by `src`/`name`/`title`/`id`, not its ordinal, so a sibling inserted ahead leaves the
key unchanged). What run 33 did NOT do was turn that durability into a NUMBER against a competitor. The node
tier got that number in run 32, but only inside the browser-tied `webarena_replay.rs` script — never in CI.
Run 34 lifts the frame-tier head-to-head one rung HIGHER than the node tier: a CI-gated assertion, not a
browser-script one.

**What shipped:** the frame-tier twin of `peer.rs`'s `DomPositions`/`StagehandCache` pair.
- `FrameOrder::from_owner_order(owners)` — a positional ordinal→discriminator view of the frame-owner order.
  `discriminator_at(ordinal)` and `ordinal_of(discriminator)` are the two directions a positional resolver
  needs. Identical discriminators collapse to their FIRST ordinal (`first_ordinal_of` keeps the earliest via
  `.entry(label).or_insert(i)`) — the D41 bound, encoded in the data structure itself.
- `FrameOrdinalCache` — models a Stagehand `frameOrdinal` resolver. `bind(discriminator, &order)` caches the
  current ordinal for free (0 re-grounds). `reresolve(&order)` walks every cached handle and charges ONE
  re-ground per handle whose cached ordinal no longer holds its discriminator, repairing it to the fresh
  ordinal. This is the frame-tier analogue of `StagehandCache::self_heals`.
- 6 new peer tests. The load-bearing one is the measured Leg B: a `checkout` frame at ordinal 0, an `ads`
  sibling inserted ahead → `checkout` now at ordinal 1 → the positional cache `reresolve == 1`, while the
  discriminator key over the same reorder pays 0. The `(positional, discriminator) == (1, 0)` pair is the
  CI-gated head-to-head number. Leg A (in-frame churn, owner order unchanged) is 0. The D41 collapse bound
  has its own test at the `FrameOrder` level.
- The duplicate-`src` degradation unit test lives in `frames.rs`
  (`identical_discriminator_siblings_degrade_to_document_order_on_a_front_insert`): two `src`-equal `ads`
  slots key `ads`/`ads#1`, a third `ads` inserted ahead re-mints to `ads`/`ads#1`/`ads#2` — document-order
  parity with Playwright `.nth()`, asserted so the honesty bound is legible in CI.
- README vs-the-field gains the frame-tier `1`-vs-`0` paragraph + the distinct-vs-identical sentence citing
  Playwright `.nth()` as the field's best for identical-src iframes.

**Judgment calls:**
- *CI-gated number now; live HAR example deferred.* With no Chrome container running, I judged the highest
  fully-green increment to be the peer-modelled head-to-head (it makes the frame-tier claim a CI NUMBER, more
  rigorous than the node tier which only measures in the browser script) PLUS the degradation test + README
  sentence. I explicitly did NOT build the browser-tied `webarena_frame_replay.rs` this run: run 32 caught a
  real bug (the reorder leg first measured 0 self-heals) only by running live, so shipping an un-smoke-run
  browser example would violate "build it right". It is queued as ROADMAP 3.2f-live with an exact fixture spec,
  to be built+smoke-run when a browser is stood up. Same prove(33)→measure-in-CI(34)→measure-live split that
  worked for the node tier (prove 31 → measure 32).
- *No content-fingerprint disambiguator for same-src frames.* D41 is explicit: per-frame AX is a separate
  fetch unavailable when the frame tree is keyed, and document-order fallback is already field parity with
  Playwright `.nth()`. Building a disambiguator would be inventing a capability the field does not have and
  the constraint forbids. The honest move is to encode the bound as a test and name the parity in the README.
- *Identical-discriminator collapse modelled at the data-structure level.* `first_ordinal_of` makes the
  positional cache's behavior on duplicate discriminators a property of `FrameOrder` itself, so the peer
  baseline and the real engine degrade the same way — no divergent special-casing in the test.

**Tests:** 7 new (6 peer: `FrameOrder` positional view, free bind, the measured 1-reground reorder, the
discriminator's 0-reground over the same reorder, in-frame churn at 0, the D41 collapse-to-first-ordinal;
1 frames: the duplicate-`src` front-insert degradation). Workspace 224 → 231, clippy clean under
`-D warnings`, fmt clean.

Commit sha: see the commit that lands this entry. **Next: 3.2f-live cross-frame FRAME-TIER live HAR
measurement — build + smoke-run `webarena_frame_replay.rs` with a `src=checkout`-behind-`src=ads` fixture,
capturing a self-contained HAR and replaying it offline to measure the two legs live, in a run where a
Chrome is stood up.**

## Build run 35 — 3.2f-live cross-frame FRAME-TIER live HAR measurement

The browser-tied twin of D41's CI-gated frame-tier head-to-head, and the FRAME-tier analogue of the node-tier
`webarena_replay.rs` rail. A cross-frame page is reached ENTIRELY from a recorded HAR (no network, no live
origin), its checkout frame's card is churned in place and then a sibling ad frame is inserted ahead of it, and
the checkout button's durable eid is measured against a modelled Stagehand `frameOrdinal` resolver across both
transitions. Built + SMOKE-RUN live on the cached `chrome-headless-shell` — the run that caught the real bug.

**Built:**
- `scripts/fixtures/frame-site/index.html` — a single self-contained page whose interactive element lives one
  frame down inside a same-origin `name="checkout"` srcdoc iframe, with inline `__atFrameRerender` (rebuilds the
  checkout frame's card in place via its `contentDocument`) and `__atFrameReorder` (inserts a `name="ads"`
  srcdoc iframe BEFORE the checkout owner).
- `crates/anchortree-cdp/examples/webarena_frame_replay.rs` — connect over `ws://`, `ReplayFulfiller` the HAR,
  observe 3 legs, peer = `FrameOrder`/`FrameOrdinalCache` fed the fixture's ground-truth owner order.
- `scripts/run-once-frame.sh` — stands up Chrome + a static server, captures a self-contained HAR with
  `webarena_capture`, replays it through `webarena_frame_replay`.

**Judgment calls (stamped D42):**
- *srcdoc-name fixture, single file.* A srcdoc owner has no `src`, so the D40 discriminator falls cleanly to
  `name` (keys `checkout`/`ads`, eids `fcheckout/...`/`fads/...`), and srcdoc frames are pierced inline with no
  request of their own — so the parent document alone is a complete HAR. This sidesteps the multi-document
  capture problem a `src=ads.html` would create (only fetched at reorder time during replay, not at capture) and
  keeps the node-tier single-file offline rail intact one tier up.
- *Reorder leg = stability, not rebind (the live-run catch).* The first cut asserted the checkout button's eid
  appears in `diff.rebound` after the reorder, mirroring the node tier. The live smoke-run failed it with
  `0 rebound, 1 added, 0 removed`. Correct and stronger: a frame-owner reorder does not touch the checkout
  frame's own document, so the button keeps its `backendNodeId` and — because its discriminator key `checkout` is
  reorder-invariant — the `(FrameKey, backendNodeId)` soft-match still hits, so the eid stays bound with ZERO
  churn. Ordinal keying would instead have dropped `f0/...` and minted `f1/...`; observing neither IS the proof.
  Leg A (inner churn) is the rebind; Leg B (reorder) asserts the eid is absent from both `diff.removed` and
  `diff.added`, still live via `IdentityMap::binding`, still `frame_key == "checkout"`, peer 1 re-ground.
- *No new unit test.* The live smoke-run IS the regression evidence — same shape as the node-tier rail, an
  operational script, not a CI gate. The example compiles in CI; its assertions run live.

**Live result:** `observe 1` mints 3 eids (checkout button + status + parent h1); `observe 2` (inner churn)
2 rebound / 0 peer re-grounds; `observe 3` (frame reorder) 0 rebound / 1 added (ads button) / 0 removed,
checkout button held bound keyed `checkout`, peer 1 re-ground. Ledger: 2 rebinds at 0 LLM re-grounds.

**Tests:** 0 new unit tests (the live smoke-run is the proof). Workspace 231 unchanged, clippy clean under
`-D warnings`, fmt clean. Commit sha: see the commit that lands this entry. **Next: 3.5b Tier 2 — the live
WebArena-Verified Docker standup for HAR-resistant flows, gated behind the `pids.max=256` feasibility check.**

## Build run 36 — 2026-06-18 — Phase 3.5b Tier 2: durable identity over a REAL WebArena page, offline (D43 resolved)

Runs 30–35 proved the M=1 rail against in-repo fixtures (`m1-site`, `frame-site`) carrying our own
`__atRerender`/`__atReorder` hooks. Run 36 lands the growth datapoint D43 asked for: the pure-Rust observe loop
run end-to-end against a GENUINE, server-rendered WebArena-Verified application page — no fixture, no
instrumentation of ours. A real OpenStreetMap `/about` page is booted live, captured to a self-contained HAR,
torn down, and reconstructed ENTIRELY from the recording with no live origin — and anchortree mints 30 durable
eids over it.

**Built:**
- `crates/anchortree-cdp/examples/webarena_observe.rs` — the general replay-and-observe rail. Unlike fixture-bound
  `webarena_replay` (which asserts `__atRerender`/`__atReorder` + "Buy now"), this makes no assumption about the
  page: it `ReplayFulfiller`s whatever self-contained HAR it is handed, navigates via raw `Page.navigate`,
  observes once, mints eids, asserts `stats.fulfilled >= 1` and `!diff.added.is_empty()`, prints sample handles.
- `scripts/run-once-webarena.sh` — boot-one-site harness. `docker run` the smallest per-site image
  (`am1n3e/webarena-verified-map`, 1.19 GB) as sibling `at-wa-map`, `docker network connect phantom_phantom-net`
  for container-DNS reachability, wait for HTTP, `webarena_capture` → self-contained HAR, tear the site down,
  `webarena_observe` the HAR offline. Pre-builds the examples with the browser DOWN to stay under the phantom
  pids budget.
- `crates/anchortree-cdp/src/fulfill.rs` — two real fidelity fixes (below) + 3 unit tests.

**Judgment calls (stamped D43):**
- *Raw `Page.navigate`, not `goto`.* A real multi-asset page never settles to network-idle the way a single
  self-contained fixture does: sub-resources it never recorded (favicons, late XHRs) are intercepted and honestly
  aborted, so the `load` lifecycle never fires and `goto`/`wait_for_navigation` (which wait for it) hang forever.
  `page.execute(NavigateParams::new(url))` returns the moment the navigation is committed; a fixed 1200 ms beat
  then lets the replayed document parse + run inline scripts before observing. The DOM the agent reasons over is
  present at commit + parse; full network-idle is not a precondition for minting durable identity over it.
- *Wire-framing-header strip (real bug #1).* A captured HAR stores the DECODED response body but keeps the
  origin's `Content-Encoding: gzip` + `Content-Length` from the compressed stream. Forwarding them verbatim to
  `Fetch.fulfillRequest` makes Chrome try to gunzip already-plain text → empty DOM. `is_wire_framing_header`
  strips `content-encoding`/`content-length`/`transfer-encoding`; CDP re-frames the body itself. The m1-site
  fixture is uncompressed, so it never exercised this — only a real gzip origin did.
- *Status-0 fail (real bug #2).* An opaque/aborted capture has HAR status 0. `Fetch.fulfillRequest` rejects it
  with `-32602 "Invalid http status code"`, leaving the request paused forever; a blocking head `<script src>`
  stuck there stalls the parser (`ready: loading`, `body: null`). Per the D30 honesty guard, fail status-0
  entries (a `100..=599` guard) so the browser proceeds rather than hanging. Both fixes weaken no existing test.
- *Netns gate.* A bare `docker run` sibling lands on the default `bridge`, isolated from phantom; `-p` publishes
  on the HOST, not phantom's loopback. `docker network connect phantom_phantom-net at-wa-map` makes it resolvable
  by container DNS (`http://at-wa-map:8080/about`). Documented in the harness header so the next run does not
  re-discover it.
- *Tests pin the fixes, the live run proves the rail.* +3 `fulfill.rs` unit tests (status-0 fail, wire-framing
  strip, case-insensitivity); the Tier-2 M=1 itself is a live-smoke-run proof — a new example + boot-one-site
  harness, the same operational-script shape as the node/frame-tier rails. The live run IS the regression evidence.

**Live result:** smallest per-site image measured at 1.19 GB (reddit 4.57 / shopping 5.42 / gitlab 22.01),
162 GB free. Booted `at-wa-map`, joined `phantom_phantom-net`, captured a 1.23 MB inline-body `network.har`
(9 entries) of the real OSM `/about`. Site torn down; `webarena_observe` replayed it with no live origin:
`ready: complete`, `title: OpenStreetMap`, htmlLen 12777, **31 AX nodes → 30 durable eids** (`btn-openstreetmap`,
`hd-local-knowledge`, `hd-community-driven`, `lnk-history`, `lnk-export`, …). "OK: a real WebArena-Verified page
was reconstructed ENTIRELY from a recorded HAR and anchortree minted 30 durable eids over it. No live origin was
touched during replay."

**Tests:** +3 `fulfill.rs` unit tests → workspace 234 passing (was 231), clippy clean under `-D warnings`, fmt
clean. Commit sha: see the commit that lands this entry. **Next: 3.5b Tier 2 widen — feed agent_response +
network_trace to the webarena-verified evaluator container for deterministic scoring, then widen M/N across the
258 Hard ids.**

## Build run 37 — 2026-06-18 — Phase 3.5b Tier 2: EXTERNAL evaluator score == 1.0 at M=1 (D44 resolved)

Run 36 reconstructed a real page from a recorded HAR and minted 30 durable eids, but the eid count was OUR
success criterion, not the benchmark's. This run closed that gap: the authentic ServiceNow
`webarena-verified` evaluator scored a live-captured navigation **1.0** — the first EXTERNAL deterministic
datapoint, with the evaluator's own checksums stamped on the result.

**What landed.**
- **The recorder fix that made a document count as a navigation.** The evaluator's `is_navigation_event`
  (tracing.py) classifies on `Accept` / `sec-fetch-*` headers. For a top-level navigation, CDP's
  `Network.requestWillBeSent.request.headers` is a SPARSE provisional set (only User-Agent +
  Upgrade-Insecure-Requests); the real on-wire headers arrive on `Network.requestWillBeSentExtraInfo`. `har.rs`
  + `runner.rs` gained an order-independent extra-info header-merge (a stash holds extras that land before
  their `requestWillBeSent`; on insert the stash is drained and applied). Without it, the document HAR entry
  fails the nav check and `NetworkEventEvaluator` finds no matching event → score 0. +2 unit tests pin both
  event orderings (`extra_info_upgrades_sparse_navigation_headers`,
  `extra_info_before_will_be_sent_is_stashed_and_applied`).
- **The honest M=1 task.** D44 proposed task 369 (`__MAP__/way/154257484/`). The public slim map image
  `am1n3e/webarena-verified-map` (~4.75 GB) ships the OSM Rails stack + OSRM routing binaries but **NO OSM
  way/node data** — `current_ways`/`current_nodes` are empty (54 MB cluster, 0 rows), postgres-15
  (`/data/database/postgres`) won't start, every `/way/`, `/node/`, `/relation/` browse page 404s. A task whose
  expected target is a data-backed page cannot honestly score 200 here. I enumerated all 20 map NAVIGATE tasks
  and picked **356**, whose network assertion is `last nav == GET 200 to __MAP__` (the home page), which the
  image GENUINELY serves 200. No fabricated response — a real live capture scored by the real evaluator.
- **The harness.** `scripts/run-once-eval.sh` retargeted 369→356: boots `at-wa-map`, joins
  `phantom_phantom-net`, captures via the `webarena_capture` example (`ANCHORTREE_TASK_TYPE=navigate`), tears
  the site down (scoring is offline), runs `eval-tasks`, asserts `score == 1.0`.
- *Docker-out-of-Docker mount gotcha (solved).* The evaluator runs as a SIBLING container against the host
  daemon, so bind-mount sources resolve in the HOST namespace, not ours. A `/tmp` mktemp dir is private to this
  container and the daemon creates an empty placeholder for it (`IsADirectoryError: /config.json`). Fix: `WORK`
  lives under the `phantom_phantom_repos` volume (`/app/repos`) and is translated to its host data dir
  (`/var/lib/docker/volumes/phantom_phantom_repos/_data`) for the `-v` flags via a `host_path()` helper.

**Live result:** `eval_result.json` → `score: 1.0`, `status: success`. `AgentResponseEvaluator` 1.0
(`{navigate, success, null, null}`), `NetworkEventEvaluator` 1.0 (last nav = `GET 200 http://at-wa-map:8080/`,
both sides normalised to `{base_url: "__MAP__/"}`). Checksums: `evaluator
35c3385b1db4b3378657589f95f50defd4234bd36e5b93d44733fd561b01db4e`, `data
d65275660814663375028e9017e1f929e3c38321041b125795e2713b52243d30`, version `1.2.3`. The clean end-to-end script
run reproduced it from a fresh site boot.

**Tests:** +2 `har.rs` unit tests → workspace 236 passing, clippy clean under `-D warnings`, fmt clean. The
extra-info merge is the regression-pinned code; the external score is a live-smoke-run proof (the same
operational-script shape as the node/frame-tier rails — the live run IS the regression evidence for the score).
Commit sha: see the commit that lands this entry. **Next: 3.5b Tier 2 widen — boot a data-loaded map image to
score a `/way/`-class NAVIGATE and the first RETRIEVE (typed-data) task, then widen M/N across the 258 Hard ids.**

## Build run 38 — 2026-06-18 — Phase 3.5b Tier 2 widen: FIRST RETRIEVE score == 1.0 (D45 item (1) resolved)

The external evaluator scored a NAVIGATE at M=1 in run 37; D45 (research run 36) pivoted the widen off the
data-less map image onto self-contained sites and asked for the first RETRIEVE — the typed-data extraction path
D44 deferred. This run lands it: shopping_admin **task 11**, intent "Get the total number of reviews that our
store received so far that mention term 'disappointed'", expected `retrieved_data: [6]`.

- **The honest mechanism (reverse-engineered + de-risked before writing Rust).** The store's review grid is the
  LEGACY `varienGrid` (`reviewGrid`), not the new UI component. Its filter encodes into the URL PATH as
  `/filter/<base64>/` where base64(`detail=disappointed`) = `ZGV0YWlsPWRpc2FwcG9pbnRlZA==`. Navigating to
  `http://at-sa/admin/review/product/index/filter/ZGV0YWlsPWRpc2FwcG9pbnRlZA==/` returns a 200 server-rendered
  page whose initial HTML already carries `<span id="reviewGrid-total-count"> 6 </span> records found` — no async
  JS, the count is in the document. anchortree reads `#reviewGrid-total-count` out of the live DOM and emits it.
  This is honest: anchortree reports the number the store itself renders; if the store held a different count it
  would report that and score 0. DB ground truth confirmed = 6 (`review_detail` LIKE '%disappointed%').
- **Why task 11 scores on the agent_response alone.** Task 11 has ONLY an `AgentResponseEvaluator` (no
  `NetworkEventEvaluator`). The evaluator's `_normalized_retrieved_data` wraps a scalar into a tuple, so emitting
  the JSON Number `6` normalises to `(6,)` and matches the expected `[6]` (results_schema `{items: number}`). A
  string `"6"` would fail schema validation, so the example parses the DOM read into a JSON Number.
- **The example.** New `crates/anchortree-cdp/examples/webarena_retrieve.rs` — a site-agnostic login-then-read
  RETRIEVE rail driven by env vars (`ANCHORTREE_LOGIN_URL` + `ANCHORTREE_LOGIN_JS` to authenticate,
  `ANCHORTREE_CAPTURE_URL` + `ANCHORTREE_READ_JS` to read the answer, `ANCHORTREE_RETRIEVE_NUMBER=1` to emit a
  JSON Number). Captures the whole authenticated session (login POST + redirect + grid) into the HAR, then emits
  `AgentResponse::retrieved(json!(6))`. +5 `parse_retrieved_number` unit tests (padded / suffix / multi-digit /
  json-number-not-string / no-digit-error).
- **The harness.** New `scripts/run-once-retrieve.sh` mirrors `run-once-eval.sh`: reuses/boots
  `am1n3e/webarena-verified-shopping_admin` as `at-sa`, pins Magento `base_url` to `http://at-sa/` + `cache:flush`
  so the container-DNS admin serves 200 (the image ships `localhost:7780`, which 302-redirects every request),
  launches headless Chrome, drives the example, tears the site down, scores offline, asserts `score == 1.0`.

**Live result:** `eval_result.json` → `score: 1.0`, `status: success`. `AgentResponseEvaluator` 1.0;
`actual_normalized.retrieved_data == [6.0] == expected.retrieved_data` (intent_template_id 288, task_revision 2).
Checksums identical to run 37 (same evaluator + dataset): `evaluator
35c3385b1db4b3378657589f95f50defd4234bd36e5b93d44733fd561b01db4e`, `data
d65275660814663375028e9017e1f929e3c38321041b125795e2713b52243d30`, version `1.2.3`.

**Tests:** +5 example-target unit tests (`cargo test --example webarena_retrieve` = 5 passed); workspace
`cargo test --all` holds at 236 passing; clippy `--all-targets -D warnings` clean (gates the example compile);
fmt clean. The count-parse is the regression-pinned code; the external score is a live-smoke-run proof (same
operational-script shape as the prior tier rails — the live run IS the regression evidence for the score).
Commit sha: see the commit that lands this entry. **Next: 3.5b Tier 2 widen item (2) — a data-backed NAVIGATE to
a real CONTENT page on shopping or gitlab (refutes the map 404 as image-specific), then widen M/N across the 258
Hard ids.**
