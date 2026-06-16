# Agent-First Browser Interface — Technical Design

**Status:** Draft v1 · 2026-06-16
**Audience:** an engineer building this, not an investor reading a pitch.

---

## Core insight (one sentence)

The determinism an LLM agent lacks in a browser is an **identity** problem, not a rendering problem: give every interactive element an ID that survives the agent's own mutations, expose only the interactive-semantic slice of the page, and report changes as deltas — then the agent can act reliably and cheaply on top of any CDP-speaking browser.

---

## What we refuse to build

This is the **interface layer**, not the infra layer. We explicitly do **not** build:

- **A browser, a browser fleet, or browser-as-a-service.** That space is funded and crowded — Browserbase ($40M), Kernel ($22M), plus Cloudflare Browser Rendering and Lightpanda. We sit *over* any CDP endpoint they expose.
- **A general agent / planner / model.** No LLM loop, no task planner, no "agent." We are the perception+action primitive a framework (Browser Use, Stagehand, a bespoke loop, an MCP host) calls into.
- **A Playwright/Puppeteer replacement.** We speak CDP directly, but we are not a general scripting automation API. We are deliberately narrow: durable IDs, semantic snapshot, diffs, structured actions.
- **A scraper, a CAPTCHA solver, or an anti-bot tool.**
- **A visual/computer-use vision stack.** Screenshots and set-of-marks are a *fallback*, gated behind the a11y path, not the primary channel.

The bet: four properties that the literature keeps re-deriving but nobody ships cleanly as a single primitive — durable identity, interactive-only semantic model, event-sourced diffs, structured action space.

---

## Architecture overview

```
┌──────────────────────────────────────────────────────────────┐
│  Consumer:  framework loop  |  MCP host (Claude/Cursor/etc.)   │
└───────────────┬───────────────────────────┬──────────────────┘
                │ library API                │ MCP stdio/SSE
        ┌───────▼───────────────────────────▼────────┐
        │                CORE                          │
        │  ┌────────────┐  ┌──────────────────────┐   │
        │  │ IdentityMap │  │ SemanticModel         │  │
        │  │ eid ⇄       │  │ (interactive-only AX  │  │
        │  │ backendNode │  │  + DOM fusion)         │  │
        │  │ + fingerprint│ └──────────────────────┘   │
        │  └────────────┘  ┌──────────────────────┐   │
        │  ┌────────────┐  │ DiffEngine            │   │
        │  │ ActionExec  │  │ (baseline → deltas)   │   │
        │  └────────────┘  └──────────────────────┘   │
        │  ┌────────────────────────────────────────┐ │
        │  │ FallbackRenderer (set-of-marks/screenshot)│
        │  └────────────────────────────────────────┘ │
        └───────────────┬─────────────────────────────┘
                        │ CDP (chrome-remote-interface / raw WS)
        ┌───────────────▼─────────────────────────────┐
        │ Any CDP endpoint:                            │
        │  local Chrome • Lightpanda • Browserbase •   │
        │  Cloudflare Browser Rendering                 │
        └──────────────────────────────────────────────┘
```

### The CDP call flow

**On attach / navigation (build a baseline):**

1. `Target.attachToTarget` (flatten mode) → session. Subscribe to child targets for cross-origin iframes (`Target.setAutoAttach {autoAttach:true, flatten:true}`).
2. `DOM.enable`, `Accessibility.enable`, `Page.enable`, `Runtime.enable`, `CSS.enable` (CSS only when visibility/layout filtering needs computed style).
   - `Accessibility.enable` is load-bearing: it "causes `AXNodeId`s to remain consistent between method calls." ([Accessibility domain](https://chromedevtools.github.io/devtools-protocol/tot/Accessibility/))
3. `Accessibility.getFullAXTree` → the semantic spine. Each `AXNode` carries `backendDOMNodeId` linking it to a DOM node. ([Accessibility domain](https://chromedevtools.github.io/devtools-protocol/tot/Accessibility/))
4. `DOMSnapshot.captureSnapshot {computedStyles:[...]}` → one flattened array with layout boxes + computed style + `backendNodeId` per node, including iframes/shadow DOM flattened. This is the layout/visibility oracle and is far cheaper than walking `DOM.getDocument` deeply. ([DOMSnapshot domain](https://chromedevtools.github.io/devtools-protocol/tot/DOMSnapshot/))
5. **Fuse** AX tree (semantics) + snapshot (geometry/visibility) keyed on `backendNodeId`. Produce the interactive-only set. Mint/refresh `eid`s in IdentityMap.
6. Emit `<baseline>` observation.

**On each action (emit a diff):**

1. Resolve the target `eid` → `backendNodeId` via IdentityMap.
2. `DOM.pushNodesByBackendIdsToFrontend {backendNodeIds:[id]}` → fresh `nodeId` (frontend IDs are session-scoped and die on `documentUpdated`, so we always re-push at action time rather than cache `nodeId`). ([DOM domain](https://chromedevtools.github.io/devtools-protocol/tot/DOM/))
3. Execute via the structured action (`Input.dispatchMouseEvent` / `Input.dispatchKeyEvent` / `DOM.focus` + `Input.insertText`, or `Runtime.callFunctionOn` against the resolved object for native setters).
4. Wait for stabilization (network-idle heuristic + mutation-quiescence; see DiffEngine).
5. Recompute interactive set; compute delta against last baseline; emit `<diff>` with `added` / `removed` / `changed` / transient `observations`.

---

## Primitive 1 — Durable element identity

### The problem with the state of the art

- **Playwright MCP `ref`s are per-snapshot.** "Refs are stable within a single snapshot... After navigation or DOM updates, the tool returns a fresh snapshot with new refs." ([Playwright MCP snapshots](https://playwright.dev/mcp/snapshots)) An agent that re-snapshots after typing gets a *new* ref vocabulary every step — the identity resets constantly.
- **vercel-labs `agent-browser` `@e2` refs** are likewise snapshot-scoped; its own docs say to "take a fresh snapshot before retrying." ([vercel-labs/agent-browser](https://github.com/vercel-labs/agent-browser))
- **Browser Use** moved off Playwright to raw CDP (`cdp-use`) and uses an `EnhancedDOMTreeNode` "super-selector" combining `targetId`, `frameId`, `backendNodeId`, x/y, and an `element_index` ordinal. ([Closer to the Metal](https://browser-use.com/posts/playwright-to-cdp)) The `element_index` is *recomputed* per observation — stable enough within a turn, not designed to survive across the agent's own re-renders.

### CDP facts we build on

- **`backendNodeId`** is a backend-scoped, document-lifetime-stable handle: "Unique DOM node identifier used to reference a node that may not have been pushed to the front-end." It persists independent of frontend tracking. ([DOM domain](https://chromedevtools.github.io/devtools-protocol/tot/DOM/))
- **`nodeId`** is frontend/session-scoped and is invalidated wholesale by `DOM.documentUpdated` ("Node ids are no longer valid"). Never cache it; re-push from `backendNodeId` at action time via `DOM.pushNodesByBackendIdsToFrontend`. ([DOM domain](https://chromedevtools.github.io/devtools-protocol/tot/DOM/))
- **`AXNodeId`** stays consistent between calls while `Accessibility` is enabled, and each `AXNode` exposes `backendDOMNodeId`. ([Accessibility domain](https://chromedevtools.github.io/devtools-protocol/tot/Accessibility/))

### Our identity strategy: `eid` as a re-bindable fingerprint, not a raw CDP ref

An `eid` (e.g. `btn-submit`, `inp-email`) is a **logical, content-derived, durable** identifier owned by IdentityMap. It is *not* a CDP id passed through. The IdentityMap maintains a binding:

```
eid  →  { backendNodeId, axNodeId, frameId, targetId, fingerprint, bbox }
```

**`backendNodeId` is the primary key while the document lives.** When a node survives a soft mutation (React re-render that reuses the DOM node, attribute change, text change), its `backendNodeId` is unchanged and the binding is preserved with zero work — this is the common case and where we beat per-snapshot ref schemes outright.

**Re-binding after hard mutation.** When the framework replaces a node (new `backendNodeId`) or `DOM.documentUpdated` fires (full re-render / SPA route swap), we re-resolve each live `eid` to a node by **fingerprint match**, so the *same logical element keeps the same `eid`*. The fingerprint is a tuple, matched in priority order:

1. Stable author attributes if present: `id`, `name`, `data-testid`, `aria-label`, `for`. (Cheap, exact.)
2. `(role, accessible-name)` from the AX node. (ModelPiper's AX-native engine validates this: "AX selectors are more stable — CSS refactors and component library migrations don't break them," matching by role + name similarity + tree position. ([ModelPiper AX-native](https://modelpiper.com/blog/ax-native-browser-automation-cdp-engine)))
3. Structural path: ordinal among siblings of same role within the nearest landmark region.
4. Geometry proximity (bbox centroid) as a tiebreaker.

A match above a confidence threshold **rebinds the existing `eid` to the new `backendNodeId`**. No match → the `eid` is retired and reported as `removed`; genuinely new elements get freshly minted `eid`s reported as `added`. This is exactly the "stable `eid`s + the previous IDs are no longer assumed valid only on navigation" contract that lespaceman's agent-web-interface ships ([agent-web-interface](https://github.com/lespaceman/agent-web-interface)) — but pushed further: we attempt cross-render rebind by fingerprint *even within a navigation*, instead of resetting the whole vocabulary.

**`eid` naming.** Human/LLM-legible and role-prefixed (`btn-`, `inp-`, `lnk-`, `chk-`, `sel-`, `row-`), derived from accessible name slugified, deduped with a numeric suffix. Legible IDs reduce model confusion vs opaque `e17`. The mapping `eid ⇄ backendNodeId` is internal; the model only ever sees `eid`.

**Node-tracking events.** We subscribe to `DOM.attributeModified` / `attributeRemoved` (state changes: disabled, aria-expanded, value), `DOM.childNodeInserted` / `childNodeRemoved` (add/remove), `DOM.characterDataModified` (text), and `DOM.documentUpdated` (full invalidation → trigger fingerprint rebind sweep). ([DOM domain](https://chromedevtools.github.io/devtools-protocol/tot/DOM/)) `Accessibility` also emits `nodesUpdated` when previously-requested AX nodes change ([Accessibility domain](https://chromedevtools.github.io/devtools-protocol/tot/Accessibility/)); we use it as a cheap "semantics dirty" signal to know *which* subtree to re-fetch rather than re-pulling the full AX tree.

---

## Primitive 2 — Interactive-only semantic page model

### Principle

Accessibility-first, interactive-only. The AX tree already discards presentational markup: "A React app with 2,000 DOM nodes might have 200 AX nodes." ([ModelPiper](https://modelpiper.com/blog/ax-native-browser-automation-cdp-engine)) We narrow further to the **interactive + state-bearing** slice (controls, links, inputs, plus the landmark/heading skeleton for orientation and a bounded slice of readable content).

### CDP mechanism

- `Accessibility.getFullAXTree` is the spine. Filter to nodes whose `role` ∈ {button, link, textbox, combobox, checkbox, radio, switch, slider, menuitem, tab, option, searchbox, ...} plus landmarks (navigation, main, banner, contentinfo) and headings for region structure. Drop `ignored:true` nodes.
- For each kept AX node, fuse the `DOMSnapshot.captureSnapshot` record (keyed by `backendNodeId`) to get: visibility (in viewport, non-zero box, not `display:none`), enabled/disabled, bbox for set-of-marks, and the input `type`.
- Emit per-element state flags the agent needs: `enabled`, `checked`, `selected`, `expanded`, `focused`, `required`, `val` (current input value) — the same state vocabulary agent-web-interface exposes. ([agent-web-interface](https://github.com/lespaceman/agent-web-interface))

### Serialization format: compact HTML-ish tags, not JSON

We follow Skyvern's empirical result: representing interactive elements as HTML instead of JSON cut tokens ~11% (20–27% across their dataset; one element 31 vs 70 tokens) **and** raised success +3.9% (63.8% vs 59.9%), hypothesized as fewer hallucinations from a smaller context. ([Skyvern HTML vs JSON](https://www.skyvern.com/blog/how-we-cut-token-count-by-11-and-boosted-success-rate-by-3-9-by-using-html-instead-of-json-in-our-llm-calls/)) JSON's repeated key names are pure overhead per element.

Our wire format is the terse XML-state agent-web-interface validated — short tags, region grouping, `eid` as the action handle:

```xml
<state step="1" title="Sign in | Acme" url="https://app.example.com/login">
  <meta view="1280x720" scroll="0,0" layer="main" />
  <baseline reason="first" />
  <region name="main">
    <h id="hd-sign-in">Sign in</h>
    <inp id="inp-email" type="email">Email</inp>
    <inp id="inp-password" type="password">Password</inp>
    <chk id="chk-remember">Remember me</chk>
    <btn id="btn-submit">Sign in</btn>
  </region>
</state>
```

### Token targets

Reference points from prior art:
- Browser Use sends ~245 interactive elements where Playwright MCP labels ~789 — interactive-only filtering is roughly a 3× reduction in element count before any per-element savings.
- A raw GitHub AX tree is ~19K tokens; compressed to ~4.3K (~4.4× reduction).

**Targets:** for a median content-app page, **≤ 5K tokens** for the full baseline snapshot, **≤ 800 tokens** for a typical post-action diff. We assert these as regression gates in the benchmark harness (below), not as aspirations.

---

## Primitive 3 — Event-sourced / diff observations

### Principle

Full baseline **once** after navigation; thereafter emit only deltas. The agent "continues from the changed page state instead of re-reading the entire DOM" — the agent-web-interface contract. ([agent-web-interface](https://github.com/lespaceman/agent-web-interface)) This is the single biggest token lever across a multi-step task: an N-step login that re-serializes the page each step costs O(N · page); diffs cost O(page + N · delta).

### CDP mechanism: CDP DOM events as the primary source, injected MutationObserver as a guarded supplement

Two candidate change sources:

| | CDP DOM mutation events | Injected `MutationObserver` (via `Runtime.evaluate`) |
|---|---|---|
| Coverage | attribute, childNode insert/remove, characterData, documentUpdated | full DOM mutations incl. subtree, plus we can debounce in-page |
| Cross-origin iframes | works via flattened auto-attach sessions | needs injection per frame/world |
| Robustness | first-class, survives page JS | page CSP / overwriting `MutationObserver` can interfere |
| Cost | events stream regardless | one `Runtime.evaluate` install + batched callbacks |

**Decision:** CDP DOM events are the **primary** change feed — they're first-class, immune to page CSP, and already give us `DOM.documentUpdated` (the hard-reset signal) and `Accessibility.nodesUpdated` (the semantics-dirty signal). We add a thin injected `MutationObserver` **only** as a *quiescence detector*: it answers "has the DOM stopped changing for K ms?" so the DiffEngine knows when a post-action mutation storm has settled, without us polling. We do not rely on the injected observer for the change *content* (which CDP events + a targeted re-fetch of the dirty AX subtree provide). This keeps us robust on hostile pages while still getting a clean stabilization signal.

### The delta algorithm

State after each baseline is the IdentityMap's interactive set: `{ eid → (fingerprint, state-flags, bbox, text) }`.

On stabilization after an action:
1. Re-fetch only the AX subtree(s) flagged dirty by `nodesUpdated` (fallback to full `getFullAXTree` if `documentUpdated` fired).
2. Rebind `eid`s via Primitive 1.
3. Diff old vs new interactive set:
   - **added** — new `eid`s (post-rebind).
   - **removed** — retired `eid`s.
   - **changed** — same `eid`, changed state-flag or value or text.
4. Capture **transient observations** — elements with `role=status`/`alert`/toast that appeared then vanished within the stabilization window get reported in `<observations>` with `delay_ms` and `transient="true"` even though they're gone, because they're the agent's success signal (e.g. "Note saved."). This is the agent-web-interface `<observations>` pattern and it's what lets an agent *confirm* an action without re-reading the page.

Emit:

```xml
<state step="4" ...>
  <meta .../>
  <diff type="mutation" added="1">
    <status id="rd-note-status" role="status">Note saved.</status>
  </diff>
  <observations>
    <appeared when="action" eid="toast-note-saved" role="status" delay_ms="180" transient="true">Note saved.</appeared>
  </observations>
</state>
```

---

## Primitive 4 — Structured action space (deterministic, diff-returning)

Every action is keyed by **`eid`** and returns the resulting diff. Actions never take CSS selectors from the model (selectors are a power-user library escape hatch, not part of the agent contract).

| Action | Params | CDP execution |
|---|---|---|
| `navigate` | url | `Page.navigate` → wait load → full baseline |
| `click` | eid, [button] | re-push backendNodeId → `DOM.getBoxModel` → `Input.dispatchMouseEvent` (move,press,release) at box centroid; **occlusion check** first |
| `type` | eid, text, [clear] | `DOM.focus` → optional select-all+delete → `Input.insertText` (and `Input.dispatchKeyEvent` for keys that trigger handlers) |
| `select` | eid, value | resolve node → `Runtime.callFunctionOn` to set `.value` + dispatch `change`, or open native + arrow |
| `press` | key | `Input.dispatchKeyEvent` against focused element |
| `scroll` | dir/eid, [px] | `Input.dispatchMouseEvent` wheel, or `DOM.scrollIntoViewIfNeeded` for eid |
| `hover` | eid | `Input.dispatchMouseEvent` move to centroid |
| `check`/`uncheck` | eid | click with checked-state assertion |
| `upload` | eid, files | `DOM.setFileInputFiles` |

### Determinism guarantees

- **Re-resolve at action time.** Always `DOM.pushNodesByBackendIdsToFrontend` immediately before acting; never trust a cached `nodeId`. If the `backendNodeId` no longer resolves, run a fingerprint rebind first; if that fails, return a structured `stale` error with the closest candidates rather than acting blindly.
- **Occlusion pre-check.** Before a click, hit-test the centroid (`DOM.getNodeForLocation` or an injected `elementFromPoint`) and verify it lands on the target or a descendant. If a consent banner/modal covers it, fail early and *name the covering element's `eid`* — this is the vercel-labs failure-mode, turned into a structured, actionable error. ([vercel-labs/agent-browser](https://github.com/vercel-labs/agent-browser))
- **Every action returns `{ ok, diff, observations, error? }`.** Success/failure is explicit and carries the page delta, so the agent's next decision is grounded in the actual resulting state, not an assumption.
- **Stabilization barrier** between act and report (network-idle + mutation quiescence, bounded timeout) so diffs are taken against a settled page.

---

## Fallback — set-of-marks / screenshot

The a11y tree under-represents some UIs: `<canvas>` apps, custom-painted widgets, map tiles, ARIA-less divs with click handlers, drag surfaces. Vision is the fallback, never the default — A11y/SoM materially raise per-task latency and tokens (OSWorld-Human, WebSight), so we gate it.

### Trigger conditions (any of)

1. The interactive set is suspiciously sparse relative to clickable-looking geometry (snapshot shows large elements with pointer cursor / click listeners but no AX role).
2. The agent's last `click`/`type` produced **no diff** (action landed nowhere meaningful) twice.
3. Explicit `mode:"visual"` request from the consumer.
4. Target region is inside a `<canvas>` / WebGL surface.

### How set-of-marks works here

- Take `Page.captureScreenshot` (clip to viewport).
- Overlay numbered marks **tied to durable `eid`s**: for each interactive element with a bbox (from the snapshot we already have), draw a labeled box; the label maps `mark# ↔ eid` in the response so a click by mark resolves through the same IdentityMap. No separate identity space — marks are a *view* of the durable IDs.
- For under-represented regions, additionally synthesize candidate marks from clickable geometry (pointer-cursor / listener heuristics) and assign them provisional `eid`s so the agent can still act, with a `synthetic="true"` flag.

This keeps a single identity vocabulary across text and vision modes, which neither Playwright MCP (vision and a11y are separate) nor set-of-marks papers (marks are ephemeral) deliver.

---

## Language & runtime recommendation

**Recommendation: TypeScript for v1 core + MCP server; keep the CDP transport and hot-path serialization behind a narrow interface so a Rust core can be swapped under it later if benchmarks demand.**

### Why TypeScript first

- **CDP ecosystem gravity.** `chrome-remote-interface`, typed protocol definitions (`devtools-protocol` package), and the entire MCP reference tooling are TS-native. Fast iteration on the *algorithms that are the actual product* — fingerprint rebind, diff, AX/snapshot fusion — matters far more in months 1–3 than raw throughput.
- **Embeddable AND MCP in one toolchain.** The library ships as an npm package consumable by JS/TS agent frameworks, and the MCP server is the same code with a stdio/SSE shell — `@modelcontextprotocol/sdk` is first-class in TS. One language covers both required surfaces.
- **The workload is I/O-bound, not CPU-bound.** The dominant cost is CDP round-trips and the LLM call, not local compute. We are reducing *tokens*, not microseconds. Rust's perf edge (Browser Use's CDP move was about *element-extraction speed and iframe correctness*, not language ([Closer to the Metal](https://browser-use.com/posts/playwright-to-cdp))) buys little against a 2–10s model turn.

### Why not Rust (for v1)

vercel-labs `agent-browser` (Rust, single static binary, 36K★ ([vercel-labs/agent-browser](https://github.com/vercel-labs/agent-browser))) and Lightpanda (Zig) prove the static-binary ethos is attractive. But: (a) the MCP + embeddable-library dual requirement is cheaper to satisfy in TS today; (b) the AX/diff/fingerprint logic will churn heavily early and Rust's iteration tax is real; (c) we'd reach for `cdp-use`-style codegen anyway. **Rust is the right *eventual* home** for a single-binary CLI distribution and for the diff hot path if profiling shows serialization dominating — hence the narrow transport/serialization seam from day one. We do **not** start there.

### Runtime constraints honored

- Embeddable: pure library entrypoint, no daemon required, BYO CDP endpoint (`ws://...`).
- MCP server: thin wrapper exposing `navigate/snapshot/click/type/select/press/scroll/screenshot` tools over the library.
- Works against local Chrome, Lightpanda (22 CDP domains incl. Accessibility ([Lightpanda](https://lightpanda.io/blog/posts/web-automation-stack-explained))), Browserbase, Cloudflare Browser Rendering — anything exposing a CDP WebSocket.

---

## Benchmark harness — "is the library worth it?"

The harness is the proof, and it ships in month 1. It measures two axes against a **Playwright-MCP baseline** on a fixed flow suite.

### Metrics

1. **Tokens-per-task** — sum of all observation tokens fed to the model across a completed flow (baseline + every diff/snapshot). Tokenized with the actual target tokenizer (tiktoken `o200k`).
2. **Success-under-mutation** — does the flow complete when the page re-renders between steps? We inject controlled mutations (see below) and measure completion rate. This is the differentiating metric: per-snapshot-ref tools should degrade here while durable `eid`s hold.
3. **Steps-to-completion** and **stale-action rate** (actions returning `stale`/no-diff) as secondary signals.

### Fixed flow suite (multi-step, deterministic, self-hosted)

We host the targets ourselves (a small set of synthetic apps) so the benchmark is hermetic and reproducible — no live-site flakiness:

- **F1 Login** — email → password → submit → assert dashboard. (2–3 steps.)
- **F2 Search + result select** — type query → submit → click Nth result. (3 steps; result list mutates.)
- **F3 Multi-page form** — 3-page wizard with validation, back/next, a required field error path. (8–10 steps.)
- **F4 Dashboard-row task** — find a row by content → open → fill a note → confirm toast. (the agent-web-interface journey; exercises transient `<observations>`.)

### Mutation injection (the core experiment)

Each synthetic app runs in modes:
- **stable** — DOM stays put.
- **soft-rerender** — between every step, a framework-style re-render that *reuses* nodes (backendNodeId stable) — should be free for us, costly for nothing.
- **hard-rerender** — between every step, nodes are replaced (new backendNodeId, same content) and a `documentUpdated` fires — this kills per-snapshot refs and exercises our fingerprint rebind.
- **reorder / insert** — rows inserted/reordered above the target.

**Hypothesis to prove:** under `hard-rerender` and `reorder`, our success rate stays high (rebind works) while a ref-recompute baseline drops; and tokens-per-task is materially lower across all modes due to diffs.

### Harness mechanics

- A tiny scripted "agent" with a *fixed oracle policy per flow* (no LLM — deterministic action sequence keyed on logical intent) so we measure the *interface*, not model variance. The same logical script runs against our library and against Playwright-MCP (translating intent → that tool's ref discovery each step).
- Output: a CSV + a generated markdown report (tokens, success%, steps) per (flow × mutation-mode × tool). Wired into CI as a regression gate against the token targets (baseline ≤ 5K, diff ≤ 800).

---

## Phased roadmap (slow, cron-driven, autonomous-build-friendly)

Each phase is a coherent shippable increment; cadence follows substance, not a quota.

### Phase 0 — Spike (week 1)
- Attach to local Chrome via CDP, `Accessibility.getFullAXTree` + `DOMSnapshot.captureSnapshot`, fuse on `backendNodeId`, print the interactive-only set as the XML-state format.
- No identity persistence yet, no diffs. Pure read path.
- **Exit:** one command renders a real page (e.g. a login form) as a ≤5K-token baseline.

### Phase 1 — Durable identity + actions (weeks 2–3)
- IdentityMap with `eid ⇄ backendNodeId` and the fingerprint rebind ladder (attrs → role+name → structural → geometry).
- Structured actions: `navigate/click/type/select/press/scroll`, each re-pushing backendNodeId at action time, with occlusion pre-check and `{ok, error}` results.
- Subscribe to DOM mutation + `documentUpdated` events; rebind sweep on hard reset.

### Phase 2 — "Alive" (week 4 deliverable)
**Week-4 "alive" = a real, externally-runnable thing that proves the thesis on one flow.** Concretely:
- The **diff model** works end-to-end: baseline once, then `<diff>` + `<observations>` per action, with transient toast capture.
- An **MCP server** exposes `navigate/snapshot/click/type/select/press` — an MCP host (e.g. Claude Code/Cursor) can complete the **F4 dashboard-row journey** against a self-hosted target, driving purely by `eid`, with diffs in between.
- The **benchmark harness** runs F1+F4 against our lib in `stable` and `hard-rerender` modes and emits the tokens/success report, demonstrating: (a) diffs cut tokens-per-task vs a re-snapshot baseline, and (b) success-under-`hard-rerender` stays high because `eid`s rebind.
- Public repo, README with the agent loop, the four-primitive pitch, and the first benchmark numbers.

### Phase 3 — Robustness & fallback (weeks 5–8)
- Cross-origin iframe support via flattened auto-attach (the Browser Use pain point ([Closer to the Metal](https://browser-use.com/posts/playwright-to-cdp))).
- Set-of-marks/screenshot fallback with `eid`-tied marks + synthetic marks for under-represented regions; trigger heuristics.
- `Accessibility.nodesUpdated`-driven targeted subtree re-fetch (stop full-tree re-pulls).
- Full F1–F4 × all mutation modes in CI; token-target regression gates.

### Phase 4 — Reach & hardening (weeks 9–16)
- Validate against Lightpanda, Browserbase, Cloudflare Browser Rendering CDP endpoints; document per-backend quirks.
- Shadow DOM / web-component depth, virtualized lists (windowed rows), file upload, dialogs.
- Optional: extract the transport + serialization hot path; spike a Rust core behind the seam if profiling justifies; single-binary CLI distribution.

---

## Name candidates

Checked against npm (404 = free) and conceptual GitHub/crates availability. Avoid the obvious occupied single words (`axion`, `glint`, `weave`, `loupe`, `lucid`, `tether`, `latch` — all taken on npm).

| Name | Rationale | npm | Notes |
|---|---|---|---|
| **axweave** | weaves the **AX** tree with DOM geometry — exactly what the fusion step does | free | clear, on-thesis, slightly technical |
| **anchortree** | the durable-identity thesis: IDs that stay *anchored* across re-renders | free | most evocative of the core insight |
| **nodeloom** | "loom" = the rebind fabric tying logical eids to shifting nodes | free | memorable, distinctive |
| **axref** | the AX-derived durable **ref** (vs ephemeral CDP refs) | free | short, but "ref" overloaded |
| **axport** | the *port* (interface) onto the AX layer | free | clean, library-ish |
| **anchorpoint** | durable anchor metaphor, friendlier word | free | longer |
| **loomdom** | DOM woven into a stable fabric | free | playful |
| **semdom** | semantic DOM | free | descriptive but generic |

**Lead recommendation: `anchortree`** — it names the actual differentiator (identity that stays anchored as the tree re-renders), is free on npm, reads as a library, and is unlikely to collide. `axweave` is the strong runner-up if we want to foreground the AX-fusion mechanism. Verify crates.io and GitHub org availability at adoption time (crates.io returned rate-limited during this pass, not a clean signal).

---

## Appendix — primary sources

- CDP DOM domain (backendNodeId/nodeId semantics, mutation events, documentUpdated): https://chromedevtools.github.io/devtools-protocol/tot/DOM/
- CDP Accessibility domain (getFullAXTree, AXNodeId consistency, backendDOMNodeId, nodesUpdated): https://chromedevtools.github.io/devtools-protocol/tot/Accessibility/
- CDP DOMSnapshot domain (captureSnapshot, flattened layout+style): https://chromedevtools.github.io/devtools-protocol/tot/DOMSnapshot/
- Browser Use — "Closer to the Metal: Leaving Playwright for CDP" (cdp-use, EnhancedDOMTreeNode super-selectors, element_index): https://browser-use.com/posts/playwright-to-cdp
- Playwright MCP snapshots (per-snapshot refs, regenerated on change): https://playwright.dev/mcp/snapshots
- vercel-labs/agent-browser (Rust CLI, `@eN` refs, occlusion failure mode): https://github.com/vercel-labs/agent-browser
- lespaceman/agent-web-interface (stable eids, baseline/diff XML state, observations): https://github.com/lespaceman/agent-web-interface
- ModelPiper — AX-native CDP engine (AX stability, role+name+position rebind, 2000 DOM → 200 AX): https://modelpiper.com/blog/ax-native-browser-automation-cdp-engine
- Skyvern — HTML vs JSON token economics (−11% tokens, +3.9% success, 31 vs 70 tokens/element): https://www.skyvern.com/blog/how-we-cut-token-count-by-11-and-boosted-success-rate-by-3-9-by-using-html-instead-of-json-in-our-llm-calls/
- Lightpanda — CDP domains incl. Accessibility, AX tree native: https://lightpanda.io/blog/posts/web-automation-stack-explained
- OSWorld-Human (A11y/SoM latency cost): https://arxiv.org/pdf/2506.16042
