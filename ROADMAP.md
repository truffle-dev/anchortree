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
- [ ] 1.2 `anchortree-cdp` crate: connect via `chromiumoxide`, run one
  accessibility + DOM + layout pass, produce `Vec<ObservedNode>`. Keep
  `anchortree-core` browser-free behind a trait. Smoke against a real
  Browserbase session.
- [ ] 1.3 `ElementState` extraction from CDP (enabled/checked/expanded/value/
  visible) wired into `ObservedNode`.
- [ ] 1.4 Structural-path builder: derive `structural_path` from the
  interactive ancestry during the CDP pass.
- [ ] 1.5 End-to-end demo binary: connect, observe twice across a real SPA
  re-render, print the `Diff`, assert eids survived.

## Phase 2 — "alive" deliverable (week 4 target)

- [ ] 2.1 Action space: `click(eid)`, `type(eid, text)`, `select(eid, option)`
  resolved through the IdentityMap to live CDP nodes.
- [ ] 2.2 Set-of-marks / screenshot fallback for elements with no clean
  accessible identity.
- [ ] 2.3 Token-budget guardrails: ≤5K baseline observation, ≤800 per diff.
  Add a measuring test.
- [ ] 2.4 A `README` quickstart an agent can copy-paste to drive a page.

## Phase 3 — breadth (weeks 5-8)

- [ ] 3.1 Cloudflare deploy target decided + a thin control-plane example
  (Browser Run or Container + Lightpanda image).
- [ ] 3.2 Multi-frame / iframe identity.
- [ ] 3.3 Benchmark harness: anchortree-stable-id vs. raw Playwright-MCP on a
  re-render-heavy task suite. Publish numbers.

## Phase 4 — polish + reach (weeks 9-16)

- [ ] 4.1 Crate published to crates.io.
- [ ] 4.2 Project page + docs site on truffleagent.com.
- [ ] 4.3 Blog post + dev.to crosspost on the identity thesis with benchmark
  data.

## Exit condition (by week 3)

If the durable-identity rebind does not measurably beat naive re-grounding on
the benchmark suite (Phase 3.3 preview), reassess the thesis before investing
in breadth.
