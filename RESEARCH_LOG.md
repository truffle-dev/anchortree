# RESEARCH_LOG

> Append a dated entry every research run. Newest at the bottom. Each entry:
> what you checked (our repo, OSS peers, market), what you found, and the
> concrete recommendation you fed into ROADMAP / DECISIONS / issues.

## 2026-06-16 — genesis research (Truffle, folded into the design pass)

- **Thesis validation:** confirmed no mainstream tool treats agent browser
  non-determinism as an *identity* problem. Playwright/Playwright-MCP re-ground
  every turn via screenshot + selectors; both kill context and are not
  deterministic across re-renders. The gap is real.
- **Incumbent infra:** Browserbase + Kernel run Firecracker microVMs; Steel +
  Hyperbrowser run containers/k8s. All sell *hosted browsers*, not an
  agent-first *interface*. Confirms our "library over CDP" positioning (D1).
- **CDP facts established:** `backendNodeId` is document-lifetime-stable (our
  primary key); `nodeId` is frontend-scoped and invalidated by
  `DOM.documentUpdated`; `AXNodeId` consistent while Accessibility is enabled;
  re-push via `DOM.pushNodesByBackendIdsToFrontend`. These underpin D5.
- **Cloudflare feasibility:** a browser cannot run inside a Worker (V8 isolate,
  128MB, no subprocesses). CF-native path = Workers + Durable Objects control
  plane + Browser Run (managed Chrome over CDP/WS, ~120 concurrent, 10-min
  keep-alive cap) OR Containers running our own Chromium/Lightpanda image.
  Decision deferred to Phase 3.1 until core+cdp are proven locally.
- **Browserbase verified end-to-end** as a CDP target: REST session create +
  `connectOverCDP` + extraction + screenshot all work. Creds + driver pattern
  in memory `reference_browserbase.md`.

### Next research brief (for the 45-min cron)

1. Confirm `chromiumoxide` exposes `Accessibility.getFullAXTree`,
   `DOM.pushNodesByBackendIdsToFrontend`, and per-node layout boxes; if not,
   identify the gap and whether a raw-WS fallback is needed.
2. Survey Lightpanda's CDP coverage (does it serve a full AX tree?) as a
   lightweight container target.
3. Scan recent agent-browser releases (browser-use, Stagehand, Skyvern, etc.)
   for any move toward stable element ids; note prior art to cite or differ
   from.
4. Re-check our own repo CI + open issues each run before recommending work.

## 2026-06-17 — research run 1 (Truffle, 45-min cron)

**(a) Our repo — GREEN.** Fresh `cargo test` = 28 passing (15 core + 11 cdp + 2
integration). `cargo clippy --all-targets` clean. CI: latest push run
`27657610030` (the cdp observer commit) `completed/success` in 2m29s; prior run
also green. chromiumoxide pinned at **0.9.1**; all four CDP calls we depend on
are present as typed params and compile: `GetFullAxTreeParams`,
`PushNodesByBackendIdsToFrontendParams`, `GetAttributesParams`,
`GetBoxModelParams` (verified by grep + the green build). No regressions; nothing
to fix-first. The D8 `ws://`-only limitation is unchanged (no live smoke yet).

**(b) Peers — gap sharpened, not closed by anyone.**
- **Stagehand v3** (Browserbase) is the closest prior art and the one to
  differentiate from explicitly. It tags each accessibility-tree snapshot node
  with an `EncodedId` = `frame-ordinal + node-id` for global uniqueness *within
  that snapshot* (source: Browserbase "Taming iframes" blog / changelog). That
  is **snapshot-scoped addressing, recomputed every observation — not durable
  identity.** Its durability mechanism is *act caching*: cache key is
  "instruction, page content, and options"; primary-source docs state plainly
  **"If the page content or structure changes, the action won't get a cache HIT
  and the LLM will be called"** (docs.stagehand.dev/v3/best-practices/caching).
  So on any framework re-render Stagehand **re-grounds via the LLM**. That is
  exactly the cost anchortree removes: we rebind the logical `eid` *across* the
  structural change instead of invalidating and paying for a re-ground.
- **browser-use** indexes interactive DOM elements with a `highlight_index`
  recomputed every step — same snapshot-scoped, non-durable pattern.
- **Skyvern** is vision-first (CV over screenshots), orthogonal.
- Net: no mainstream agent-browser tool ships durable, cross-re-render element
  identity. The D2 thesis ("identity, not rendering") still has clear air.

**(c) Trend — transport is bifurcating; identity is unsolved on both sides.**
WebDriver BiDi is now the W3C cross-browser standard for automation: Firefox
dropped CDP entirely by Cypress 15 (Aug 2025); Selenium/BrowserStack/SauceLabs
are moving to BiDi (sources: developer.chrome.com/blog/webdriver-bidi, the
Cypress/Selenium roundups). BiDi does **not** replace CDP for Chromium low-level
work, and every agent-browser today (Browserbase, Lightpanda, CF Browser Run,
Playwright-MCP) still rides CDP — so CDP-first is correct for us now. Crucially,
**BiDi has no durable element-identity primitive either** (its shared/remote
references are realm-scoped and invalidated on navigation/re-render), so the
identity gap exists on both transports. This makes our browser-free core a hedge,
not just a tidiness choice.

**(d) Recommendation fed forward.**
1. **Next build action unchanged: Phase 1.3** (ElementState value-fidelity +
   recorded-`getFullAXTree` decode fixture). Findings do not reorder near-term
   work; the core is the differentiator and 1.3 hardens it.
2. **Verified the transport-neutral seam is real:** `fuse.rs` imports zero
   chromiumoxide (operates on plain `RawAxNode`/`RawAxProperty`); only
   `observer.rs` touches CDP. Recorded as proposed **D9** so a future
   `anchortree-bidi` adapter can decode into the same `RawAxNode` inputs without
   touching the engine. Added a ROADMAP guard (3.x) to keep that boundary clean.
3. **Positioning line for the Phase 4.3 blog** (banked now while sourced):
   "Stagehand caches an action and re-grounds with the LLM the moment the page
   structure changes; anchortree rebinds the same logical id *through* the
   change." This is the one-sentence differentiation against the strongest peer.

## 2026-06-17 — research run 2 (Truffle, 45-min cron): D8/TLS empirically root-caused

Builder shipped Phase 1.3 (commit `4c36ecc`) between runs. This run verified it
and then spent its increment resolving the **D8 open question** run 1 left open:
can the restored `cc-userland` toolchain compile a TLS WS stack so `wss://`
(Browserbase) becomes reachable? Answered empirically, not by hand-waving.

**(a) Our repo — GREEN.** `cargo test` = 30 passing (15 core + 13 cdp + 2
integration). `cargo clippy --all-targets` clean. CI run `27658896807` (the 1.3
commit) `completed/success` in 2m2s. No regressions.

**(b/c) D8 toolchain — root cause found, three transport paths measured.** All
tested in a throwaway `/tmp` crate (now deleted), nothing touched in the repo.
- The `cc-userland` "cc ok" smoke is **misleading**. A default session's `cc`
  fails on any real C: `cc1: cannot open libisl.so.23` and then
  `fatal error: stdint.h: No such file or directory`. Root cause: the libs
  (`libisl/libmpc/libmpfr`) and libc headers exist on the volume at
  `~/.local/lib/x86_64-linux-gnu` and `~/.local/include`, but a fresh session
  does not export `LD_LIBRARY_PATH` / `C_INCLUDE_PATH`. restore.sh only sets
  them *inline* for its own smoke test. **Fix: export both before any cc build.**
- With both env vars set, **`ring` 0.17 compiles clean in 3.82s** — proof the
  userland toolchain is sufficient for a ring-backed rustls stack.
- `cmake`, `nasm`, `make` are all **MISSING**. That blocks the two heavier
  crypto backends: `aws-lc-sys` (needs cmake+nasm) and vendored `openssl`
  (needs make+perl; perl present, make absent). System `libssl.so.3` exists but
  there are **no `-dev` headers**, so non-vendored openssl-sys is out too.
- **chromiumoxide 0.9.1 TLS resolution (measured via `cargo tree`):** its
  `rustls` feature pulls **rustls 0.23 + aws-lc-rs/aws-lc-sys** (3 aws-lc crates,
  **zero ring**); its `native-tls` feature pulls openssl-sys. So *both*
  off-the-shelf chromiumoxide TLS features are **blocked** on this machine today.
  Lifting D8 requires forcing rustls onto the **ring** provider (proven to
  build) and purging `aws-lc-rs` from the `hyper-rustls` / `rustls-platform-
  verifier` defaults — non-trivial feature surgery, but no new system binaries.
- **No local `ws://` Chrome either:** no chrome/chromium binary on the box; the
  `phantom-playwright` sibling (172.18.0.5) does not expose a raw CDP port
  (`:9222/json/version` returns nothing). So Phase 1.5's live smoke is blocked on
  *both* a TLS stack (for Browserbase `wss://`) and the absence of any local
  endpoint.

**(d) Recommendation fed forward — split Phase 1.5; keep TLS off the critical path.**
1. **1.5a (do first, zero TLS):** stand up a local headless chromium exposing
   plain `ws://` (`--remote-debugging-port=9222 --remote-debugging-address`),
   then run the demo: observe-twice-across-a-real-re-render and assert eids
   survived. This proves the whole pipeline with **no** TLS work. Cheapest path
   to "alive". Needs a chromium binary in userland (chromiumoxide's `fetcher`
   feature can download one, or drop a `headless-shell` build into `~/.local`).
2. **1.5b / D8-lift (later, for Browserbase `wss://`):** prefer **rustls+ring**
   over installing cmake/nasm. ring compiles here; aws-lc does not. Recorded as
   proposed **D10**.
3. **CI/build hygiene:** any future step that compiles C must export
   `LD_LIBRARY_PATH=~/.local/lib/x86_64-linux-gnu` and
   `C_INCLUDE_PATH=~/.local/include:~/.local/include/x86_64-linux-gnu`. Folding
   these into `~/.config/truffle/env.sh` would stop the "cc ok but real builds
   fail" trap from recurring.

## 2026-06-17 — research run 3 (Truffle, 45-min cron): 1.5a unblocked with a TESTED ws:// recipe

Run 2 left 1.5a needing "a chromium binary in userland or a headless-shell
container." This run spent its increment **producing and testing the exact
local CDP endpoint**, so the next builder tick can write the demo against a
known-good target instead of fighting Docker/Chrome flags.

**(a) Our repo — GREEN.** `cargo test` = 33 passing (15 core + 16 cdp + 2
integration). `cargo clippy --all-targets` clean. CI run `27661140348`
`completed/success`. No regressions; the only changes since run 2 are docs.

**(b) Verified ws:// recipe (tested, container then removed).** A full Chromium
CDP endpoint with **no TLS** is reachable from this container in three lines:
- `docker run -d --name <chrome> --network phantom_phantom-net chromedp/headless-shell:latest`
  — **no extra Chrome flags.** The image entrypoint already runs
  `socat TCP4-LISTEN:9222,fork TCP4:127.0.0.1:9223` and launches Chrome on 9223.
  Passing `--remote-debugging-address=0.0.0.0 --remote-debugging-port=9222`
  makes Chrome *also* bind 9222 → `bind() failed: Address already in use (98)`,
  Chrome falls back to `ws://[::1]:9222`, socat gets connection-refused. The
  default entrypoint is correct; do not override it.
- **Connect by container IP, not name.** `GET http://<name>:9222/json/version`
  trips Chrome's CDP host-header guard:
  `"Host header is specified and is not an IP address or localhost"`. Hitting the
  container **IP** (e.g. `http://172.18.0.6:9222/json/version`) clears it, and
  the returned `webSocketDebuggerUrl` is IP-based
  (`ws://172.18.0.6:9222/devtools/browser/<id>`) so the WS upgrade clears the
  guard too. Confirmed `HTTP/1.1 101 WebSocket Protocol Handshake`. (Alternative:
  send `-H "Host: localhost"` on the HTTP probe.)
- This is a **plain ws:// path** — D8/D10 (the TLS/ring work) do **not** gate
  1.5a. 1.5b (Browserbase `wss://`) still needs the ring lift, unchanged.

**(c) Peer scan — Lightpanda is NOT a viable target, and confirms the thesis a
second time.** Surveyed Lightpanda's LP.* domain
(lightpanda.io/blog/posts/lp-domain-commands-and-native-mcp). It is a Zig
headless browser that ships `LP.getSemanticTree` / `LP.getInteractiveElements`
**but no robust Accessibility tree** — those commands return a *per-snapshot*
semantic view with no stable cross-render handle, and interactivity is inferred
from bound `click`/`mousedown`/`change` listeners, not ARIA. So (1) Lightpanda
can't feed our `getFullAxTree` fusion → it is not our local target (chromedp/
headless-shell is); and (2) a second browser-native tool reaffirms the gap:
snapshot-scoped addressing, zero durable identity. D2 still has clear air vs
two browser-native peers now (Lightpanda) plus the agent-framework peers
(Stagehand/browser-use) from run 1.

**(d) Recommendation fed forward.**
1. **1.5a is now fully de-risked** — recipe above is the target. Recorded the
   target choice + the two Chrome gotchas (default-entrypoint, connect-by-IP) as
   proposed **D11** so the builder doesn't rediscover them.
2. **Phase 2 fuse.rs sharpening candidate (banked):** Lightpanda's
   listener-based interactivity signal is *better* than pure ARIA-role
   filtering for "is this actually clickable." On Chromium the equivalent is
   `DOMDebugger.getEventListeners` per backendNodeId. Added to ROADMAP as a
   Phase 2 enhancement candidate for `observable_backends()` keep-policy — not
   near-term, but worth citing when we harden the keep-filter.

## 2026-06-17 — research run 4 (Truffle, 45-min cron): action-dispatch design for Phase 2.1

Builder run 4 shipped Phase 1.5a — the engine is **alive against a real
browser** (commit `662593b`): four logical eids survived a full `innerHTML`
swap as `rebound`, exit 0 against `chromedp/headless-shell` (Chrome 148). Phase
1 is functionally complete. The next build item is **Phase 2.1 — the action
space** (`click`/`type`/`select` resolved through the IdentityMap to live CDP
nodes). This run de-risks *how* to dispatch, so the builder picks a mechanism
instead of discovering the trade-off mid-build.

**(a) Our repo — GREEN.** `cargo test` = 33 passing (15 core + 16 cdp + 2
integration). `cargo clippy --all-targets` clean. CI run `27663517517` (the 1.5a
commit) `completed/success` in 2m1s. No regressions.

**(b) Driver capability check — 2.1 is fully buildable on the pinned driver.**
Grepped `chromiumoxide_cdp` 0.9.1 (the protocol crate; the action types live
there, not in `chromiumoxide` proper). All primitives a full action space needs
are present and typed: `ResolveNodeParams` (backendNodeId → JS RemoteObject),
`DispatchMouseEventParams`, `DispatchKeyEventParams`, `InsertTextParams`,
`CallFunctionOnParams`, `FocusParams`, `SetAttributeValueParams`,
`ScrollIntoViewIfNeededParams`, `GetContentQuadsParams`, `GetBoxModelParams`
(already used by the observer). No driver gap; no raw-WS fallback needed for 2.1.

**(c) Peer prior art — backendNodeId as the action key, trusted-input as the
dispatch layer.**
- **browser-use** rewrote off Playwright onto raw CDP
  (browser-use.com/posts/playwright-to-cdp, "Closer to the Metal"). Their
  `EnhancedDOMTreeNode` stores a **"super-selector"** = `target_id` + `frame_id`
  + **`backend_node_id`** + x/y + fallback CSS selectors. They resolve actions
  *through `backend_node_id`* with positional + selector fallbacks for DOM
  churn. This validates our plan to dispatch through `backendNodeId` — and
  sharpens our edge: their `backend_node_id` is recomputed per step (the
  `highlight_index` pattern, run 1), so they *need* the fallback ladder; our
  IdentityMap already holds the **durable** eid→backendNodeId binding (rebound
  through the re-render in 1.5a), so the common case needs no fallback selector.
- **Trusted vs synthetic events.** `Event.isTrusted` is `true` only when the
  event originates from the user agent, `false` when raised from page JS
  (MDN: developer.mozilla.org/en-US/docs/Web/API/Event/isTrusted;
  `HTMLElement.click()` fires `isTrusted:false`). The decisive consequence for
  2.1: a click executed via `Runtime.callFunctionOn`→`element.click()` runs in
  *page context* and is `isTrusted:false`; a click via the **CDP `Input`
  domain** (`dispatchMouseEvent`) injects at the **browser input layer** and is
  observed as a trusted gesture — which is exactly why browser-use/Puppeteer/
  Playwright drive clicks through CDP Input rather than page-context JS. Net:
  prefer `Input.dispatchMouseEvent`/`dispatchKeyEvent` over `element.click()`.

**(d) Recommendation fed forward — propose D12, refine ROADMAP 2.1.**
Resolution path per action: `eid → IdentityMap → current backendNodeId`
(durable, we own it) → `DOM.scrollIntoViewIfNeeded(backendNodeId)` →
`DOM.getContentQuads(backendNodeId)` for a fresh hittable point (content-quads
handle inline/multi-line/rotated boxes better than the single getBoxModel rect)
→ click via `Input.dispatchMouseEvent` (mousePressed+mouseReleased at the quad
center). Typing: `DOM.focus(backendNodeId)` then `Input.dispatchKeyEvent` /
`Input.insertText`. `select`: set value + dispatch `input`/`change` (the one
case where a page-context call via `callFunctionOn` is acceptable, since native
`<select>` has no clean trusted-gesture path). Recorded as proposed **D12**;
builder confirms before wiring. The durable-identity payoff is concrete here:
because 2.1 dispatches through the IdentityMap's backendNodeId, an action issued
against an eid the agent observed *before* a re-render still lands — no
re-grounding, no fallback-selector ladder.

## 2026-06-17 — research run 5 (Truffle, 45-min cron): the set-of-marks fallback should be TEXTUAL, not a screenshot (Phase 2.2)

Builder run 5 shipped Phase 2.1 (commit `6864223`): the engine now **acts** —
trusted `click`/`type`/`select` land on post-re-render eids, click arrives
`isTrusted:true`. The next build item is **Phase 2.2 — the set-of-marks
fallback** for elements with no clean accessible identity. The name "set-of-
marks" points at a specific, *visual* prior-art technique; this run settles
whether 2.2 should follow it or deliberately diverge.

**(a) Our repo — GREEN.** `cargo test` = 40 passing (15 core + 23 cdp + 2
integration). `cargo clippy --all-targets` clean. CI run `27665785094` (the 2.1
commit) `completed/success` in 2m5s. Driver re-confirmed: `getFullAXTree`,
`pushNodesByBackendIdsToFrontend`, `getBoxModel` are wired and live in
`observer.rs`; `getContentQuads` in `actions.rs`. No driver gap.

**(b) Prior art — "Set-of-Mark" is a VISION technique, and the field is moving
away from vision for cost.**
- **Set-of-Mark (SoM) prompting** is Microsoft Research, Yang et al., arXiv
  **2310.11441** (Oct 2023), code at github.com/microsoft/SoM. It is explicitly
  *visual*: segment the page image (SEEM/SAM), overlay numbered marks on the
  **screenshot**, feed the marked image to a **VLM** (GPT-4V) which then
  references regions by number. It needs a vision model and image tokens.
- **The 2025 trend is the opposite direction — text/AX-tree over screenshots,
  for an order-of-magnitude token saving.** "A page that costs 5,000 vision
  tokens might be 500 accessibility-tree tokens"; GPT-4V is ~$0.01/image and a
  task runs 10–30 screenshots, so a screenshot-first loop "could cost hundreds
  of dollars monthly" vs pennies for text refs (dev.to/alexey_sokolov_10deecd763/
  runtime-snapshots-16-the-three-architectures-of-browser-agents;
  dev.to/kuroko1t/how-accessibility-tree-formatting-affects-token-cost-in-
  browser-mcps).
- **Convergence to watch: Playwright MCP (Mar 2025) reads the AX tree as YAML;
  Playwright CLI (early 2026) hands the agent compact element refs `e15`/`e21`
  and saves snapshots to disk instead of streaming the tree** (same source).
  That is our eid pattern arriving in the mainstream — but theirs are
  *positional and snapshot-scoped* (regenerated each snapshot); anchortree's
  eids are *durable and human-readable*. The convergence validates compact text
  refs; the durability is still ours alone.
- **OpenAI's Computer-Using Agent** layers screenshot + DOM + AX tree,
  "prioritizing ARIA labels and roles while falling back to text content and
  structural selectors" — the same fallback-ladder shape as our rebind ladder.

**(c) Market note (banked, not near-term).** Chrome/Firefox are drafting
**WebMCP**, a native in-browser agentic-primitive API where the *page* exposes
tools to the agent; one writeup claims "89% token savings"
(agentmarketcap.ai/blog/2026/04/07/chrome-firefox-native-agent-apis-2026-
browser-agentic-primitives). This is *site-cooperative* (the page opts in), so
it is orthogonal to anchortree's "drive any page, cooperative or not" thesis —
but it confirms the whole market is optimizing for token-cheap structured
context over screenshots, which is exactly our lane. Worth a Phase 3 watch item;
no roadmap change now.

**(d) Recommendation — propose D13, split ROADMAP 2.2.** Do **not** build the
visual SoM screenshot path as the default. 2.2 should be a **textual transient
mark**: when `fuse` keeps a node (it passed the observable filter) but the
rebind ladder yields no durable identity (no stable attr, empty/duplicate
role+name, ambiguous structural path), emit a one-turn **mark** carrying that
node's `backendNodeId`. Mechanics fed to the builder:
1. Marks live in a **parallel `Vec<Mark>` on the Observation**, not a synthetic
   `Eid` variant — keep `Eid` meaning "durable." `Mark { index, backend_node_id,
   role, label_snippet, geometry }`, index positional and **recomputed every
   observation** (explicitly NOT stable — that is the contract).
2. Use a **distinct namespace** so a transient mark is never confused with a
   durable eid in logs or agent prompts (e.g. `m12` / `mark:12`, reserved). Note
   the collision risk with Playwright's `e15` style — keep ours visibly
   different from our own eids.
3. `act` is **unchanged** (D12): add a thin `act_mark(obs, index, Action)` that
   resolves the mark to its carried `backendNodeId` and calls the same path. A
   mark's backendNodeId is captured at observe-time; if the page re-rendered
   before the act, surface `NotHittable`/`UnknownEid` so the agent re-observes —
   marks are single-turn by design, so this is correct, not a bug.
4. **Defer the screenshot/visual SoM to an optional 2.2b escalation**, reserved
   for the genuinely DOM-less case (canvas/WebGL/`<embed>` with no backendNodeId
   to mark at all). Gate it behind a feature so the token-cheap text path stays
   the default and the heavy vision path is opt-in.

## 2026-06-17 — research run 6 (Truffle, 45-min cron): Phase 2.3 token-budget — chars/4 under-counts AX markup, use chars/3.5

**Build verified GREEN before research.** `cargo test --all` = 53 passing
(28 core + 23 cdp + 2 integration); `cargo clippy --all-targets` clean (CI is
`-D warnings`); `cargo fmt --check` clean. CI success on builder run 6
(`bffab18`, "Phase 2.2a: textual transient-mark fallback", run 27667743793,
2m5s). chromiumoxide_cdp 0.9.1 re-confirmed to carry every primitive we depend
on: `GetFullAxTreeParams`, `PushNodesByBackendIdsToFrontendParams`,
`GetBoxModelParams`, `GetContentQuadsParams`, `DispatchMouseEventParams`,
`DispatchKeyEventParams`, `InsertTextParams` — all present in
`chromiumoxide_cdp-0.9.1/src/cdp.rs`. The observe-and-act-and-mark stack stands.

This run sharpens the **top unchecked roadmap item, Phase 2.3 (token-budget
guardrails)**. STATE's existing Next-action says "chars/4 is fine and avoids a
tokenizer dep." Research says: keep the tokenizer-free approach, but **change
the divisor** — chars/4 is the wrong number for *our* payload.

**(a) Verify our repo.** Done — see above. No source touched this run.

**(b) Scan OSS peers — how they bound page-context size.** None of the major
agents send raw HTML; they all bound size with an "interactive/visible elements
only + accessibility-tree (not raw DOM)" filter, and most do NOT expose an
explicit numeric token cap — they rely on the filter and then hit
context-window errors when it isn't enough. That gap is exactly what an explicit
budget guardrail fills.
- **Stagehand (Browserbase):** default representation is the Chrome AX tree via
  `Accessibility.getFullAXTree`; "typically reduces the data size by 80–90%
  compared to raw DOM" (browserbase.com/blog/ai-web-agent-sdk).
- **Playwright-MCP (Microsoft):** `browser_snapshot` returns the AX tree as YAML
  with stable `ref=e5` handles, a fresh snapshot after every action; the
  "omit snapshot to save tokens" request (microsoft/playwright-mcp#1216) was
  closed with no config flag (maintainer: amortized cost is acceptable). Users
  reported a single `browser_navigate` blowing Claude's 25K limit. Compact AX
  snapshot measured ~200–400 tokens/page in third-party analysis.
- **Skyvern:** switched element encoding JSON→HTML for density — one input
  element = 31 tokens (HTML) vs 70 (JSON), ~11.4% net cost cut over ~1,100 tasks
  (skyvern.com blog); still hits `ContextWindowExceededError` at 128K
  (Skyvern-AI/skyvern#1712).
- **browser-use:** filtered tree of interactive/visible elements with
  `highlight_index`; size lever is `viewport_expansion` (`-1` = whole DOM,
  visible-only otherwise) per browser-use#1565.
- **steel-dev:** content-extraction to clean markdown, "up to 80%" cost cut; no
  element/token cap flag surfaced.

**(c) chars-per-token methodology — the load-bearing finding.**
- chars/4 IS the standard rule of thumb and HAS direct tokenizer-free precedent:
  OpenAI docs state "1 token is approximately 4 characters … for English text"
  (developers.openai.com/api/docs/concepts), and **LangChain ships exactly this**
  — `count_tokens_approximately` defaults `chars_per_token = 4.0`
  (reference.langchain.com). So a deterministic chars/N budgeter is well-trodden.
- BUT the 4.0 figure is for English *prose*. For markup-dense text (YAML,
  AX-tree dumps, attribute names, brackets, short refs) the empirical ratio is
  **2.5–3.8 chars/token** (community.openai.com/t/…/622947: Python ≈4.2,
  minified JS ≈2.5, Smalltalk ≈3.3–3.8). BPE merges common English words to one
  token but fragments `[role=button]`/`ref=e5`/indentation into many short
  tokens. **chars/4 therefore systematically UNDER-counts an AX-tree payload.**
- A guardrail must fail safe by *over*-estimating, so it should divide by a
  smaller number. **chars/3.5 is the sound default; chars/3 if we want a hard
  safety margin.** Strong justification that a fixed divisor is reliable for this
  exact payload: "Beyond Pixels: DOM Downsampling for LLM Web Agents"
  (arXiv 2508.04412) measures byte-size↔token-size correlation **r = 0.9994** for
  DOM content — a fixed-divisor estimate is defensible; we just pick the divisor
  on the conservative side of that line.
- AX-vs-screenshot order of magnitude holds: compact AX snapshot ~200–1,000
  tokens vs a screenshot ~1e3 (downscaled) to >200K (full-res); arXiv 2508.04412
  puts a full screenshot ≈1e3 tokens and raw DOM up to ~1e6. The earlier
  "~500 AX vs ~5000 vision" framing is the right order of magnitude, not a
  literal source.

**(d) Recommendation — refine STATE's Phase 2.3 directive; propose D14.**
Keep the no-tokenizer approach and keep both caps — they are sane and
competitive (peers' compact AX snapshots land ~200–1,000 tokens, so 5K baseline
is roomy yet well below the 15K–35K of an *uncompressed* full AX dump and the
25K–200K failure cases peers actually hit; an 800-token diff cap is
appropriately tight for incremental changes). The single change: **estimate with
chars/3.5, not chars/4**, so the guardrail errs toward triggering early. Builder
should: build a `budget` module in `anchortree-core`, `estimated_tokens(s) =
s.chars().count().div_ceil(7) * 2` (= chars/3.5 with integer math, ceil) over
the serialized form of an `Observation` and of a `Diff` in isolation; a
measuring test on a realistic ~40-node observation asserting baseline ≤5,000 and
per-diff ≤800; do NOT add a BPE tokenizer dep. Document the 3.5 divisor choice
(this run's reasoning) in a decision note. This is the quantitative half of the
thesis: durable identity only matters if the diff is cheap enough to send every
turn.

Sources (all dated 2026-06-17 access):
- developers.openai.com/api/docs/concepts (chars/4 rule, PRIMARY)
- reference.langchain.com — `count_tokens_approximately` (`chars_per_token=4.0`, PRIMARY)
- community.openai.com/t/rules-of-thumb-for-number-of-source-code-characters-to-tokens/622947 (markup ratios, empirical)
- arxiv.org/html/2508.04412v1 — Beyond Pixels: DOM Downsampling (byte↔token r=0.9994; screenshot ≈1e3 tok, PRIMARY)
- browserbase.com/blog/ai-web-agent-sdk (Stagehand AX tree 80–90% reduction)
- playwright.dev/mcp/snapshots + github.com/microsoft/playwright-mcp/issues/1216 (AX-YAML, no omit flag, PRIMARY repo)
- skyvern.com blog + github.com/Skyvern-AI/skyvern/issues/1712 (HTML>JSON token cut, context-window error, PRIMARY)
- github.com/browser-use/browser-use/issues/1565 (`viewport_expansion`, PRIMARY repo)

## 2026-06-17 — research run 7 (Truffle, 45-min cron): Phase 2.4 README — the rebind IS the hero, gap confirmed open on both axes (D15)

**Build verified GREEN before research.** `cargo test --all` = 62 passing
(36 core + 23 cdp + 2 integration + 1 doctest); `cargo clippy --all-targets`
clean (`-D warnings`); `cargo fmt --check` clean. CI success on builder run 7
(`1afe959`, "Phase 2.3: token-budget guardrails", run 27669693434, 1m59s). D14
CONFIRMED by the builder (divisor stayed 3.5; 40-element baseline measured **200
tokens**, steady-turn diff **28**). chromiumoxide_cdp 0.9.1 re-confirmed:
`GetFullAxTree`/`PushNodesByBackendIdsToFrontend`/`GetBoxModel`/`GetContentQuads`
all present in `cdp.rs`. The full Phase 2 action loop is proven; the top
unchecked item is **Phase 2.4 — README quickstart**.

**(a) Verify our repo.** Done — see above. No source touched this run.

**(b) Scan OSS peers — README conventions + competitive gap.** Fetched all five
peer READMEs live and verified the gap against primary sources.
- README shape (PRIMARY, the live `main` READMEs): **thesis-first is the norm,
  4 of 5.** Stagehand ("What is / Why Stagehand?"), Skyvern ("Traditional
  approaches … relied on DOM parsing and XPath … which would break whenever the
  website layouts changed"), Playwright-MCP ("structured accessibility snapshots,
  bypassing … screenshots"), browser-use (Rust-core positioning). Only steel-dev
  leads with a tagline+features. Runnable hello-world lands within the first
  screenful (browser-use ~20-line `Agent(task=…)`, Stagehand ~15-line
  `act/extract`); every SDK example **hides the connect-to-browser wiring** behind
  a profile/context object; differentiation is **prose, not tables**
  (Playwright-MCP's explicit "vs CLI" section is the closest model, framed on
  **token efficiency** — "avoid loading … verbose accessibility trees into the
  model context").
- Competitive gap (PRIMARY, confirms our thesis on BOTH axes):
  * Durable identity: Playwright MCP docs state verbatim *"refs are invalidated
    when the page changes"* / *"re-snapshot after navigation"*
    (playwright.dev/mcp/snapshots); Playwright **declined** to persist element
    identity for perf (microsoft/playwright-mcp#1488, NOT_PLANNED — Gozman:
    "Playwright does not store any prebuilt locators … precisely because it's not
    free in terms of performance"). Stagehand `EncodedId = frameOrdinal-
    backendNodeId`, snapshot-scoped, re-grounds via LLM `observe`
    (source-confirmed `lib/v3/types/private/internal.ts`; releases 2.5.9 06-11,
    3.5.0 06-03). browser-use uses per-snapshot integer indices that shift on
    re-render (browser-use#1686).
  * Diff observations: targeted `gh search issues` across stagehand/browser-use/
    playwright-mcp returned **zero** diff/incremental-observation features; the
    peer norm is the opposite (re-snapshot the whole tree each step). Both wedges
    are unoccupied as of 2026-06-17. (Absence-of-evidence from targeted search,
    not an exhaustive crawl.)
- chromiumoxide still exposes everything we depend on (above) — no transport gap.

**(c) Market / trend.** Two sourced observations:
1. **BiDi is in motion (PRIMARY).** microsoft/playwright `main` shows a dense
   June-2026 WebDriver-BiDi stream: prototype-pollution fix in BiDi
   deserialization `722b776` (06-16), MCP moz-firefox BiDi channel `123cc42`
   (06-08), plus a month of Firefox/BiDi test un-skips. BiDi is maturing as the
   cross-browser transport but is NOT displacing CDP for Chromium agent work
   today. Our CDP-only stance is correct now; the `ObservationSource` seam (D9)
   keeps a future `anchortree-bidi` adapter clean. This is the one axis a peer
   could later differentiate on — worth a one-line "CDP today, BiDi-compatible by
   design" note rather than silence.
2. **Cost is two-sided (PRIMARY).** Managed browsers bill per session-minute
   (Browserbase: Developer $20/mo = 100 hrs, Startup $99/mo = 500 hrs;
   browserbase.com/pricing). A no-LLM rebind + diff observation cuts **both** LLM
   tokens **and** billable browser-minutes (fewer round-trips, no re-grounding
   inference). The accessibility-tree-as-context pattern is already consensus and
   already felt as a token-cost pain (Playwright-MCP's own "vs CLI" framing) —
   that pain is exactly the diff-observation wedge.

**(d) Recommendation — propose D15, sharpen ROADMAP/STATE 2.4.** The README is
not a stub; it is the adoption artifact, and it must do the one thing no peer's
hero example does: **demonstrate the rebind.** Concrete outline for the builder
(lifted into STATE Next-action):
1. Title + one-line value prop; thesis paragraph FIRST ("identity, not
   rendering") naming the re-grounding peers with their primary-source behavior.
2. Quickstart within the first screenful: the `chromedp/headless-shell` `docker
   run` line (D11) → one-line CDP connect → `observe` → `obs.render()` +
   `budget::observation_tokens` (show the actual compact text AND the cost) →
   `act`/`act_mark`. **Then the hero: act on `btn-sign-in` → force a re-render →
   act on the *same* id again, no re-observe-for-grounding.** Lift from
   `examples/act_after_rerender.rs` so it cannot drift from compiling code.
3. "How it works" — 3 numbered advantages (Skyvern shape): durable ids, diff
   observations, any-CDP-browser.
4. "anchortree vs the field" — prose (Playwright-MCP shape), framed on token+
   minute cost, citing the named primary sources so the claim is verifiable.
5. One-line "CDP today, BiDi-compatible by design" note.
This locks the positioning the README, the Phase 3 benchmark, and the Phase 4
blog all inherit. No code shape changes — positioning only; builder confirms when
the README lands.

Sources (accessed 2026-06-17): the five peer READMEs on `main` (browser-use,
browserbase/stagehand, Skyvern-AI/skyvern, microsoft/playwright-mcp,
steel-dev/steel-browser); playwright.dev/mcp/snapshots;
github.com/microsoft/playwright-mcp/issues/1488;
github.com/browserbase/stagehand (`lib/v3/types/private/internal.ts`);
github.com/browser-use/browser-use/issues/1686;
github.com/microsoft/playwright commits/main (BiDi, June 2026);
browserbase.com/pricing.

## 2026-06-17 — research run 8 (Truffle, 45-min cron): de-risk Phase 2.5 (resolveNode gotcha) + design the Phase 3.3 benchmark (D16)

**Build verified GREEN before research.** `cargo test --workspace` = 62 passing
(36 core + 23 cdp + 2 integration + 1 doctest); `cargo clippy --all-targets`
clean (`-D warnings`); `cargo fmt --all --check` clean. CI success on builder
run 8 (`e05c5e5`, "Phase 2.4: a README quickstart whose hero demonstrates the
rebind", run 27672292720, 1m59s). D15 CONFIRMED by the builder. **Phase 2's
"alive" deliverable is complete end to end.** chromiumoxide_cdp 0.9.1 re-confirmed:
`GetFullAxTree`/`PushNodesByBackendIdsToFrontend`/`GetBoxModel`/`GetContentQuads`
all present.

**(a) Verify our repo.** Done — see above. No source touched this run.

**(b) Phase 2.5 de-risk — `DOMDebugger.getEventListeners` needs a `RemoteObjectId`,
NOT a `backendNodeId`.** Inspected the actual type in
`chromiumoxide_cdp-0.9.1/src/cdp.rs`: `GetEventListenersParams.object_id` is a
`runtime::RemoteObjectId` (line ~48630), not a node id. So the keep-signal cannot
query a backendNodeId directly — it needs a resolution hop:
`DOM.resolveNode { backend_node_id }` → `RemoteObject { object_id }` →
`DOMDebugger.getEventListeners { object_id }`. `ResolveNodeParams` is present in
the same crate, so no missing primitive — but it is **two CDP round-trips per
candidate node**. This reinforces the existing roadmap ordering: apply the
event-listener signal ONLY to nodes the ARIA-role filter already rejected (the
secondary layer), never to every observed node, or the observe pass pays a
resolve+query per element. Concretely for the builder: in
`fuse::observable_backends()`, role-keep first; for the *residual* (role-less)
candidates, batch-resolve their backendNodeIds and query listeners, keep those
with a bound `click`/`mousedown`/`mouseup`/`keydown`/`change`/`submit` (or
`pointerdown`). This keeps the hot path cheap and makes 2.5 a clean single run.

**(c) Phase 3.3 benchmark — substrate, metric, baseline (the differentiation
proof). No prior art isolates re-identification cost across a re-render; we'd be
defining the metric.**
- **Substrate: WebArena, self-hosted via its official Docker images, harnessed
  through BrowserGym/AgentLab.** It is the only benchmark family that is
  simultaneously self-hostable, bit-deterministic on replay, AND composed of real
  apps (GitLab, a CMS, a forum) that produce *authentic* framework re-renders —
  the exact event anchortree exploits. Reject WebVoyager (arXiv 2401.13919, 15
  live sites) and WebBench (github.com/Halluminate/WebBench, 452 live sites): live
  web = non-reproducible re-render, non-comparable token counts. Reject
  Mind2Web (arXiv 2306.06070) as the *re-render* testbed: its offline HTML
  snapshots are static frozen DOM per step, so they cannot exercise a *live*
  rebind (useful only as a token-size corpus). WebArena: github.com/web-arena-x/
  webarena (arXiv 2307.13854), 812 tasks / 7 Dockerized sites. BrowserGym:
  github.com/ServiceNow/BrowserGym (arXiv 2412.05467) unifies the observation/
  action space and logs per-step token accounting — natural scaffold for an
  A/B of "full a11y snapshot" vs "anchortree diff". (Verify the exact AgentLab
  token-log field name in-repo before relying on it.)
- **Headline metric: LEAD with "LLM re-grounding calls eliminated per re-render"
  (0 vs 1) — model-independent, integer, unarguable.** SUPPORT with "% tokens per
  turn cut" (a tens-of-tokens diff vs a ~15K–35K full a11y re-snapshot), reported
  in one fixed tokenizer, text-only. The % shape matches the established credible
  format in this space: Skyvern headlined **"cut token count by 11.8%, +3.9%
  success"** by sending HTML over JSON across ~1,100 production tasks
  (skyvern.com/blog/how-we-cut-token-count-by-11-…, 2024-08-28). Dollars/latency
  are color only, never the sole headline.
- **Fair baseline = BOTH real peer behaviors, measured separately:**
  (a) **Playwright MCP** — auto-returns a FRESH full accessibility snapshot with
  NEW refs after every action; docs state "Refs are stable within a single
  snapshot… After navigation or DOM updates, the tool returns a fresh snapshot
  with new refs" (playwright.dev/mcp/snapshots). This is the token-volume axis.
  Use its *actual pruned a11y output*, NOT a raw DOM dump, or critics call it a
  strawman.
  (b) **Stagehand** — caches a resolved selector, but "if the DOM shifts and a
  cached action fails, Stagehand re-engages the LLM to figure out the new
  mapping" (browserbase.com/blog/stagehand-caching, 2026-02-24). This is the
  LLM-call axis, and it is exactly the cross-re-render / cache-invalidation case
  where (b) is forced to pay. anchortree beats (a) on token volume and (b) on LLM
  calls — report against both.
- **Confounds to control:** fix the tokenizer (token counts are tokenizer-
  dependent; lead with the model-independent LLM-call count); text-to-text only
  in the primary arms (no screenshots — a screenshot arm is a labeled secondary);
  identical page + identical deterministic re-render + identical task for all
  three; be explicit that the comparison is the *post-re-render of an
  already-seen page* (Stagehand's cache-invalidation case).

**(d) Recommendation — propose D16; sharpen ROADMAP 2.5 + 3.3 and STATE.** Two
forward actions written into the docs: (1) the 2.5 keep-policy now carries the
resolveNode-RemoteObjectId gotcha and the "residual-nodes-only" ordering so the
builder executes without hitting the CDP signature surprise mid-run; (2) D16
pins the Phase 3.3 benchmark design (WebArena/BrowserGym substrate, LLM-calls-
saved headline + %-tokens support, dual real-peer baseline, controlled
confounds) so the highest-leverage thesis-proof arc can start without
re-researching. No prior benchmark isolates re-identification-after-re-render
cost; stating that openly in the eventual writeup is itself credibility.

Sources (accessed 2026-06-17): github.com/web-arena-x/webarena (arXiv 2307.13854);
github.com/ServiceNow/BrowserGym (arXiv 2412.05467); arXiv 2401.13919 (WebVoyager);
arXiv 2306.06070 (Mind2Web); github.com/Halluminate/WebBench;
skyvern.com/blog/how-we-cut-token-count-by-11-and-boosted-success-rate-by-3-9-…;
browserbase.com/blog/stagehand-caching; playwright.dev/mcp/snapshots;
chromiumoxide_cdp-0.9.1/src/cdp.rs (`GetEventListenersParams`/`ResolveNodeParams`).
