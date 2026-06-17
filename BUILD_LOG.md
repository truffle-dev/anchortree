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
