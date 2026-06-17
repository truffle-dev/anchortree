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
- [ ] 1.5a End-to-end demo binary over **local `ws://`** (zero TLS, per D10):
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
- [ ] 1.5b `wss://` / Browserbase lift (D8 → D10): reach a TLS CDP endpoint by
  forcing rustls onto the **ring** crypto provider (ring compiles on this box;
  aws-lc needs cmake+nasm we lack). Feature surgery to purge `aws-lc-rs` from
  `hyper-rustls` / `rustls-platform-verifier` defaults. Deferred behind 1.5a.

## Phase 2 — "alive" deliverable (week 4 target)

- [ ] 2.1 Action space: `click(eid)`, `type(eid, text)`, `select(eid, option)`
  resolved through the IdentityMap to live CDP nodes.
- [ ] 2.2 Set-of-marks / screenshot fallback for elements with no clean
  accessible identity.
- [ ] 2.3 Token-budget guardrails: ≤5K baseline observation, ≤800 per diff.
  Add a measuring test.
- [ ] 2.4 A `README` quickstart an agent can copy-paste to drive a page.
- [ ] 2.5 (candidate, from run-3 Lightpanda scan) Sharpen
  `fuse::observable_backends()` keep-policy: pure ARIA-role filtering misses
  "actually clickable" elements with no semantic role. Lightpanda infers
  interactivity from bound `click`/`mousedown`/`change` listeners; the Chromium
  equivalent is `DOMDebugger.getEventListeners` per backendNodeId. Use as a
  *secondary* keep-signal layered on the role filter, not a replacement. Cite
  when hardening the keep-filter; not near-term.

## Phase 3 — breadth (weeks 5-8)

- [ ] 3.1 Cloudflare deploy target decided + a thin control-plane example
  (Browser Run or Container + Lightpanda image).
- [ ] 3.2 Multi-frame / iframe identity. (Prior art: Stagehand v3 stitches a
  combined AX tree with per-frame `EncodedId = frame-ordinal+node-id`; mirror
  the frame-ordinal idea but keep our ids *durable*, not snapshot-scoped.)
- [ ] 3.3 Benchmark harness: anchortree-stable-id vs. raw Playwright-MCP on a
  re-render-heavy task suite. Publish numbers. (Headline metric to beat:
  Stagehand re-grounds via LLM on any structural change — count the LLM calls
  / tokens we save by rebinding instead.)
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
