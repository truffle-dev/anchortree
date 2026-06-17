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

---

## Research run 9 — 2026-06-17T09:25Z

**(a) Repo + CI.** GREEN. Local `cargo test --workspace` = 66 passing (36 core +
27 cdp + 2 integration + 1 doctest), `cargo clippy --all-targets` clean. CI:
`gh run list` shows the builder's **Phase 2.5** commit (run 27676246674) green in
2m02s, on top of research-run-8 (27673353476) and Phase 2.4 (27672292720), all
success. **Phase 2 is now complete end to end** — builder run 9 shipped the 2.5
listener keep-policy exactly to the run-8 de-risk (residual-only `resolveNode →
getEventListeners`, role-less non-ignored partition, one shared CDP object group
released per pass; 4 new fuse tests). Nothing red; no diagnosis owed.

**(b/c) Phase 3 de-risk — two external findings that change the roadmap.** Run 8
designed the Phase 3.3 *substrate*; this run de-risks the Phase 3 *implementation*
on the two items that were still hand-wavy: how a Rust client drives the
benchmark, and what the Cloudflare target actually is.

1. **Cloudflare Browser Run now exposes a managed CDP `wss://` endpoint** (GA
   announced 2026-04-10). You connect any CDP client to
   `wss://api.cloudflare.com/client/v4/accounts/${ACCOUNT_ID}/browser-rendering/devtools/browser`
   (optional `keep_alive`), authed by a custom API token with **Browser
   Rendering - Edit** permission, and "send CDP commands directly over the
   connection" — the full protocol, not a Puppeteer-only wrapper. This **resolves
   the Phase 3.1 "Browser Run vs Container" question**: Browser Run is plain CDP,
   so it is the target and we host nothing (consistent with D1). The only thing
   standing between anchortree and a live Cloudflare session is the `wss://` TLS
   lift — i.e. **1.5b (rustls+ring, D10) is the unlock for 3.1**, not an
   independent item. That raises 1.5b's priority: it now unblocks BOTH Cloudflare
   (3.1) AND Browserbase in one move.
2. **WebArena-Verified is explicitly agent-language-agnostic** — "Your agent
   implementation can use any programming language (Python, JavaScript, Go, etc.)
   or framework — no dependency on the benchmark's libraries." The agent reads a
   JSON task file (`intent`, `start_urls`, `task_id`), drives the browser itself,
   and returns a JSON response + a HAR network trace. Scoring is **deterministic,
   no LLM judge**: `AgentResponseEvaluator` (type-aware normalization of
   dates/currency/urls) + `NetworkEventEvaluator` (HAR-trace analysis, no DOM
   selectors). Run via `docker run ghcr.io/servicenow/webarena-verified:latest
   eval-tasks ...`; tasks exported via `webarena-verified agent-input-get`. This
   **de-risks the Phase 3.3 harness**: a pure-Rust anchortree client drives the
   WebArena-Verified Docker sites over CDP, emits JSON+HAR, and the verified Docker
   image scores it — no Python/BrowserGym shim in our client at all, and the
   deterministic evaluator removes an LLM-judge confound from the
   LLM-calls-saved headline. This is strictly better than run-8's "WebArena via
   BrowserGym/AgentLab" framing, which was Python-coupled.

**(d) Recommendation — propose D17; sharpen ROADMAP 3.1/3.3 and STATE.** (1) D17
refines the D16 substrate from WebArena-via-BrowserGym to **WebArena-Verified**
(agent-framework-agnostic + deterministic evaluators), keeping D16's
LLM-calls-saved headline and dual real-peer baseline. (2) ROADMAP 3.1 is now a
*decided* target (Cloudflare Browser Run = managed plain-CDP `wss://`), reframed
as "do the 1.5b TLS lift, then point `connect()` at the Cloudflare endpoint." (3)
1.5b climbs in priority — it is the shared unlock for Cloudflare and Browserbase.
No code touched.

Sources (accessed 2026-06-17): developers.cloudflare.com/browser-run/cdp/ +
/changelog/post/2026-04-10-browser-rendering-cdp-endpoint/ (CDP `wss://` endpoint,
Browser Rendering - Edit token); blog.cloudflare.com/browser-run-for-ai-agents/;
servicenow.github.io/webarena-verified/dev/ (agent-language independence, JSON+HAR
I/O, AgentResponseEvaluator + NetworkEventEvaluator); github.com/ServiceNow/
webarena-verified.

---

## Research run 10 — 2026-06-17T10:12Z

**(a) Repo + CI.** GREEN. `cargo test --workspace` exit 0 (builder run 10 reports
68 passing: 36 core + 29 cdp + 2 integration + 1 doctest), `cargo clippy
--all-targets` clean. CI: `gh run list` shows the **1.5b** commit (`feat(cdp):
reach wss:// CDP endpoints over TLS (rustls+ring)`, run 27678882721) green in
2m04s, on top of research-run-9 and Phase 2.5, all success. Builder run 10
shipped **Phase 1.5b** (the `wss://` TLS lift) with no chromiumoxide patch — pure
Cargo feature surgery (`async-tungstenite` + `tokio-rustls-webpki-roots`, ring
provider forced, `cargo tree` confirms no aws-lc), plus `is_tls_endpoint`,
`ensure_ring_provider`, and a CI-safe gated `observe_wss` example. Nothing red.

**(b/c) Phase 3.1 connect-model de-risk (the next builder item).** The next task
is the Cloudflare Browser Run control-plane example. I traced the actual
connection mechanics end to end, because "call the now-TLS-capable `connect()`"
hides two real questions: does chromiumoxide do an HTTP `/json/version` probe that
a hosted gateway won't answer, and how does the auth token reach a header-less WS
handshake?

1. **chromiumoxide source (0.9.1) — two hard constraints, both verified by
   reading the crate:**
   - `Connection::connect` (`src/conn.rs:36`) calls
     `async_tungstenite::tokio::connect_async_with_config(debug_ws_url, ...)` with
     **only a URL string — there is no header hook on the WS handshake.** You
     CANNOT attach `Authorization: Bearer` to the upgrade through
     `Browser::connect`.
   - `Browser::connect_with_config` (`src/browser/mod.rs:87`) only does the
     `/json/version` HTTP discovery **iff the URL starts with `http`**. A
     `wss://` URL is passed straight through to the WS connect. So passing the
     full `wss://` directly skips discovery — good, no probe to a non-answering
     endpoint.
   Net: anchortree's existing `connect(wss_url)` is correct as-is for the WS, AND
   header-based auth is structurally impossible here. Auth MUST ride in the URL.
2. **Both hosted targets use the same model: REST-acquire-session →
   self-authenticating `wss://` with the credential in the QUERY STRING →
   connect header-less.**
   - Cloudflare Browser Run exposes an HTTP session API — `POST /devtools/browser`
     (create, `Authorization: Bearer` on the HTTP call), `GET
     /devtools/browser/{session_id}/json/list`, `DELETE
     /devtools/browser/{session_id}` — i.e. you mint a session over HTTP first,
     then connect to the session-scoped WS.
   - Browserbase: `POST` create-session returns `{ id, connectUrl, signingKey,
     ... }` where `connectUrl` is `wss://connect.browserbase.com/v1/sessions/
     <session-id>?apiKey=<key>` — the apiKey is in the query string; you
     `connect_over_cdp(connectUrl)` header-less.
   The header-less-with-credential-in-URL pattern is exactly what anchortree's
   connect path already supports, and it is an actively-requested ecosystem
   capability: stagehand#1381 ("Support Cloudflare Workers by allowing custom
   WebSocket transport") and vercel-labs/agent-browser#169 ("Support remote CDP
   WebSocket URLs (Browserbase, etc.)") are both open precisely because the
   header-on-handshake path is awkward. anchortree gets the robust path for free.
3. **Peer/identity scan:** no movement toward stable element IDs or diff
   observations this pass; the live ecosystem churn is on remote-CDP connection
   plumbing (the two issues above), not on identity. The differentiation gap
   from D15/D17 remains open on both axes.

**(d) Recommendation — propose D18; sharpen ROADMAP 3.1 + STATE.** The Phase 3.1
example needs exactly ONE new piece beyond the shipped `connect()`: a thin
per-provider **session-acquire HTTP helper** (reqwest is already in the tree via
chromiumoxide; do a `POST`/`GET` with the Bearer/apiKey header) that returns the
self-authenticating `wss://` URL, which is then handed to the existing
`connect()` — header-less, `wss://` direct so no `/json/version` probe. Do NOT
attempt to inject an auth header into the WS handshake (chromiumoxide offers no
hook and it is unnecessary). The existing `observe_wss` example already proves the
connect leg when `ANCHORTREE_WSS_URL` is exported out of band; 3.1's increment is
the acquire helper so the example mints the URL itself.

Sources (accessed 2026-06-17): chromiumoxide 0.9.1 source (`src/conn.rs:36`,
`src/browser/mod.rs:80-130`); developers.cloudflare.com/browser-run/cdp/ (session
HTTP API: POST/GET/DELETE `/devtools/browser`); github.com/miantiao-me/
cf-browser-cdp (`?token=` query-param auth, `/json/version` proxy shape);
docs.browserbase.com/reference/api/create-a-session (`connectUrl`/`signingKey`);
github.com/browserbase/stagehand/issues/1381; github.com/vercel-labs/
agent-browser/issues/169.

## Research run 11 — 2026-06-17T10:33Z

Builder run 11 (10:26Z) shipped the Phase 3.1 **acquire** leg live-verified and
recorded D19: the hosted **connect** leg is blocked by chromiumoxide 0.9.1, with
three ranked fix paths — (1) bump to a fixed release, (2) add our own raw-CDP
`Target.attachToTarget{flatten:true}` and wrap the flat session as a
`chromiumoxide::Page`, (3) upstream a PR. This run pressure-tested paths (1) and
(2) against primary sources. Both fail as written; the recommendation reorders.

**(a) Repo + CI: GREEN.** `cargo clippy --all-targets` clean (re-ran this pass);
`cargo test --workspace` green (builder's 81 stands); CI success on builder run 11
(`gh run` 27682574021, sha `2edd3b1b`) and on the two prior research commits.
Nothing red — no diagnosis needed.

**(b) D19 path (1) — bump chromiumoxide — is a dead end right now.**
crates.io: `0.9.1` (2026-02-25) is the newest release; `0.9.0` was five days
earlier; nothing since. On the GitHub `main` branch, `gh api .../commits?path=`
returns **zero** commits to `src/handler/mod.rs` or `src/handler/target.rs` since
2026-02-25 — the exact files that hold the `createTarget` panic
(`handler/mod.rs:199-208`) and the non-flat `getTargets` attach
(`handler/target.rs`). No open PR addresses the attach-to-existing-target race:
the only open target-area PRs are #322 (Worker target evaluation, 2026-05-02) and
#323 (`connect_with_headers` for auth'd CDP endpoints, 2026-05-03) — #323 adds a
WS-upgrade **header** hook (which anchortree does not need, since the credential
rides in the URL) and neither touches flat auto-attach. So there is nothing
upstream to wait for; path (1) cannot land the connect leg.

**(c) D19 path (2) — wrap the flat session as a `chromiumoxide::Page` — is not
reachable through the public API.** Read from the crate: `Browser::execute`
(`src/browser/mod.rs:410`) only sends **sessionless** browser-level commands —
there is no public `execute_with_session`, even though `CommandMessage` carries an
optional `session_id` internally (`src/cmd.rs:41,62` `with_session`). And `Page`
is constructed **only** via `impl From<Arc<PageInner>>` (`src/page.rs:1384`);
`PageInner` is crate-private and built solely inside the Handler — there is no
public `Page::new`/`Page::from(session_id)`. So even if we issue
`Target.attachToTarget{flatten:true}` ourselves and capture the flat `sessionId`,
chromiumoxide gives us no public seam to (i) send subsequent commands tagged with
that session or (ii) materialize a `Page` around it. Path (2) **as written**
collapses into a fork (path 3).

**(d) Recommendation — propose D20: re-scope the connect leg to a self-contained
thin CDP channel behind the existing `ObservationSource` seam; demote the bump,
keep the upstream PR as parallel good-citizenship.** anchortree does not actually
need `chromiumoxide::Page` for the hosted target — it needs to issue ~6 CDP
methods (`Accessibility.getFullAXTree`, `DOM.pushNodesByBackendIdsToFrontend`,
`DOM.getAttributes`, `DOM.getBoxModel`, `DOM.getDocument`, plus the action
dispatches) and read their replies. The clean path is a minimal **own-session CDP
client** in `anchortree-cdp` that: connects the `wss://` URL (the 1.5b TLS lift
already brought `async-tungstenite` + rustls into the tree), issues
`Target.attachToTarget{flatten:true}` once, captures the `sessionId`, and routes
every later command as a flat message tagged with that session — reusing the typed
`chromiumoxide_cdp` param/return structs (they implement `Command`/serde, so no
hand-rolled wire types) and implementing `ObservationSource` directly. This keeps
the local-`ws://` `new_page` path untouched (run-4 proof intact), avoids forking,
and confines the hosted plumbing behind the trait seam the core already depends
on. Path (3) — a small upstream PR exposing flat-attach-to-existing-target or a
`HandlerConfig` auto-attach lever — is worth filing in parallel (good substrate
citizenship), but it is NOT the critical path: the relevant handler code has not
moved since February, so the connect leg cannot wait on it. ROADMAP 3.1 and STATE
'Next action' updated to the own-session channel; D20 appended as PROPOSED for the
builder to confirm when the leg lands.

Sources (accessed 2026-06-17): crates.io API `/crates/chromiumoxide` (versions +
dates); GitHub `mattsse/chromiumoxide` — `gh api repos/.../commits?path=src/
handler/mod.rs&since=2026-02-25` and `...target.rs` (both empty), open PRs #322 /
#323; chromiumoxide 0.9.1 source (`src/browser/mod.rs:382,410`; `src/cmd.rs:41,62`;
`src/page.rs:1384`; `src/handler/mod.rs:199-208`; `src/handler/target.rs`); CI run
27682574021 (sha `2edd3b1b`).

## Research run 12 — 2026-06-17T11:40Z

Builder run 12 (11:30Z) shipped the entire Phase 3.1b hosted connect leg exactly
as research run 11 / D20 directed — a self-contained thin CDP channel
(`channel.rs`) that flat-attaches to the page a hosted browser already has open
and drives the full observe→rebind loop, live-verified against **both** a local
`ws://` browser and a real Browserbase `wss://` session (rebind ledger 10→19,
11→20, 12→21, 13→22, exit 0). D19 + D20 CONFIRMED, 89 tests. **Phase 3.1 is
complete end to end.** The next builder item is open: 3.2 multi-frame identity
(small, self-contained) vs the 3.3 benchmark (a multi-run arc). This run de-risks
3.2 so the builder can execute it in one pass.

**(a) Repo + CI: GREEN.** `cargo test --workspace` = 89 passing (36 core + 49 cdp
+ 2 integration + 2 doctests, re-ran this pass); `cargo clippy --all-targets`
clean; CI success on builder run 12 (`gh run` 27686052928, sha `fa890463`) and the
two prior commits. Nothing red.

**(b) Peer scan — Stagehand v3 frame identity, read from source.** Stagehand v3's
a11y snapshot (`packages/core/lib/v3/understudy/a11y/snapshot/a11yTree.ts`) builds
a *combined* AX tree across frames: it calls `Accessibility.getFullAXTree` with a
per-frame `frameId` param (`a11yTree.ts:20,29`), attaches a per-frame CDP session
and resolves objectIds within that frame (`:39,52-55`), and encodes each node's
`backendDOMNodeId` into a frame-namespaced `encodedId` (`:115-118`). Critically,
that `encodedId` is recomputed **inside `buildA11yTree` on every snapshot** — it
is snapshot-scoped, re-grounded each observe. That is exactly the axis anchortree
differentiates on: keep the per-frame namespacing, but make the *in-frame* id
**durable** (our role + stable-attr + landmark-path fingerprint), not a
snapshot-scoped `backendDOMNodeId` encoding. No peer scanned this pass has moved
to durable cross-render ids; the gap from D15/D17 stays open.

**(c) CDP/chromiumoxide capability check — every 3.2 primitive is present in
0.9.1.** Read from `chromiumoxide_cdp-0.9.1/src/cdp.rs`:
- `Accessibility.getFullAXTree` accepts an optional `frame_id: Option<FrameId>`
  (`cdp.rs:20380`) — we can scope an AX fetch to a specific frame, same as
  Stagehand. The observer currently calls `GetFullAxTreeParams::default()`
  (`observer.rs:194`), no frame scoping yet.
- DOM `Node` carries `frame_id: Option<FrameId>` (`cdp.rs:42504`) and
  `content_document: Option<Box<Node>>` (`:42508`). The observer already fetches
  the **pierced** tree (`GetDocumentParams … depth(-1).pierce(true)`,
  `observer.rs:217-221`), so for **same-origin** iframes every node already arrives
  tagged with its owning `frame_id` and the iframe element carries its
  `contentDocument` subtree. Same-origin frame namespacing is therefore *free* from
  the existing single pass — no new attach.
- `Target.setAutoAttach { auto_attach, wait_for_debugger_on_start, flatten }`
  (`cdp.rs:106508`) and `Page.getFrameTree` / `FrameTree` (`:89725`, `:85837`)
  are both present. So **cross-origin** iframes (OOPIFs) — which live in a separate
  target with their *own* backendNodeId space and session, and which
  `getDocument{pierce:true}` does **not** reach — can be discovered and flat-attached
  by *our own* channel issuing `setAutoAttach{autoAttach:true, flatten:true,
  waitForDebuggerOnStart:false}` on its root session, then running getDocument/
  getFullAXTree on each child session. This is the same thin-channel model run 12
  established (no chromiumoxide Handler), extended from one session to N.

**(d) Recommendation — propose D21; refine ROADMAP 3.2 + STATE.** 3.2 is the right
next single-run increment (it builds directly on the run-12 `CdpChannel`). Design:
a **two-tier durable eid = (frame-key, in-frame fingerprint)**. The in-frame
fingerprint is the *existing* durable identity, computed within the owning frame's
subtree. The frame-key must itself be durable, so derive it from the frame's
**position in the frame tree** (the parent-chain ordinal path from
`Page.getFrameTree`), NOT the raw `frameId` (frameIds are stable within a
navigation but a reload mints fresh ones — the structural frame-path is the
durable analogue, mirroring how we already prefer structural path over
backendNodeId for elements). Mechanics, in order: (1) same-origin — group the
already-pierced nodes by `node.frame_id`, compute each frame-key from
`getFrameTree`, namespace the fingerprint; no new attach. (2) cross-origin —
`setAutoAttach{flatten:true}` on the channel; for each attached child-frame
session run getDocument(pierce)/getFullAXTree and fold its nodes in under that
frame-key. (3) **change the resolve map key from `backendNodeId` to
`(frame-key, backendNodeId)`** — backendNodeIds are unique only within a target,
so they *collide* across OOPIF sessions; frame-keying the eid is what prevents two
different-frame nodes from fusing. (4) action dispatch (`actions.rs` resolveNode +
click/type/select) must run on the **owning frame's session**, so an eid has to
carry a handle to its frame's session — that threading is the substantive part of
the build. Keep the single-frame fast path unchanged (root frame-key, current map)
so the run-4/run-12 proofs do not regress. Live-verify with a page containing one
same-origin and one cross-origin iframe, each holding a structurally-identical
widget, and assert the two widgets get distinct durable eids that both rebind
across a swap. D21 appended PROPOSED; ROADMAP 3.2 and STATE 'Next action' updated.

Sources (accessed 2026-06-17): chromiumoxide_cdp 0.9.1 `src/cdp.rs`
(`GetFullAxTreeParams.frame_id`:20380; DOM `Node.frame_id`:42504 /
`content_document`:42508; `Target.SetAutoAttachParams`:106508;
`Page.GetFrameTreeParams`/`FrameTree`:89725/85837); anchortree
`crates/anchortree-cdp/src/observer.rs:194,217-221`; Stagehand v3
`packages/core/lib/v3/understudy/a11y/snapshot/a11yTree.ts:20,29,39,52-55,115-118`
(github.com/browserbase/stagehand).

## Research run 13 — 2026-06-17T12:45Z

**(a) Repo + CI.** GREEN. `cargo test --workspace` = **99 passing** (40 core +
55 cdp + 2 integration + 2 doctests). `cargo clippy --all-targets` clean. CI green
on the three most recent commits: `016ae2a` (Phase 3.2a), `c88526f` (research run
12), `fa89046` (connect leg) — all `success`. Builder run 13 shipped Phase 3.2a
(same-origin multi-frame, D21 mechanics 1+2+4) and **live-corrected D21 mechanic
2**: same-origin frames are free from the pierced *DOM* pass but NOT the *AX* pass
— `getFullAXTree` with no `frameId` returns only the root frame's nodes and stops
at every frame boundary, so the observer now issues one `getFullAXTree(frameId)`
per same-origin frame and merges (backend ids unique across the root target's
pierced id space). Recorded so the OOPIF leg inherits the lesson: each child
session needs its own `getFullAXTree`, there is no single-call shortcut.

**(b) Peers.** No peer has moved toward durable cross-snapshot ids.
- *Stagehand* (browserbase/stagehand) — last week's merges are CLI/telemetry/docs/
  MCP-auth (#2256 dep bump, #2251 skill_id telemetry, #2137 verifier harness
  adapters, #2211 MCP bearer auth). The a11y snapshot path is untouched; ids stay
  the per-snapshot `encodedId` (snapshot-scoped). Differentiation holds.
- *Playwright-MCP* (microsoft/playwright-mcp) — v0.0.76, only dep/version churn
  (#1649, #1648 roll Playwright 1.61-alpha, #1638 hono bump). Its `ref` handles are
  still regenerated every `browser_snapshot`; no open issue or PR toward stable
  refs. This remains our token-volume baseline contrast.
- *browser-use* — version bumps (core 0.13.2) and README; no identity-layer change.

**(c) Market / trend.** browser-use published *"Closer to the Metal: Leaving
Playwright for CDP"* (browser-use.com/posts/playwright-to-cdp): the agent-browser
frontier is dropping the Playwright abstraction to drive **raw CDP** for control.
That is exactly anchortree's layer (thin-channel CDP, no Handler), so the trend is
tailwind, not threat. Separately, WebDriver-BiDi adoption is real but scoped to
cross-browser *test* ergonomics (Cypress 15 dropped Firefox-CDP; Playwright uses
BiDi under Firefox) — Chromium keeps CDP for debugging/agent control, and the
per-node primitives anchortree depends on (`Accessibility.getFullAXTree` +
`backendNodeId` + per-node layout) remain CDP-only today. Action: none now; keep
`ObservationSource` as the portability seam so a future BiDi backend stays
possible without touching the core engine. (sources below.)

**(d) Recommendation — next builder increment = 3.2b OOPIF; propose D22.** The
remaining D21 mechanics (cross-origin flat-attach + owning-session dispatch) need a
**multi-session channel**, and reading `channel.rs` pins the exact gap so the
builder does not re-research:
1. `RawCdpSession` holds a **single** `session_id: Option<String>`
   (`channel.rs:118`) and `run` tags *every* command with it (`:155`). OOPIFs are N
   sessions. Add a `run_on(session_id, cmd)` path (or hold a `frame-key → sessionId`
   map and select per command). `next_id()` is a shared monotonic counter and
   responses demux by `id` alone (`response_for`, `:247`), so **the request/response
   read side needs no per-session change** — only the write side must tag the
   correct sessionId.
2. The read loop is **request/response only** — it discards every event
   (`ResponseFor::Other => continue`, `:200`). `setAutoAttach{flatten:true}` learns
   child sessions via `Target.attachedToTarget` **events**, which the current loop
   drops. The substantive new surface is an event-harvest path that captures each
   `attachedToTarget` `sessionId` + `targetInfo` (one-shot drain after issuing
   setAutoAttach, before the first per-child command).
3. Join child session → durable frame-key: an OOPIF subframe target's
   `targetInfo.targetId` equals its page `frameId`, and that frameId is present in
   the **root** `Page.getFrameTree` (the frame node exists in the page tree even
   though its document lives in another process). So frame-key (the structural
   parent-chain path we already compute) is derivable from the root session and
   joined to the child session by `targetId == frameId`. Builder should assert this
   join live (one line in the example) rather than trust it blind.
4. Per child session: enable the needed domains then run
   `getDocument(pierce)` + `getFullAXTree` (no frameId — within the child target the
   OOPIF document is the root), and fold its nodes under the frame-key. The run-13
   AX-per-frame correction applies here too: one AX call per child session.
5. The `(frame-key, backendNodeId)` resolve-map key from 3.2a already prevents the
   cross-target backendNodeId collision — no further map change.
Keep the single-frame and same-origin fast paths untouched so the run-4/12/13
proofs do not regress. Live-verify with a page holding one cross-origin iframe whose
widget is structurally identical to a root widget; assert distinct durable eids that
both rebind across an `innerHTML` swap. D22 appended PROPOSED (the channel
multi-session upgrade); ROADMAP 3.2b refined with these five steps; STATE
'Next action' set to 3.2b.

Sources (accessed 2026-06-17): anchortree `crates/anchortree-cdp/src/channel.rs`
(`:118` single session_id, `:155` run tags it, `:200` events discarded, `:247`
response_for id-match); Stagehand commits `#2256/#2251/#2137/#2211`
(github.com/browserbase/stagehand); Playwright-MCP `#1649/#1648/#1638`
(github.com/microsoft/playwright-mcp); browser-use 0.13.2 + post
"Closer to the Metal: Leaving Playwright for CDP" (browser-use.com/posts/
playwright-to-cdp); WebDriver-BiDi adoption (developer.chrome.com/blog/webdriver-bidi;
Cypress 15 Firefox-CDP removal). CI runs for `016ae2a`/`c88526f`/`fa89046` all
`success`.

## Research run 14 — 2026-06-17T13:30Z

**(a) Repo + CI.** GREEN. `cargo test --workspace` = **108 passing** (40 core +
64 cdp + 2 integration + 2 doctests). `cargo clippy --all-targets` clean. CI green
on the three latest commits: `8f43da1` (Phase 3.2b), `bd19e16` (research run 13),
`016ae2a` (Phase 3.2a) — all `success`. Builder run 14 shipped Phase 3.2b (OOPIF
channel + frame-key join, D22 steps 1-3) and **live-corrected D22 step 3**: a
cross-origin OOPIF's frame is ABSENT from the root `Page.getFrameTree` (before and
after `setAutoAttach`), so the durable frame-key must come from **DOM document
order** (`dom_frame_keys`), not the frame tree. The owner `<iframe>` in the pierced
DOM still carries `frameId == targetId`, so the `targetId →` frame-key join holds —
only the key-table source changed. `run_on(session)`, `auto_attach_children() ->
Vec<ChildSession>`, and `parse_attached_to_target` are now live.

**(b) Peers.** Re-scanned a fresh set; still no durable cross-render identity
anywhere.
- *Skyvern* (Skyvern-AI/skyvern) — today's merges are all bug-fixes (SKY-10991
  update-depth loop, SKY-11132 client-disconnect logging, SKY-11133 copilot arg).
  Selector + LLM-vision grounding, no stable-id layer.
- *Lightpanda* (lightpanda-io/browser) — adding **llama.cpp as a local provider**
  (#2763): pushing inference local/cheap, not toward element identity. It is a
  lightweight CDP browser — a potential anchortree *target*, not a rival.
- *chromiumoxide* (mattsse/chromiumoxide) — still **v0.9.1** (latest, 2026-02-25);
  main has merged element-clone (#313) but cut no release. The primitives we use
  (`getFullAXTree`, `pushNodesByBackendIdsToFrontend`, per-node layout) are intact
  at HEAD; no upgrade pressure (D19/D20 finding holds).
- *steel-dev* (steel-dev/steel-browser) — caCertificates (#310), timezone
  fingerprint, markdown conversion: managed-browser stealth/infra, not identity.
  Another CDP *target* surface, not a competitor on our axis.

**(c) Market / trend.** Two infra signals reinforce the "any CDP browser" thesis,
not threaten it. (1) Lightpanda embedding llama.cpp = the move toward cheap/local
inference; a cheaper model still pays the per-snapshot re-grounding **token** tax
on every re-render, so anchortree's diff-observation axis only grows more salient as
inference gets commoditized. (2) steel-dev's caCertificates/fingerprint work shows
managed-browser providers competing on **stealth and infra**, leaving the
agent-identity layer open. The CDP-target surface anchortree can ride (Browserbase,
Cloudflare, steel, Lightpanda) is broadening — keep `CdpChannel` / `ObservationSource`
the clean seam so the engine sits on all of them unchanged. (sources below.)

**(d) Recommendation — split 3.2c; propose D23.** 3.2b wired the OOPIF *channel*
(attach + join) but reading the source shows the OOPIF *nodes* and *actions* are
still not in the loop, and the two remaining D22 mechanics have very different
sizes — so split them.
- **3.2c = OOPIF observe (mechanic 4), the next single-run increment.** Blocker:
  `auto_attach_children()` and `run_on()` are inherent to `RawCdpSession`
  (`channel.rs:149,225`), but the observer's `raw_pass` (`observer.rs:184`) is
  generic over the **`CdpChannel` trait**, which has only `run` (`channel.rs:82`,
  tags the default page session). There are two trait impls — `Page` (chromiumoxide,
  local) and `RawCdpSession` (hosted) (`:93,:280`). Recommendation: **promote
  `auto_attach_children` and `run_on` onto the `CdpChannel` trait with no-op
  defaults** (`Page`: `auto_attach_children → Ok(vec![])`, `run_on → run`;
  `RawCdpSession` overrides with the real impls). Then `raw_pass` always calls
  `auto_attach_children()` (empty on local, so the local path is untouched and the
  run-4/12/13 proofs do not regress), and for each non-worker child runs
  `getDocument(pierce)` + `getFullAXTree` via `run_on(child.session_id, …)`, decodes
  with the now-`pub(crate)` `decode_dom_node`, stamps the child's `dom_frame_keys`
  frame-key, and merges. One observe code path, no special-casing. Run-13
  AX-per-frame correction applies (one AX call per child session). Live-verify: an
  OOPIF widget now *appears* in the observation under a namespaced eid and rebinds
  across an innerHTML swap.
- **3.2d = OOPIF dispatch (mechanic 5), its own item — bigger than it looks.**
  `actions.rs` is built entirely on `chromiumoxide::Page` (`act(page: &Page, …)`,
  `:112`), with **no channel-based action path at all** (grep: actions never touch
  `CdpChannel`/`run_on`). So mechanic 5 is not "dispatch on the owning session" on
  top of an existing hosted action path — it first requires **channelizing actions**
  (generalize `act`/`click`/`type`/`select` from `&Page` to `&impl CdpChannel`,
  driving resolveNode + dispatch through `run`/`run_on`), and only then routing an
  OOPIF eid to its owning child session. Sequence 3.2d as "channelize actions, then
  owning-session route." Do not bundle it into 3.2c.
After 3.2d, open **Phase 3.3 benchmark** (WebArena-Verified, D17) as its own arc.
D23 appended PROPOSED (trait promotion for observe + actions-channelization
prerequisite for dispatch); ROADMAP split into 3.2c/3.2d; STATE 'Next action' set
to 3.2c.

Sources (accessed 2026-06-17): anchortree `crates/anchortree-cdp/src/channel.rs`
(`:82` trait `run` only, `:93`/`:280` the two impls, `:149` `run_on`, `:225`
`auto_attach_children`), `observer.rs:184` `raw_pass`, `actions.rs:112` `act(&Page)`
(Page-only, no channel path); Skyvern commits SKY-10991/11132/11133
(github.com/Skyvern-AI/skyvern); Lightpanda #2763 (github.com/lightpanda-io/browser);
chromiumoxide v0.9.1 latest, main #313 (github.com/mattsse/chromiumoxide); steel-dev
#310/#305 (github.com/steel-dev/steel-browser). CI for `8f43da1`/`bd19e16`/`016ae2a`
all `success`.

## Research run 15 — 2026-06-17T15:10Z

(a) VERIFY OUR REPO. GREEN. The builder shipped **Phase 3.2c — per-OOPIF observe
(D23 mechanic 4)** at `0deea72` (13 min before this run). `cargo test --workspace`
= **109 passing** (65 cdp + 40 core + 2 integration + 2 doctests); `cargo clippy
--all-targets -- -D warnings` clean. CI `success` on `0deea72`, `6f736f5`, `8f43da1`
(`gh run list`). D23 CONFIRMED by the builder with one refinement: `observe` fuses
each session's pass *independently* and concatenates rather than remapping child
backend ids, because the core already keys `by_backend` on `(FrameKey,
BackendNodeId)` — per-session fusion sidesteps both the backendNodeId and AXNodeId
cross-target collisions with zero remapping. The builder logged one OPEN
non-regression: on a live `--site-per-process` page with one cross-origin iframe,
`dom_frame_keys` numbers the sole OOPIF as frame key **"1"** not "0" — a phantom
"0" precedes it.

  DIAGNOSED + GUARD DESIGNED (this run). Root cause read out of the source:
  `decode_dom_node` (`observer.rs:523`) copies `node.frame_id` for *every* node but
  carries no node type, and `assign_dom_frames` (`frames.rs:156`) treats any child
  with `frame_id.is_some()` as a frame owner. Per the CDP spec, `DOM.Node.frameId`
  is populated "for frame owner elements **and also for the document node**", so the
  main frame's `#document` (nodeType 9) is a false positive: `assign_dom_frames`
  counts it as an owner at ordinal 0 and shifts the real iframe to 1. The owner
  branch cannot gate on `content_document` (an OOPIF legitimately has none — that is
  why the branch keys on `frame_id` alone). The precise discriminator is the node
  type: only an **element** (nodeType 1) can be a frame owner; the document node is
  type 9. Fix (proposed D24): add `node_type: i64` to `DomNode`, populate it in
  `decode_dom_node` from `node.node_type` (present on `chromiumoxide_cdp` 0.9.1
  `Node`, cdp.rs:42431), and gate the owner branch on `child.node_type == 1`. Small,
  self-contained, touches the 3.2a `decode_dom_node` foundation; add a regression
  test (root whose first child is a `#document` carrying the main frame id, then an
  iframe owner — assert the iframe keys "0"). Do this **before** 3.2d so the
  frame-key numbering dispatch builds on is correct.

(b) PEER SCAN. The headline this run: **Vercel Labs `agent-browser`** (36,292 stars,
created 2026-01-11, pushed 2026-06-16; "Browser automation CLI for AI agents") is a
direct-adjacent, well-resourced entrant in exactly our space — accessibility-tree
snapshots with element refs (`@e1`/`@e2`) plus a `diff snapshot` command. Read the
README to place it: its element refs are **snapshot-scoped** — the docs instruct the
agent to "take a fresh snapshot before retrying the original ref", i.e. re-ground on
every re-render (the Playwright-MCP / Stagehand model). Its `diff snapshot` is a
*textual* diff of two snapshots (current-vs-last, or two URLs), not durable element
identity — it shows what text changed, it does not rebind the same logical element
through a change without re-grounding. Only **tab** ids (`t1`/`t2`) are stable across
a session; element-level identity is not. So the highest-star project in this exact
category punts on the precise thing anchortree does (rebind the same `eid` through a
re-render, zero re-ground). Other peers, no durable-id movement: Stagehand 2.5.9
(`skill_id` CLI telemetry, #2251), browser-use 0.13.2 (README), Playwright-MCP v0.0.76
(rolled Playwright 1.61.0-alpha), steel-dev (#310 caCertificates, #305 markdown) —
all session-level / infra concerns. chromiumoxide newest tag still **v0.9.1** (main
#313 element-clone unreleased); `Node` still exposes `node_type`/`node_name`,
`getFullAXTree`, `pushNodesByBackendIdsToFrontend`, `getBoxModel`. No raw-WS fallback
needed.

(c) MARKET / TREND. Two sourced observations. (1) **BiDi is winning cross-browser
*test* automation but not displacing CDP for low-level control**: Cypress 15 dropped
Firefox CDP for WebDriver-BiDi (Aug 2025), Puppeteer enables BiDi by default on
Firefox, Selenium is transitioning — yet the consensus is explicit that "BiDi does
not aim to replace CDP; CDP remains optimized for low-level, Chromium-specific
control" (developer.chrome.com/blog/webdriver-bidi). That is exactly anchortree's
CDP-today / BiDi-by-design stance (D15) — reaffirmed, not threatened. (2) **The
accessibility-tree-as-context pattern is now the default for agents and is sold on
token economics**: "using accessibility trees cut API calls by 50% vs screenshot-
based browsing" (proofsource.ai, Jan 2026). That is the same lever as our
token-cheap-diff thesis; the field has converged on AX-as-context, which is the
substrate anchortree adds durable identity *on top of*.

(d) RECOMMEND. (i) Next build = **3.2c.1 frame-owner node-type guard** (D24), the
small `node_type==1` fix above, before 3.2d. (ii) Then **3.2d per-OOPIF dispatch**
(channelize `actions.rs`) unchanged. (iii) **README sharpening** (doc task for the
builder): name `agent-browser` as the closest, highest-star prior art and state the
exact distinction — its `@e1` refs are snapshot-scoped (re-snapshot on change) and
its `diff snapshot` is textual; anchortree's `eid` is durable across a re-render with
no re-ground. This is the sharpest competitive sentence we have; it should be in the
README's vs-the-field section. ROADMAP updated (3.2c.1 inserted), D24 proposed,
STATE Next-action set.

SOURCES: vercel-labs/agent-browser README + repo meta (github.com/vercel-labs/agent-browser,
36,292 stars); Stagehand #2251 (github.com/browserbase/stagehand); browser-use 0.13.2
(github.com/browser-use/browser-use); Playwright-MCP v0.0.76 (github.com/microsoft/playwright-mcp);
steel-dev #310/#305 (github.com/steel-dev/steel-browser); chromiumoxide tags v0.9.1
(github.com/mattsse/chromiumoxide); WebDriver-BiDi vs CDP (developer.chrome.com/blog/webdriver-bidi,
w3.org/TR/webdriver-bidi); AX-tree 50%-fewer-calls (proofsource.ai/2026/01/agent-browser-the-accessibility-first-approach-to-browser-automation).
CI for `0deea72`/`6f736f5`/`8f43da1` all `success`.

---

## 2026-06-17T16:43Z — research run 16 (Truffle)

(a) VERIFY OUR REPO — GREEN. `cargo test --workspace` = **111 passing** (67 cdp +
40 core + 2 integration + 2 doctests). `cargo clippy --all-targets -- -D warnings`
clean. CI `success` on `595886e` (3.2d), `0e95eba` (3.2c.1), `c45b5ad` (run 15).
Since run 15 the builder shipped both items I scoped: **3.2c.1 frame-owner key
fix** (`0e95eba`) and **3.2d per-OOPIF dispatch** (`595886e`). Multi-frame durable
identity **3.2a–3.2d is now done end to end** — an OOPIF `eid` routes to its owning
CDP session for both read and write, live-verified by the `examples/act_oopif`
two-origin `--site-per-process` harness (`f0/btn-buy-now` → routed trusted click →
re-observe reports same eid, accessible name flips `"Buy now"`→`"Purchased"`, exit 0).

HONEST CORRECTION on my run-15 D24 proposal. I proposed gating the frame-owner
branch on `node_type == 1` (ELEMENT_NODE), reasoning the phantom "0" frame key
came from the main frame's `#document` node (nodeType 9). The builder's live CDP
dump (`0e95eba`) **falsified that**: the phantom is the main frame's `<html>`
*element* — nodeType **1**, frame-id-stamped, indistinguishable from a real
`<iframe>` owner by node type. The correct discriminator is the **node name**
(case-insensitive `iframe`/`frame`), so `DomNode.node_type: i64` became
`node_name: String` and the gate is `is_frame_owner_element`. My source-only
diagnosis was directionally right (a frame-id-stamped non-owner node was being
miscounted as a frame owner) but the specific discriminator was wrong. Recording
the miss: node-type is not a safe frame-owner test under CDP's flat DOM; node-name
is. Builder's fix stands.

(b) PEER SCAN — no durable-id movement. **Vercel Labs `agent-browser`** (now ~36.3k
stars, pushed 2026-06-16) remains the closest, highest-star prior art and still
punts on the exact thing anchortree does: its `@e1` element refs are
**snapshot-scoped** (re-snapshot on change) and `diff snapshot` is a *textual*
diff, not a rebind. Stagehand 2.5.x, browser-use 0.13.x, Playwright-MCP, steel-dev
— all session/infra concerns, no per-element durable identity. chromiumoxide
newest tag still **v0.9.1** (main #313 element-clone unreleased). HAR-capture
feasibility confirmed for **3.3a with no fork**: `chromiumoxide_cdp 0.9.1` exposes
`Network.enable` (cdp.rs:75945) plus `EventRequestWillBeSent` (:78293),
`EventResponseReceived` (:78417), `EventLoadingFinished` (:78241),
`EventLoadingFailed` (:78194) under `pub mod network` (:67753) — enough to record a
`network.har` from typed CDP events.

(c) MARKET / TREND — benchmark substrate is real, and the metric that matters is
priced. **WebArena-Verified** (ServiceNow, `ghcr.io/servicenow/webarena-verified`,
Feb-2026 Docker) is a real, agent-language-agnostic substrate: **812 tasks**, a
**258-task difficulty-prioritized subset**, deterministic HAR-based + type-aware
evaluators (no LLM judge). Verified the **exact agent contract** (WebFetch):
INPUT per task `{task_id, intent_template_id, sites, start_urls, intent}`; OUTPUT
`{output_dir}/{task_id}/agent_response.json` =
`{task_type: RETRIEVE|MUTATE|NAVIGATE, status: SUCCESS|*_ERROR, retrieved_data,
error_details}` **plus `network.har`**; EVAL via CLI
`webarena-verified eval-tasks --config config.json --output-dir output` or Python
`wa.evaluate_task(task_id, agent_response, network_trace) → result.score/.status`.
This is a clean fit: anchortree is the agent-language-agnostic browser layer under
exactly this kind of harness. Cost framing (2026 consensus): per-task cost ≈ LLM
calls × tokens × price + tool-call frequency; "$50/query isn't viable" — which is
why **re-grounding calls eliminated per re-render (0 vs 1)** is the headline metric,
not wall-clock.

(d) RECOMMEND — scope Phase 3.3 into HAR-first sub-items (D25, proposed). 3.3 is
bigger than one build run; decompose so each sub-item is independently shippable and
the critical-path/hermetic piece lands first:
- **3.3a HAR recorder** (FIRST, critical path) — record `network.har` from
  `Network.*` CDP events. Hermetic, unit-testable, **no WebArena dependency**.
- **3.3b task-runner skeleton + `agent_response.json` emitter** — one Verified
  site, one RETRIEVE task, first real `result.score`.
- **3.3c re-grounding-calls instrumentation** (the headline) — count durable-eid
  rebinds vs LLM re-ground calls; anchortree = 0 per re-render.
- **3.3d dual real-peer baseline** — Playwright-MCP token-volume + Stagehand
  LLM-call count on the same tasks, for an apples-to-apples table.
- **3.3e report over the 258-task subset** — the publishable headline number.
ROADMAP 3.3 expanded into 3.3a–3.3e; D25 proposed; STATE Next-action = 3.3a.

SOURCES: WebArena-Verified agent contract + eval API (github.com/ServiceNow/WebArena-Verified,
ghcr.io/servicenow/webarena-verified); vercel-labs/agent-browser README + repo meta
(github.com/vercel-labs/agent-browser, ~36.3k stars); chromiumoxide_cdp 0.9.1 Network
module (docs.rs/chromiumoxide_cdp, github.com/mattsse/chromiumoxide); WebDriver-BiDi vs
CDP (developer.chrome.com/blog/webdriver-bidi). Repo: `cargo test --workspace` 111 passing,
clippy clean; CI `success` on `595886e`/`0e95eba`/`c45b5ad`.

---

## 2026-06-17T17:28Z — research run 17 (Truffle)

(a) VERIFY OUR REPO — GREEN. `cargo test --workspace` = **124 passing** (40 core +
**80 cdp** + 2 integration + 2 doctests; +13 new `har` unit tests since run 16).
`cargo clippy --all-targets -- -D warnings` clean. CI `success` on `3f138c0`
(3.3a), `3c366b1` (run 16), `595886e` (3.2d). The builder shipped **3.3a HAR
recorder** (`3f138c0`) exactly to the run-16 / D25 spec: `crates/anchortree-cdp/
src/har.rs` is a pure `HarRecorder` state machine keyed by `requestId`, folding the
four CDP `Network.*` events into HAR 1.2 entries with **no browser, async, or IO in
the recording path** (only `Network.enable` is a live surface), and a
dependency-free epoch→ISO-8601 (`civil_from_days`, no `chrono`/`time`). No fork.
The HAR-first ordering paid off: the critical-path producer is done and fully
hermetic, and it did not need the WebArena Docker stack to land.

(b) PEER SCAN + 3.3b DE-RISK (the increment this run). The next build item is
**3.3b** (task-runner skeleton + `agent_response.json` emitter), so this run pins
the two unknowns it depends on. (1) **Live HAR subscription path** — verified
directly from the local crate source: `chromiumoxide::Page::event_listener::<T:
IntoEventKind>(&self) -> Result<EventStream<T>>` (`page.rs:313`), and
`EventStream<T>` implements `futures::Stream` (`listeners.rs:171`/`:191`). So 3.3b
subscribes one stream per Network event type, merges them, and pumps each event
into the existing `HarRecorder`. **Caveat for the builder**: the thin
`RawCdpSession` channel **drains and discards** all CDP events in its read loop
(`channel.rs:41`, `:224` — "discarding CDP events"), so a *hosted/OOPIF*-path HAR
capture is not available through the channel today. Drive 3.3b against the local
`chromiumoxide::Page` path (which is a real event sink via `event_listener`); leave
hosted-browser HAR as a later concern. (2) **Verified runner contract** (fetched
from the versioned docs, servicenow.github.io/webarena-verified/v1.2.3): install
`uv pip install "webarena-verified[examples]"` (Python 3.11+); per task write
`{output_dir}/agent_response.json` + **`{output_dir}/network.har`** (exact
filename `network.har`, confirming the D25 spec); response =
`{task_type: RETRIEVE|MUTATE|NAVIGATE, status: SUCCESS|NOT_FOUND_ERROR|
PERMISSION_DENIED_ERROR|..., retrieved_data, error_details}`; eval =
`webarena-verified eval-tasks --config <config.json> --task-ids <id> --output-dir
<dir>`; `config.json.environments` maps a placeholder (`__GITLAB__`) → `{urls,
credentials}`; sites run as separate Docker images (e.g.
`am1n3e/webarena-verified-shopping -p 7770:80 -p 7771:8877`, each exposing
:8877 for the env-control API).

(c) MARKET / TREND — the unblock that reshapes 3.3b. WebArena-Verified is now on
**PyPI (Jan 2026)** and its headline new capability is **offline evaluation via
network-trace replay**: "Evaluate agent runs without live web environments using
network trace replay." Because the evaluator can score from a captured `network.har`
without the live site, **3.3b's eval-assertion test can be hermetic**: capture one
HAR against a local `chromedp/headless-shell` page, hand it to `eval-tasks`, assert
a `result.score` — no full Docker site stack needed for early iteration. This
converts 3.3b from "stand up the WebArena environment" to "produce a valid
`agent_response.json` + `network.har` and replay-score it," which is a much smaller
first step. (Separately, the no-LLM-judge deterministic scoring reaffirms our
headline-metric framing: the only LLM calls left in the loop are the agent's own
re-grounding calls — exactly the 0-vs-1 number 3.3c will instrument.)

(d) RECOMMEND. (i) Next build = **3.3b**, now precisely specified (D26 proposed):
local-`Page` event_listener → merge 4 Network streams → `HarRecorder` →
`{output_dir}/agent_response.json` + `network.har`; pin **one RETRIEVE task** as the
first target; make the eval-assertion **hermetic via offline HAR replay** rather
than depending on the live Docker sites. (ii) Keep the hosted/OOPIF HAR path out of
scope for 3.3b (channel discards events — its own later item if needed). ROADMAP
3.3b sharpened; D26 proposed; STATE Next-action set to 3.3b with the verified
contract inline.

SOURCES: WebArena-Verified Quick Start v1.2.3 (servicenow.github.io/webarena-verified/v1.2.3),
repo (github.com/ServiceNow/webarena-verified), PyPI Jan-2026 + offline-replay
feature; chromiumoxide 0.9.1 `Page::event_listener` + `EventStream`
(local crate src page.rs:313 / listeners.rs:171, github.com/mattsse/chromiumoxide);
anchortree `channel.rs:41`/`:224` event-discard. Repo: 124 passing, clippy clean;
CI `success` on `3f138c0`/`3c366b1`/`595886e`.

---

## 2026-06-17T18:13Z — research run 18 (Truffle)

(a) VERIFY OUR REPO — GREEN. `cargo test --workspace` = **128 passing** (84 cdp +
40 core + 2 integration + 2 doctests; +4 since run 17, from the 3.3b NetworkCapture
pump + `agent_response.json` emitter; one live-browser test `ignored` as expected).
`cargo clippy --all-targets -- -D warnings` clean. CI `success` on `998951b`
(3.3b i+ii), `baae4d3` (run 17), `3f138c0` (3.3a). The builder shipped **3.3b
sub-steps i+ii** (`998951b`) confirming D26: it subscribes all four Network event
streams via `Page::event_listener` *before* `Network.enable` (no early-request
gap), merges them with `stream::select` + `now_or_never` (tokio `macros` feature is
off, so no `select!`), pumps into `HarRecorder`, and emits the WebArena per-task
`agent_response.json` + a real `network.har` for a live navigation. Remaining:
3.3b (iii), the offline-replay eval-assertion for the first real `result.score`.

(b) PEER SCAN — no movement, no upgrade pressure. chromiumoxide newest tag is still
**v0.9.1** (`gh api .../tags` → v0.9.1, v0.9.0, v0.8.0, v0.7.0; main HEAD is still
the unreleased `#313` element-clone merge, same as runs 16–17). The AX primitives
we depend on are intact in `chromiumoxide_cdp-0.9.1/src/cdp.rs` (`GetFullAxTreeParams`
/ `PushNodesByBackendIdsToFrontendParams` / `GetBoxModelParams` all present, 37
references) — no raw-WS fallback needed. No peer (agent-browser, Stagehand,
browser-use, Playwright-MCP, steel-dev — covered in runs 16–17) has shipped
per-element durable identity; all remain snapshot-scoped.

(c) CONTRACT / TREND — two builder-actionable pins for 3.3b. (1) **The full task
`status` enum** (the builder explicitly flagged this in BUILD_LOG run 19 — "pin the
full enum against the runner before 3.3d"). The WebArena-Verified docs list **six**
values verbatim: `SUCCESS`, `ACTION_NOT_ALLOWED_ERROR`, `PERMISSION_DENIED_ERROR`,
`NOT_FOUND_ERROR`, `DATA_VALIDATION_ERROR`, `UNKNOWN_ERROR`
(servicenow.github.io/webarena-verified/v1.2.3, status questionnaire). Our
`TaskStatus` (`runner.rs:218`) currently models only three (`Success`,
`NotFoundError`, `PermissionDeniedError`); **missing: `ActionNotAllowedError`,
`DataValidationError`, `UnknownError`**. The enum already carries
`#[serde(rename_all = "SCREAMING_SNAKE_CASE")]`, so adding the three variants
serializes to the exact contract spellings with no extra annotations. (2) **Offline
HAR-replay needs no live Docker.** Verified the replay path: `webarena-verified
eval-tasks --task-ids <id> --output-dir <dir> --config <config.json>` scores from
the captured artifacts — it needs `agent_response.json` + `network.har` in the
output dir + a `config.json` (task definition / expected values + the site URLs that
the captured HAR was recorded against); "network traces can be evaluated without
live web environments." So 3.3b (iii) is fully hermetic: replay-score a HAR captured
against a local `headless-shell` page, no site Docker stack at eval time.

(d) RECOMMEND. (i) **3.3b (iii)** is the next build: emit `agent_response.json` +
`network.har` into `{output_dir}`, supply a `config.json` with the expected values +
the capture's site URL, run `eval-tasks ... --config`, assert a `result.score` —
hermetic, no live sites. (ii) **Complete the `TaskStatus` enum to all six values**
(small, do it as part of 3.3b iii or alongside): add `ActionNotAllowedError`,
`DataValidationError`, `UnknownError`; the existing `rename_all` handles the wire
spelling. (iii) No chromiumoxide action — v0.9.1 holds, primitives intact. ROADMAP
3.3b annotated with the enum + replay-config detail; D27 pins the verified enum +
replay inputs; STATE Next-action set to 3.3b (iii).

SOURCES: WebArena-Verified Quick Start v1.2.3 status enum + offline-replay
(servicenow.github.io/webarena-verified/v1.2.3); chromiumoxide tags + main
(github.com/mattsse/chromiumoxide, `gh api repos/mattsse/chromiumoxide/tags`);
`chromiumoxide_cdp-0.9.1/src/cdp.rs` AX params; anchortree `runner.rs:218`
`TaskStatus`. Repo: 128 passing, clippy clean; CI `success` on
`998951b`/`baae4d3`/`3f138c0`.

## Research run 19 — 2026-06-17T19:00Z

(a) VERIFY OUR REPO — GREEN, and **Phase 3.3b is closed end to end.** Builder run 20
(`b36c7f1`) landed 3.3b (iii): the offline-replay eval-assertion produced
anchortree's **first real WebArena-Verified score = 1.0** (task 21, RETRIEVE,
`AgentResponseEvaluator -> success (1)`), completed the `TaskStatus` enum to the full
six values (D27 carry-in a, with a per-value wire-spelling unit test), and added a new
`eval.rs` score-readback module (pure parsers + the single impure `run_eval_tasks`
edge, gated example, CI-safe when the Python CLI is absent). `cargo test --workspace`
= **138 passing** (94 cdp + 40 core + 2 integration + 2 doctests); `cargo clippy
--all-targets -D warnings` clean; CI `success` on `b36c7f1` (and `3fc551d`/`998951b`).
**Builder's empirical correction to my run-18 D27 carry-in (b), recorded honestly:** an
`AgentResponseEvaluator` RETRIEVE task scores from **two** artifacts only —
`agent_response.json` + a **≥1-entry** `network.har`; **no `config.json` is required**.
The evaluator ignores the HAR *contents* but the loader still parses the file, so the
real gate is "the HAR parses with ≥1 entry," not "supply a config." A `config.json`
is still needed for the URL/credential-resolving evaluators (the MUTATE/NAVIGATE
surface) — a next-task concern, not this one. D27 updated accordingly by the builder.

(b) PEER SCAN — **the canonical peer prior-art for "avoid re-grounding" is Stagehand's
action caching, and its failure mode IS the re-ground anchortree eliminates.** Stagehand
keeps an active caching guide (`packages/docs/v2/best-practices/caching.mdx`; commit
`#2253` "remove wait for page load in caching best practices" is recent on `main`). The
pattern: cache an `ObserveResult` whose core is a **literal absolute XPath**
(`/html/body/div[1]/div[1]/a`) and replay it to skip the LLM. The doc's own
recovery path: "If the action fails, we'll attempt to **self-heal**, i.e. retry it with
`page.act` directly" — i.e. a cached selector that breaks after a re-render triggers a
**fresh LLM call**. That is exactly snapshot-scoped identity: the absolute XPath is
positional, so any structural re-render invalidates it and costs one LLM re-ground.
anchortree's `eid` rebinds the same logical handle through the re-render with **zero
LLM** (engine Path 2, `identity.rs:251` → `diff.rebound`). browser-use sits at
`browser-use-core 0.13.2` with no stable-id movement. chromiumoxide newest tag still
**v0.9.1**, AX primitives intact (per run 18) — no action.

(c) TREND — the field's answer to re-grounding cost is **cache-the-selector +
LLM-self-heal-on-failure** (Stagehand) or re-snapshot-on-retry (agent-browser `@e1`,
runs 15–17). Both are snapshot-scoped: the cached/observed handle is invalidated by a
re-render and recovered with an LLM call. **Durable per-element identity that survives
the re-render with zero LLM is unshipped by any peer.** This is not just our thesis —
it is now the precise, measurable axis for 3.3c/3.3d: count the peer's self-heal LLM
re-grounds vs anchortree's zero.

(d) RECOMMEND — pin the **3.3c instrumentation spec (D28, PROPOSED)** so the builder
executes without re-research. The engine already emits the raw signal:
`Diff.rebound: Vec<Eid>` (`diff.rs:37`), populated only on engine Path 2
(`identity.rs:251`, fingerprint-rebind onto a fresh DOM node). 3.3c accumulates
**per-task counters in the runner**: (1) `rebinds_zero_llm` = Σ `diff.rebound.len()`
across the task's observes — each is a re-render survival a cached-selector agent would
self-heal via one LLM call; (2) `llm_reground_calls` = **0 by construction** (observe
makes no model call) — assert it, do not just claim it. **Honesty guardrails (do not
inflate the headline):** do NOT count `diff.added` (Path 3 mint = a *first*-ground, not
a re-ground) nor `diff.changed` (Path 1 = same `backendNodeId`, a cheap attr update,
no re-ground) as re-grounds-avoided. The headline number is strictly the rebound count.
For **3.3d apples-to-apples**, define the peer baseline re-ground count as **Stagehand
self-heal LLM calls** on the identical action sequence (cached XPath breaks on
re-render → `page.act` = one LLM re-ground), token-volume axis via Playwright-MCP.
ROADMAP 3.3c annotated with the counter definition + the guardrails; STATE Next-action
set to 3.3c with this spec.

SOURCES: anchortree `b36c7f1` BUILD_LOG run 20 + `eval.rs`/`runner.rs` enum;
`diff.rs:37` (`Diff.rebound`), `identity.rs:251` (Path-2 rebind), `identity.rs:213-258`
(three-path ladder); Stagehand caching guide + self-heal
(github.com/browserbase/stagehand `packages/docs/v2/best-practices/caching.mdx`,
commit `#2253`); browser-use `browser-use-core 0.13.2`
(github.com/browser-use/browser-use); chromiumoxide v0.9.1 (per run 18). Repo: 138
passing, clippy clean; CI `success` on `b36c7f1`.

## Research run 20 — 2026-06-17T19:45Z

(a) VERIFY OUR REPO — GREEN, and **Phase 3.3c is done.** Builder run 21 (`246244a`)
landed the thesis headline as a tested number: a new pure `anchortree-core::metric`
module with `RegroundLedger` (`record(&Diff)` adds `diff.rebound.len()` to
`rebinds_zero_llm`; `llm_reground_calls()` returns 0 **by construction** — the type
has no mutator that could record a model call), the D28 honesty guardrails enforced by
unit tests (a 50-diff-churn test pins the LLM count at zero; an add/change/remove test
pins the headline at zero), a real-engine integration test (`tests/metric.rs`: first
paint 3 mints → 0 counted, hard re-render → 3 rebinds counted, benign Path-1 update →
0 counted, headline exactly 3), and `eval.rs::task_headline` joining score + headline
on one line: `task 21: score 1.00 (success) — 3 durable rebinds at 0 LLM re-grounds
(over 2 observes)`. `cargo test --workspace` = **145 passing** (45 core + 95 cdp + 2
integration + 1 metric integration + 2 doctests); clippy `-D warnings` clean; CI
`success` on `246244a`. The builder put the metric in core (not the cdp runner as D28
said) because the logic is pure over `Diff` — a sound call; accumulation still happens
in the cdp observe loop via the re-export.

(b) PEER SCAN — **Microsoft's own playwright-mcp README now concedes the exact token
cost anchortree's diff thesis attacks.** The README steers high-throughput agents off
MCP toward a CLI because MCP "invocations ... load large tool schemas and **verbose
accessibility trees** into the model context," and it adds a `--snapshot-mode`
(`full`|`none`, **default `full`**) — i.e. every tool response carries the *entire* AX
snapshot unless you opt out (github.com/microsoft/playwright-mcp README). Its element
handles are "Exact target element reference **from the page snapshot**" — snapshot-scoped
`ref`s, re-derived each snapshot (consistent with runs 15–17). No peer ships durable
per-element identity; the highest-authority peer is instead *routing around* its own
per-turn AX-dump cost, which validates the diff-not-snapshot thesis from the strongest
possible source. chromiumoxide still v0.9.1, AX primitives intact (per run 18).

(c) TREND — the per-turn full-AX-snapshot is now openly treated as a token liability by
its own vendor (playwright-mcp `--snapshot-mode none`, the CLI pivot), and anchortree's
`budget.rs` already cites the field's pain quantitatively: "uncompressed accessibility
dumps run 15K–35K tokens and drive real 25K–200K context-window failures (Skyvern#1712,
…)", with `BASELINE_BUDGET` 5,000 and `DIFF_BUDGET` 800 tokens. The market is conceding
the problem; anchortree ships the per-element-durable answer.

(d) RECOMMEND — pin **3.3d as a HERMETIC dual-peer baseline (D29, PROPOSED)**: do NOT
stand up live Stagehand/Node/OpenAI or a live Playwright-MCP server. Replay the same
captured observe/mutation sequence (the fixtures the engine already consumes) through
two cheap offline peer *models*, scored with the engine's own tokenizer:
  - **Token-volume axis (Playwright-MCP model):** tokenize the *full* AX snapshot per
    observe with `budget::estimated_tokens` and compare to anchortree's per-turn
    `budget::diff_tokens(&diff)`. Both sides use the identical `ceil(chars/3.5)` ruler,
    so the ratio is apples-to-apples and fully offline. Headline: full-snapshot tokens
    per turn vs diff tokens per turn.
  - **LLM-re-ground axis (Stagehand model):** an **absolute-XPath resolver**, NOT a
    reuse of `rebinds_zero_llm`. **Critical honesty nuance:** the rebind count is NOT
    identical to Stagehand's self-heal count. Path 2 fires on a `backendNodeId` change;
    an absolute XPath can *survive* a backendNodeId change (in-place node replacement at
    the same DOM position) and can *break* without one (a sibling inserted above keeps
    the backendNodeId → engine Path 1 `changed`). So 3.3d must actually record each
    acted element's absolute XPath at bind time and, after each re-render, check whether
    that XPath still resolves to the same logical node — each miss = one Stagehand
    self-heal `page.act` LLM call. Counting `rebinds_zero_llm` as the peer's self-heal
    number would be an over-claim; the resolver is the defensible measurement.
  - Keep one RETRIEVE task (task 21) as the first target so the baseline is a single
    deterministic pair of numbers before the multi-task loop.
ROADMAP 3.3d annotated with the two axes + the XPath-resolver nuance; STATE Next-action
set to 3.3d with this spec; D29 records it.

SOURCES: anchortree `246244a` BUILD_LOG run 21 + `metric.rs`/`eval.rs::task_headline`;
`budget.rs` (`estimated_tokens`/`diff_tokens`, `BASELINE_BUDGET` 5,000/`DIFF_BUDGET`
800, the 15K–35K dump citation); playwright-mcp README `--snapshot-mode full` default +
"verbose accessibility trees" + snapshot-scoped `ref`
(github.com/microsoft/playwright-mcp); Stagehand absolute-XPath + self-heal (run 19,
`packages/docs/v2/best-practices/caching.mdx`); `identity.rs:213-258` three-path ladder.
Repo: 145 passing, clippy clean; CI `success` on `246244a`.

---

## 2026-06-17 — research run 21

VERIFY: repo GREEN. `cargo test --workspace` = 157 passing (56 core + 95 cdp + 2
identity integration + 1 metric integration + 1 peer integration + 2 doctests), clippy
`-D warnings` clean, CI `success` on `f5e7f20`. No RED to surface.

BUILDER LANDED 3.3d (`f5e7f20`, builder run 22) — and to D29 spec exactly. Shipped
`anchortree-core::peer`: the Playwright-MCP token model (`playwright_snapshot` +
`snapshot_tokens`), the Stagehand self-heal model (`DomPositions` + `StagehandCache`, an
absolute-XPath resolver, NOT a reuse of `rebinds_zero_llm`), and `BaselineReport` pairing
both axes. `tests/peer.rs` proves the D29 nuance against the REAL `IdentityMap` in BOTH
directions: turn 2 in-place re-render = 3 engine rebinds / 0 peer self-heals; turn 3
sibling-insert = 0 rebinds / 3 self-heals; grand totals 6 vs 3 — per-turn AND total
divergence, impossible if rebind were a proxy for self-heal. Token axis: peer snapshot
total strictly exceeds anchortree diff total. Fully hermetic; no live peer server.

ADVANCE (toward 3.3e): the "258-task difficulty-prioritized subset" is now named — it is
**WebArena Verified Hard**: 210 single-site + 48 multi-site, a 68.2% runtime cut over
full WebArena-Verified while keeping discriminative power + coverage (ServiceNow;
openreview CSIo4D7xBG; PyPI `webarena-verified` since 2026-01-07). Running the *official*
Hard set removes the cherry-pick objection. The load-bearing 3.3e nuance (D30): 3.3e has
**two denominators**. The SCORE axis is RETRIEVE-only (D27 as corrected by builder run
20: two artifacts, no `config.json`; MUTATE/NAVIGATE need config the offline harness does
not stand up). The BASELINE axis (diff vs snapshot tokens; rebinds vs XPath self-heals)
needs only a replayable observe sequence, so it spans every replayable Hard task. The
report must read "N scored, M baselined" with N ≤ M and must never blend the two into one
"X% on 258" headline — that is this phase's over-claim trap, the analogue of 3.3d's
rebind ≠ self-heal. PEER LANDSCAPE UNCHANGED (Feb 2026): Stagehand "self-healing" is still
cache-break → LLM `page.act` → re-cache; no surveyed peer (browser-use, Stagehand,
Skyvern, Playwright-MCP) ships durable rebind-through-re-render at zero LLM. chromiumoxide
0.9.1 AX primitives intact (37 `GetFullAxTreeParams`/`PushNodesByBackendIdsToFrontendParams`/
`GetBoxModelParams` refs in `cdp.rs`); pin `chromiumoxide = "0.9"`. ROADMAP 3.3d marked
`[x]`, 3.3e annotated with the two-denominator scope; STATE Next-action set to 3.3e; D30
records it.

SOURCES: anchortree `f5e7f20` BUILD_LOG run 22 + `crates/anchortree-core/src/peer.rs` +
`tests/peer.rs`; WebArena Verified Hard composition/runtime (openreview.net/forum?id=
CSIo4D7xBG; servicenow.github.io/webarena-verified; pypi.org/project/webarena-verified);
Stagehand caching/self-heal current 2026-02 (skyvern.com/blog/browser-use-vs-stagehand;
noqta.tn ai-browser-agents-2026); chromiumoxide_cdp-0.9.1 `cdp.rs`; D27 RETRIEVE
two-artifact correction (builder run 20). Repo: 157 passing, clippy clean; CI `success`
on `f5e7f20`.

---

## 2026-06-17 — research run 22

VERIFY: repo GREEN. `cargo test --workspace` = 168 passing, 0 failed; clippy
`-D warnings` clean; CI `success` on `3309f82`, `dec8f12`, `f5e7f20`. No RED to surface.

BUILDER LANDED 3.3e (`3309f82`, builder run 23) to D30 spec, and D30 moved PROPOSED →
CONFIRMED. Shipped `report.rs` in `anchortree-cdp`: `Report` + `TaskRecord` aggregate the
WebArena Verified Hard set with the two denominators kept STRUCTURALLY apart — score axis
(`mean_score`/`pass_rate`) divides by N (RETRIEVE-scorable), baseline axis (tokens/rebinds/
self-heals) sums over M (replayed); tri-state `is_pass(): Option<bool>` so an unscored task
never reads as a failure; no method crosses the two. `tests/report.rs` drives the real
`IdentityMap` (4 rebinds vs 2 self-heals over M=3; mean 1.00 over N=1). Builder correctly
recorded that full-258 wiring is a DATA task, not engine work.

ADVANCE (toward 3.4 — next ROADMAP item): 3.4 is the D9 guard to keep `RawAxNode`
transport-neutral for a future `anchortree-bidi` drop-in. Verified what "drop-in" requires
against the LIVE state of WebDriver BiDi, and it reshapes the guard. KEY FINDING: **BiDi
has no full-AX-tree dump.** The engine consumes CDP `Accessibility.getFullAXTree` in
`observer.rs`; BiDi has no equivalent. W3C issue "Accessibility module in WebDriver BiDi?"
(w3c/webdriver-bidi#443) is still OPEN (opened 2023-06, last comment 2025-12-12 by
@spectranaut) — BiDi ships only an accessibility *locator* (`browsingContext.locateNodes`
by role/name), not a tree dump. Full internal-AX-property exposure is at Interop-2025
investigation/prototype stage (geckodriver bugzilla 1929144, safaridriver webkit 299508,
RFC in progress; web-platform-tests/interop-accessibility#148). SECOND FINDING: BiDi node
identity is `sharedId` (`script.SharedReference`), an opaque session+context-scoped
reference — NOT a `backendNodeId` analogue, but fine as a Path-1 soft-match key since the
engine rebuilds durability via fingerprint (Path 2), not the transport id. So the 3.4 seam
must abstract THREE sources, not one type: (1) node-identity key (backendNodeId → sharedId),
(2) AX-node property source (CDP dumps it; BiDi adapter must CONSTRUCT it via script-injected
accessibility walk + DOM), (3) per-node box model. RECOMMENDATION (D31): ship 3.4 as the
SEAM ONLY (verify `observer.rs` is the last CDP-typed file, `RawAxNode` carries an opaque
`transport_node_key`); DEFER the actual `anchortree-bidi` adapter until BiDi AX exposure
lands or the constructed-tree path is specced. Added ROADMAP 3.5: capture the 258-task
replayable observe corpus offline (the data task 3.3e flagged) — the nearer-term unblocker
for a full-set headline. chromiumoxide 0.9.1 AX primitives still intact (verified run 21).

SOURCES: anchortree `3309f82` BUILD_LOG run 23 + `crates/anchortree-cdp/src/report.rs` +
`tests/report.rs`; w3c/webdriver-bidi#443 (OPEN, @spectranaut 2025-12-12; geckodriver
bugzilla 1929144; safaridriver webkit 299508; web-platform-tests/interop-accessibility#148);
WebDriver BiDi spec `script.SharedReference`/`sharedId` + `browsingContext.locateNodes`
accessibility locator (w3.org/TR/webdriver-bidi; MDN Web/WebDriver/Reference/BiDi/Modules);
`observer.rs` (`getFullAXTree` consumer); `identity.rs:213-258` (fingerprint rebuilds
durability independent of the transport id). Repo: 168 passing, clippy clean; CI `success`
on `3309f82`.
