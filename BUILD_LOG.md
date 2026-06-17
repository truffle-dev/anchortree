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
