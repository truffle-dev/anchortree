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
