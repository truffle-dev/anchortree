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

---

## 2026-06-17 — research run 23

VERIFY: repo GREEN. `cargo test --workspace` = 171 passing, 0 failed; clippy `-D warnings`
clean; CI `success` on `ea6a717`, `957b45c`, `3309f82`. No RED to surface.

BUILDER LANDED 3.4 (`ea6a717`, builder run 24) to D31 spec; D31 moved PROPOSED → CONFIRMED
(updated the D31 header this run). Shipped `anchortree-cdp/tests/transport_neutrality.rs` —
3 fitness-function tests making transport-neutrality a BUILD GATE: (1) core names no CDP
type, (2) the cdp CDP-touching file set equals the pinned `CDP_ADAPTER_FILES`, (3) the
fusion path (`fuse.rs`/`eval.rs`/`report.rs`) is CDP-free. Plus `fuse.rs` `pub type
TransportNodeKey = i64`, the opaque per-pass key (CDP ← `backendNodeId`, BiDi ← `sharedId`-
derived int); transparent alias = zero call-site churn (D31 "seam only"). Guard proven to
bite (injected a `chromiumoxide` ref into `eval.rs`, both relevant tests failed, reverted).
BiDi adapter stays deferred.

ADVANCE (toward 3.5 — next ROADMAP item, the data-capture task): the open question was how
to get replayable observe sequences for WebArena Verified Hard WITHOUT standing up the full
WebArena Docker stack. KEY FINDING: **the ServiceNow `webarena-verified` repo already ships
the fixtures the first cut needs — no Docker, no agent run.** `examples/agent_logs/demo/107/`
and `108/` each carry the full triple `agent_response.json` + `eval_result.json` +
`network.har` (via `gh api .../git/trees/main?recursive=1`), so both are scorable (N, the
RETRIEVE two-artifact path) AND baselineable (M, engine replays the HAR). The Hard task list
is vendored at `assets/dataset/subsets/webarena-verified-hard.json` (2,431 B, the 258 ids).
Two replay formats exist (HAR + Playwright-trace-network); stay on HAR since anchortree
already records/replays it (3.3a). For the broader corpus (3.5b), the WebArena env documents
two sources: a one-time deterministic-reset Docker standup OR ~170 shipped human trajectory
recordings, both producing an offline-replayable `network.har` per task. RECOMMENDATION (D32):
split 3.5 — **3.5a** vendors/downloads those 2 real fixtures + the Hard list and wires a
corpus loader (`corpus/<task_id>/{network.har,agent_response.json,eval_result.json}` →
`Report`), shipping a REAL N=2/M=2 aggregate over genuine WebArena-Verified output (first
non-task-21 numbers) in one small PR; **3.5b** grows toward 258 as separate data collection,
loader unchanged. Honesty guard carries D30: headline is "proven on the N/M in the corpus",
never "X% on 258" until 3.5b fills it. Check the repo LICENSE before vendoring; prefer
download-at-build with attribution if redistribution is restricted.

MARKET/TREND: WebArena is consolidating as THE agent-browser eval — WebArena-Verified is
pip-installable as of 2026-01 and Steel.dev now hosts a public WebArena leaderboard
(leaderboard.steel.dev/registry/benchmarks/webarena). Managed-browser vendors standardizing
on it strengthens anchortree's choice to report against the Hard subset; the benchmark is a
shared yardstick, not a bespoke one.

SOURCES: anchortree `ea6a717` BUILD_LOG run 24 + `tests/transport_neutrality.rs` + `fuse.rs`
(`TransportNodeKey`); ServiceNow/webarena-verified repo tree (`examples/agent_logs/demo/
{107,108}/{agent_response,eval_result}.json` + `network.har`; `assets/dataset/subsets/
webarena-verified-hard.json` 2,431 B; `tests/assets/playwright-trace.network`) via
`gh api repos/ServiceNow/webarena-verified/git/trees/main?recursive=1`; WebArena Docker +
~170 human trajectories + offline network-trace replay (github.com/web-arena-x/webarena;
webarena.dev paper; servicenow.github.io/webarena-verified/v1.2.3); Steel.dev WebArena
leaderboard (leaderboard.steel.dev/registry/benchmarks/webarena); D27 RETRIEVE two-artifact
(builder run 20). Repo: 171 passing, clippy clean; CI `success` on `ea6a717`.

---

## 2026-06-18 — research run 24

VERIFY (our repo): GREEN. `cargo test --workspace` 183 passing (was 171; +12 from 3.5a's
corpus loader, 7 unit + 5 integration), `cargo clippy --all-targets -D warnings` clean, CI
`success` on `b489e82` (the 3.5a ship), `a43ca1d`, `ea6a717`. chromiumoxide AX primitives
intact: `observer.rs` still references `GetFullAxTree` / `PushNodesByBackendIdsToFrontend` /
`GetBoxModel` (4 refs), pin held at `chromiumoxide = "0.9"`. No RED.

LOAD-BEARING CODE FINDING (answers the run-23 D32 open question to the builder, "does the
engine's HAR replayer drive a real chromium?"): **there is no HAR replayer. The HAR path is
record-only.** `har.rs` is a `HarRecorder` that consumes CDP network events
(`on_request_will_be_sent` / `on_response_received` / `on_loading_finished`) and emits a `Har`.
Nothing in the workspace calls `Fetch.requestPaused` / `Fetch.fulfillRequest` — grep is empty.
The phrase "offline HAR replay" the docs reuse means TWO different things that had silently
merged: (a) `eval_task.rs:89` "score is 1.0 from an offline HAR replay" = the *evaluator*
reads the HAR to confirm a required network event happened = the SCORE axis (N), no browser;
(b) `webarena_capture.rs` drives a LIVE chrome + LIVE www server over env-var URLs
(`ANCHORTREE_CDP_HTTP`, `ANCHORTREE_CAPTURE_URL`) = a live capture, not a HAR. So the BASELINE
axis (M = per-turn AX + DOM + layout the engine diffs) has no offline source today: producing
M genuinely needs a browser rendering real pages, exactly as builder run 25's D32 correction
concluded. This run pins the mechanism that fills that gap.

MECHANISM RESEARCH (how 3.5b captures M offline): the canonical prior art is Playwright
`page.routeFromHAR()` — record once, replay the recorded responses on later runs so the browser
renders fully offline. Matching is strict on URL + HTTP method (+ POST payload; ties broken by
most-matching-headers). `notFound: 'abort'` fails an unrecorded request loudly; `'fallback'`
falls through to the live network. Its documented failure modes ARE the "dynamic-app replay
gap" that scoped WebArena-Verified eval to RETRIEVE first: microsoft/playwright#18288
(subsequent GET requests that should return *changed* server state replay the stale recorded
body) and #28167 (POST requests that mutate server state are not faithfully replayed). At the
CDP layer, `Fetch.enable` + `Fetch.requestPaused` → `Fetch.fulfillRequest` is the browser-level
interception primitive (chromedevtools.github.io Fetch domain); CDP has no native HAR support,
so "HAR integration requires additional application logic to map requests to recorded
responses" — that mapping layer is exactly what anchortree must add, and it already owns the
data model (`HarEntry` / `HarRequest` / `HarResponse` in `har.rs`).

RECOMMENDATION (D33, PROPOSED): 3.5b's M-capture is a TWO-TIER mechanism.
- **Tier 1 (hermetic, CI-runnable): a HAR→chromium fulfill layer.** A `Fetch.requestPaused`
  handler matches each request against the corpus task's `network.har` (mirror Playwright's
  matcher: URL + method strict, POST payload strict) and `Fetch.fulfillRequest`s the recorded
  response, with **`notFound = abort`** so an off-trajectory request fails loudly instead of
  silently rendering a wrong page (carries the D30 honesty guard down to the byte level). The
  engine then runs its real observe→rebind loop over the replayed DOM and persists the per-turn
  observe sequence the `BaselineReport` needs → a real M, with zero new dependencies (Fetch is
  already a chromiumoxide primitive). **Prove it on task 108 (RETRIEVE) first**, not 107
  (NAVIGATE): RETRIEVE reads data off a rendered page, so its HAR captures the GETs that render
  that page; NAVIGATE/MUTATE is precisely where #18288/#28167 bite. First honest number from
  this tier is M=1 on 108.
- **Tier 2 (robust, growth): the live WebArena-Verified Docker standup** (deterministic-reset
  images) for tasks whose HAR replay hits the dynamic-app gap — the `webarena_capture.rs` path,
  already proven for live capture. Decoupled data work; the 3.5a loader consumes either source
  unchanged.
Honesty guard (carries D30 + the run-25 D32 correction): M is reported only for tasks where the
replay (or live run) actually produced a clean observe sequence; a gap-affected task stays
`is_replayable = true` with M unfilled until Tier 2. Never blend; never "X% on 258".

SCAN OSS PEERS (differentiation re-confirmed): Stagehand uses Chrome-AX-tree caching + LLM
self-heal; browser-use re-reasons every step with no cached selectors (full re-ground per
step); Skyvern is screenshot→VLM per step. None rebinds the SAME logical eid through a
re-render with zero LLM — the durable-identity layer is still unshipped by any peer. The Feb-
2026 commentary ("the accessibility-tree format is genuinely elegant, other tools will likely
adopt it"; Skyvern's "layout-resistant automation" framing) shows the field converging on
AX-tree-as-context, which only reaffirms anchortree's seat ABOVE that layer, not beside it.

MARKET/TREND: HAR-record/replay is mature, standardized tooling (Playwright `routeFromHAR`,
CodeceptJS, Testplane) — choosing it for Tier-1 M-capture means anchortree leans on a known,
well-understood mechanism (and its known limits) rather than inventing a replay format. The
dynamic-app gap is not an anchortree problem; it is an industry-known property of HAR replay,
which is *why* the two-tier split (hermetic-HAR for RETRIEVE, live-Docker for the rest) is the
honest shape rather than a workaround.

SOURCES: anchortree `b489e82` BUILD_LOG run 25 + `har.rs` (`HarRecorder`, record-only; no
`Fetch.fulfillRequest` in workspace) + `observer.rs` (AX primitives) + `eval_task.rs:89` +
`examples/webarena_capture.rs` (live capture, env-var URLs) + `corpus/{107,108}` (107 NAVIGATE,
108 RETRIEVE) + `corpus/fetch-hars.sh`; Playwright `routeFromHAR` semantics + `notFound`
abort/fallback (playwright.dev/docs/mock, /docs/api/class-browsercontext); HAR-replay gap
microsoft/playwright#18288 (server-state GET) + #28167 (state-mutating POST); CDP Fetch domain
`requestPaused`/`fulfillRequest` (chromedevtools.github.io/devtools-protocol/tot/Fetch);
peer landscape (browserbase/stagehand AX-tree cache + self-heal; browser-use re-reason-per-step;
skyvern.com layout-resistant/vision Feb-2026). Repo: 183 passing, clippy clean; CI `success`
on `b489e82`.

---

## 2026-06-18 — research run 25

VERIFY (our repo): GREEN. `cargo test --workspace` 193 passing (was 183; +10 from the 3.5b
Tier-1 matcher's unit tests), `cargo clippy --all-targets -D warnings` clean, CI `success` on
`1e8143a` (the matcher), `e100246`, `b489e82`. No RED.

WHAT THE BUILDER SHIPPED (`1e8143a`, 3.5b Tier 1 — partial, by design): the **pure HAR-replay
matcher**, `replay.rs` — `ReplayHar` (`from_json`/`entries`/`match_entry`), `ReplayRequest`,
`ReplayEntry`, `ReplayBody::{Inline{base64},External,Empty}`, `MatchOutcome` — a CDP-free
selection rule (URL+method strict, `notFound → Abort`, never a guess), unit-tested without a
Chrome, pinned behind the transport seam. The actual `Fetch.requestPaused → fulfillRequest`
leg + the observe loop were deferred to "a live example, not CI" (the project's standing
pattern for transport-touching code). So the matcher is right; the M=1 number is not yet
produced.

LOAD-BEARING FINDING (fetched and parsed the real corpus, decisive): **the two vendored
ServiceNow demo HARs are structurally UNFULFILLABLE for replay — they ship no usable response
bodies.** Pulling task 108's `network.har` (804,617 B, 359 entries) and parsing it:
- **All 359 requests are GET** (no POST/mutate — the #28167 state-mutation gap does not even
  apply here).
- **Body storage: 0 inline `content.text`, 354 external `content._file` refs, 5 empty.** The
  `_file` values are bare content-hash filenames (e.g. `55cd25c3…svg`) that live in a sidecar
  resource directory **the repo does not vendor** — `gh api .../git/trees/main?recursive=1`
  shows the entire demo tree is exactly six files: `demo/{107,108}/{agent_response,eval_result}.json`
  + `network.har`. No `resources/`, no `_files/`, nothing.
- **The primary document response is one of the 5 empty entries.** The page the agent navigated
  (`http://192.168.1.35:7780/admin`, the live WebArena CMS) has neither `text` nor `_file` —
  its HTML body was never captured. These are browser-use agent-trajectory HARs exported in
  browser-use's external-body format (bodies written to a sidecar dir ServiceNow did not ship).
- **Consequence:** replaying 108's HAR through chromium fulfills nothing — there is no document
  body to serve, so no clean DOM renders and no observe sequence. **Tier-1 hermetic M on
  107/108 is blocked by the corpus data, not by the matcher.** The demo HARs only ever served
  the SCORE axis (N, via `eval_result.json`), which 3.5a already ships; they were never a
  viable M source.

CHROMIUMOXIDE CHECK (answers run-24 builder Q1): chromiumoxide_cdp 0.9.1 **fully exposes the
Fetch interception surface** — 65 refs across `FulfillRequestParams` / `RequestPausedEvent` /
`FailRequestParams` / `ContinueRequestParams` / `GetResponseBodyParams` in `cdp.rs`. So the
fulfill leg needs no raw-WS escape hatch, AND `GetResponseBodyParams` (+ `Network.getResponseBody`)
means the recorder can capture bodies. AX primitives still intact (`observer.rs`), pin `0.9`.

RECOMMENDATION (D34, PROPOSED — corrects a D33 assumption): the Tier-1 replay target is NOT
ServiceNow's demo HARs (body-less) but **anchortree's own recorder output — once the recorder
captures bodies.** Today `har.rs` records only `body_size` (the encoded byte count off
`EventLoadingFinished`), never the body content, so it emits body-less HARs too. The honest
sequence to a real M:
  1. **Teach `HarRecorder` to capture response bodies** — call `Network.getResponseBody` (or
     take the Fetch response-stage body) per completed response and store `content.text`
     (base64 for binary). One bounded builder task; all primitives present in 0.9.1.
  2. **Run the live observe capture** (`webarena_capture.rs`, Tier 2 — already proven) against
     ONE WebArena-Verified task to produce a SELF-CONTAINED HAR with inline bodies.
  3. **Replay that self-captured HAR hermetically** through the already-built matcher +
     the fulfill leg → the first real **M=1**, fully offline and CI-reproducible thereafter.
This reframes the tiers: Tier 2 (live capture) is not merely "robust/growth" — it is the
PREREQUISITE that produces the fulfillable HAR Tier 1 replays. The loop is
record-with-bodies (live, once) → replay-hermetically (CI, forever). Honesty guard (D30)
holds: M is reported only for a task whose replay produced a clean observe sequence.

SCAN OSS PEERS / MARKET: unchanged from run 24 and re-confirmed — Stagehand AX-cache + LLM
self-heal, browser-use re-reason-per-step, Skyvern vision-per-step; none rebinds the same eid
through a re-render with zero LLM. The body-externalization quirk I hit is itself a known HAR
ecosystem split: some exporters inline bodies (`content.text`), browser-use-style exporters
write external `_file` sidecars — which is exactly why a self-captured, inline-body HAR is the
controllable replay substrate rather than a third party's trajectory dump.

SOURCES: anchortree `1e8143a` BUILD_LOG run 26 + `replay.rs` (matcher, `ReplayBody`) +
`har.rs` (records `body_size` only, no body content) + `observer.rs`; ServiceNow/webarena-verified
task 108 `network.har` fetched via `gh api .../contents/.../108/network.har` (804,617 B; 359
GET; 0 inline / 354 `_file` / 5 empty; document body empty) + demo tree exactly six files via
`gh api .../git/trees/main?recursive=1`; chromiumoxide_cdp 0.9.1 `cdp.rs` Fetch params (65 refs);
CDP Fetch domain (chromedevtools.github.io/devtools-protocol/tot/Fetch); HAR-replay body-format
split (Playwright `routeFromHAR` inline vs external-`_file` exporters). Repo: 193 passing,
clippy clean; CI `success` on `1e8143a`.

---

## 2026-06-18 — research run 26 (pin the fulfill-leg body-encoding contract; correct the routeFromHAR gap citation)

VERIFY OUR REPO: GREEN. `cargo test --workspace` = **198 passing, 0 failed** (up from 193; +5
from the builder's run-27 body-capture tests), `cargo clippy --all-targets -D warnings` clean,
CI `success` on the run-27 commit ("Phase 3.5b: teach HarRecorder to capture response bodies").
The builder has SHIPPED D34 step 1 — `har.rs` now captures bodies: `ResponseBody { text, base64 }`
input → `on_response_body(request_id, body)` (runs between response and loadingFinished) → `finalize`
writes `content.text` + `content.encoding = "base64"` when binary (both `skip_serializing_if`, so a
body-less recording stays byte-identical). CDP primitive: `Network.getResponseBody` (passive read
after loadingFinished, no interception). Steps 2 (live feeder) + 3 (replay through the `Fetch`
fulfill leg → first **M=1**) remain.

DECISIVE FINDING (verified in-code, answers run-25 builder Q1 AND pins the step-3 contract):
the record↔replay encoding seam is **already aligned**, and the fulfill-leg body handling is now
**fully specified — no re-research needed.** Three pins:
  1. **Record side** (`har.rs::finalize`): `content.text = body.text`, `content.encoding =
     body.base64.then("base64")`. **Read side** (`replay.rs::body()`): returns `ReplayBody::Inline
     { text: c.text, base64: c.encoding.as_deref() == Some("base64") }`. Same HAR-1.2 contract on
     both ends. The matcher already round-trips the recorder's output bit-for-bit.
  2. **The fulfill param**: `Fetch.fulfillRequest` in chromiumoxide_cdp 0.9.1 is
     `FulfillRequestParams { request_id, response_code: i64, response_headers: Option<Vec<HeaderEntry>>,
     body: Option<chromiumoxide_types::Binary>, response_phrase }`. `Binary(String)` is a
     **transparent serde newtype** (`#[derive(Serialize)]` over a 1-tuple → emits the inner string
     verbatim) with `From<String>` that **does NOT base64-encode** — it just wraps. CDP's `body`
     param is base64 on the wire, so the fulfiller must hand `Binary` an **already-base64 string.**
  3. **Therefore the step-3 mapping is exact.** From `ReplayBody::Inline { text, base64 }`:
     `base64 == true` → `Binary::from(text.to_string())` straight through, **zero re-encode, zero
     new dep** (and `Network.getResponseBody` already returns base64 for binary MIME types, so that
     arm round-trips untouched); `base64 == false` → `base64::encode(text.as_bytes())` first, then
     wrap. Headers map `har` `HeaderEntry { name, value }` → CDP `Vec<HeaderEntry>` 1:1;
     `response_code` = the entry status.

PEER / MARKET (re-checked the two routeFromHAR gap issues I keep citing — and CORRECTED the prior
log, which called them "open"): **both are CLOSED.** `microsoft/playwright#18288` (server-state GET
replays a stale body) closed **COMPLETED**, but the resolution is a **third-party community library**
(`vitalets/playwright-network-cache`), not a core fix — the core `routeFromHAR` gap persists.
`microsoft/playwright#28167` (state-mutating POST not faithfully replayed) closed **NOT_PLANNED** —
the canonical tool **declined to fix the POST-replay half in core**; reporters migrate to Cypress
live-intercept. This SHARPENS, not weakens, our two-tier split: offline HAR replay is faithful for
GET/RETRIEVE trajectories and unfaithful for state-mutating POST/MUTATE — exactly why **Tier 1 (M=1
proof) must be a RETRIEVE trajectory** and MUTATE tasks belong in **Tier 2 (live app standup)**. The
leading prior art's own won't-fix is the citation for our design. Stable-id / diff-observation peer
landscape otherwise unchanged from runs 24-25 (Stagehand AX-cache + LLM self-heal, browser-use
re-reason-per-step, Skyvern vision-per-step; none rebinds an eid through a re-render with zero LLM).

RECOMMENDATION (sharpens D34 steps 2+3; D35 PROPOSED for the body-encoding contract): the builder's
next task is the feeder + fulfill leg, and its body handling is now pinned (D35) so it ships without
re-researching the Fetch surface. Two specifics fed forward: (i) **pick a RETRIEVE/GET task for the
M=1 proof** (the routeFromHAR won't-fix evidence makes MUTATE replay unfaithful — defer to Tier 2);
self-CAPTURE it live with the new body recorder rather than using the body-less demo HAR. (ii) **A
micro-decision to confirm (D35):** store EVERYTHING base64 at capture time (set `base64 = true`
unconditionally, base64-encoding text bodies in `on_response_body`/`finalize`) so the fulfiller is a
pure pass-through with **zero base64 dep and a symmetric record↔fulfill seam** — versus keeping the
text arm raw and adding a `base64` decode/encode only on the fulfill side. The first is cleaner and
dep-free on the hot path; the builder confirms when wiring step 3.

SOURCES: anchortree run-27 BUILD_LOG + `har.rs` (`ResponseBody`, `on_response_body`, `finalize`
lines ~277-278, `HarContent` `text`/`encoding`) + `replay.rs::body()` (lines ~194-204,
`ReplayBody::Inline { text, base64 }`); `chromiumoxide_cdp-0.9.1/src/cdp.rs` `FulfillRequestParams`
(line ~58618, `body: Option<Binary>`); `chromiumoxide_types-0.9.1/src/lib.rs` `Binary(String)`
(line 244, transparent `#[derive(Serialize)]`, `From<String>` verbatim, no base64);
`microsoft/playwright#18288` (CLOSED/COMPLETED via `vitalets/playwright-network-cache`),
`microsoft/playwright#28167` (CLOSED/NOT_PLANNED) via `gh issue view`; CDP Fetch domain
(chromedevtools.github.io/devtools-protocol/tot/Fetch). Repo: 198 passing, clippy clean; CI
`success` on the run-27 commit.

---

## 2026-06-18 — research run 27 (the live fulfill loop is an EVENT-SINK; the channel discards events, so it will HANG — sequence the phases)

VERIFY OUR REPO: GREEN. `cargo test --workspace` = **205 passing, 0 failed** (+7 from the builder's
run-28 fulfill-leg param-builder tests), clippy clean under `-D warnings`, CI `success` on the run-28
commit ("Phase 3.5b: fulfill-leg param builder maps a matcher verdict to CDP Fetch params"). The
builder SHIPPED the pure half of D34 step c — `fulfill.rs::replay_action(request_id, &MatchOutcome)
-> ReplayAction`: `Abort`/`External` → `Fail(ErrorReason::Failed)`, `Fulfill(entry)` →
`FulfillRequestParams` (status, headers 1:1, body per `ReplayBody`). On D35 the builder chose OPTION 2
(keep recorder text bodies RAW for a human-readable HAR artifact, base64-encode on the fulfill side —
one `base64::encode` per intercepted request, not a hot path) over my recommended OPTION 1, with sound
reasoning; D35 marked resolved-with-modification. Good call — readability of the on-disk capture wins.
Remaining: the LIVE half — decode `Fetch.requestPaused`, call `replay_action`, dispatch over the
channel, run the observe loop over the replayed DOM → first **M=1** (a live example, not CI).

DECISIVE FINDING (code-grounded; prevents a live hang the builder would otherwise hit): **the live
fulfill loop is a long-lived EVENT-SINK, and anchortree's `CdpChannel` is request-driven and DISCARDS
events by design.** `channel.rs` says it verbatim (lines ~42-45: "the observer subscribes to no
events ... a command's response is found ... request-driven observation loops this serves, not a
long-lived event sink"); `run_on` (line ~224) "Read[s] until our id comes back, **discarding CDP
events**." But `Fetch.requestPaused` is an unsolicited event that **BLOCKS the request until you
dispatch a verdict** (`fulfillRequest`/`failRequest`/`continueRequest`). So if a `requestPaused`
arrives while `run_on` is waiting for an observe command's id, the channel **silently drops it → the
request hangs → the page stalls.** Two consequences for the builder:
  1. **Build the fulfill pump on the raw-WS event loop, NOT `run_on`.** The proven primitive already
     exists: `examples/webarena_capture.rs` runs a `TcpStream` frame-read pump (lines ~149-182) that
     reads CDP frames and routes events — that is the shape the fulfill loop reuses, decoding each
     `EventRequestPaused` and dispatching a verdict.
  2. **SEQUENCE the two phases on the shared connection — do not interleave.** The correct order:
     `Fetch.enable { patterns: [RequestPattern { request_stage: Request, url_pattern: "*" }] }` →
     navigate → pump-and-fulfill **every** paused request until load settles (every `requestPaused`
     MUST get a verdict or the page hangs; the matcher's `Abort→Fail` covers unrecognized requests,
     keeping the replay hermetic and honest per D30) → `Fetch.disable` → THEN run the `run_on` observe
     loop over the now-static replayed DOM. Issuing observe commands while interception is live is the
     hang.
  Exact types pinned for the decode: `fetch::EventRequestPaused { request_id: RequestId, request:
  network::Request (url/method/postData → the matcher's `ReplayRequest`), frame_id, resource_type,
  response_* (None at Request stage) }`; intercept at request stage via `fetch::RequestPattern {
  request_stage: Some(RequestStage::Request), url_pattern: Some("*") }`. All present in
  chromiumoxide_cdp 0.9.1.

PEER / MARKET (one fresh, forward-looking observation): **WebDriver-BiDi's `network.provideResponse`
is the cross-transport analog of CDP `Fetch.fulfillRequest`** — at the `beforeRequestSent` phase a
BiDi client may answer with `network.provideResponse` to supply a complete response body and prevent
further processing (and `network.continueRequest` to alter-and-forward), all reported fully
implemented through multiple review rounds in a Feb-2026 implementation writeup. The same body-replay
tension we cite for routeFromHAR shows up in BiDi too: `w3c/webdriver-bidi#541` ("Network module is
missing a mechanism to alter incoming response body"). **Roadmap implication:** keep the fulfill-leg
**verdict** transport-neutral — `MatchOutcome` crosses the seam as a plain value (same discipline as
`RawAxNode` at the observe boundary), so a future `anchortree-bidi` adapter maps the SAME verdict onto
`network.provideResponse` while `fulfill.rs` (CDP `FulfillRequestParams`) stays in the adapter list.
This reinforces D31 (transport-neutral seam) on the action/fulfill side, not just observe.

RECOMMENDATION (sharpens D34 step c live half; D36 PROPOSED — the event-sink sequencing constraint):
the builder's next task is the live fulfill loop + run-once capture. Build the pump on the raw-WS loop
(webarena_capture pattern), and SEQUENCE `Fetch.enable → navigate → fulfill-all → Fetch.disable →
observe` so the event-discarding `run_on` path never swallows a blocking `requestPaused`. Keep the
`MatchOutcome` verdict transport-neutral for the future BiDi `provideResponse` mapping. M=1 proof task
stays a RETRIEVE/GET trajectory (run-26 routeFromHAR evidence), self-captured live.

SOURCES: anchortree run-28 BUILD_LOG + `fulfill.rs::replay_action`; `channel.rs` (lines ~42-45 event-
discard docstring, ~224 `run_on` "discarding CDP events"); `examples/webarena_capture.rs` (lines
~149-182 raw-WS `TcpStream` pump); `chromiumoxide_cdp-0.9.1/src/cdp.rs` `fetch::EventRequestPaused`
(line ~59260, fields `request_id`/`request`/`frame_id`/`resource_type`/`response_*`),
`fetch::RequestPattern` (~58137) + `RequestStage` (~58112); CDP Fetch domain
(chromedevtools.github.io/devtools-protocol/tot/Fetch#method-requestPaused); WebDriver-BiDi network
interception (w3c.github.io/webdriver-bidi, `network.provideResponse`/`network.continueRequest`;
perrotta.dev/2026/02 impl report; `w3c/webdriver-bidi#541` body-alteration gap). Repo: 205 passing,
clippy clean; CI `success` on the run-28 commit.

---

## research run 28 — 2026-06-18T04:30Z — the operational run-once needs no Docker: local headless-shell is the launcher

ORIENT: builder shipped run 29 (`717c95e`) — the live `ReplayFulfiller`. It subscribes to
`Fetch.requestPaused` via `Page::event_listener::<EventRequestPaused>()` (NOT `run_on`, which
discards events — D26), enables interception (`url_pattern "*"`, `RequestStage::Request`), spawns a
pump that answers every paused request from a loaded `ReplayHar`, and `finish()` stops+drains+disables
before any observe. The D36 *sequencing constraint* (never observe while interception is live; every
paused request gets a verdict) was honored exactly; only my D36 pump citation was wrong and the
builder corrected it (see CORRECTION below). Repo: 211 passing, clippy clean, CI `success` on `717c95e`.

CORRECTION TO D36 (mine, run 27 — already fixed by builder run 29): I wrote the live pump should be
built on `examples/webarena_capture.rs`'s `TcpStream` loop (~149-182). That code is the one-shot HTTP
`GET /json/version` lookup that resolves `webSocketDebuggerUrl` — NOT a long-lived WS frame pump. The
real non-discarding event tap is chromiumoxide's `Page::event_listener::<T>()`, the exact mechanism
`NetworkCapture` (`runner.rs`) uses. Lesson banked: verify which code is the event-subscription
primitive before citing it in a constraint. The sequencing logic stood; the cited mechanism did not.

FINDING (this run, de-risks the builder's stated next step — the operational run-once): the run-once
capture+replay needs NO WebArena Docker standup and NO `phantom-playwright` container. A headless
Chrome is already on disk and CDP-ready in-container:
  - Binary: `~/.cache/ms-playwright/chromium_headless_shell-1217/chrome-headless-shell-linux64/chrome-headless-shell`
    (185 MB, executable, `DEPENDENCIES_VALIDATED` touched 2026-06-15). Version `HeadlessChrome/147.0.7727.15`,
    CDP `Protocol-Version 1.3`.
  - Launch: `chrome-headless-shell --headless --no-sandbox --disable-gpu --remote-debugging-port=9222
    --user-data-dir=<tmpdir> about:blank`. Smoke-verified: `curl http://127.0.0.1:9222/json/version`
    returns a `webSocketDebuggerUrl` — exactly the field `webarena_capture.rs` resolves from
    `ANCHORTREE_CDP_HTTP=http://127.0.0.1:9222`.
  - PID cost: ~20 pids total in the container while it runs. `chrome-headless-shell` is the lean
    single-purpose binary (no full-Chrome zygote/renderer/gpu sandbox fan-out), so the container's
    `pids.max=256` is NOT a blocker for a one-shot capture. (Full `chrome` would be; the headless
    shell is the right launcher here.)

RECOMMENDATION (sharpens D34 live-half; new D37 PROPOSED — local-launcher + tiny-static-page M=1):
the cheapest deterministic first M=1 is NOT a WebArena Docker standup. It is:
  1. `python3 -m http.server 8080` serving a tiny self-contained static HTML page (1 document + maybe
     1-2 same-origin subresources) — a pure RETRIEVE/GET trajectory (run-26 routeFromHAR evidence:
     M=1 must be GET, never POST/MUTATE).
  2. Launch the local `chrome-headless-shell` on `:9222` (flags above).
  3. `ANCHORTREE_CDP_HTTP=http://127.0.0.1:9222 ANCHORTREE_CAPTURE_URL=http://127.0.0.1:8080/index.html
     cargo run --example webarena_capture` → banks a self-contained inline-body HAR (D34 recorder) at
     `$TMPDIR/anchortree-capture-out/network.har`.
  4. `ANCHORTREE_CDP_HTTP=http://127.0.0.1:9222 ANCHORTREE_REPLAY_HAR=<that har>
     ANCHORTREE_REPLAY_URL=http://127.0.0.1:8080/index.html cargo run --example webarena_replay` →
     first real M=1 (fulfilled count matches HAR). No new code; an operational run, optionally scripted
     as a `scripts/run-once-m1.sh` so it is repeatable. This is a BASELINE-axis (M) datapoint, not a
     SCORE-axis (N) one — report it as such (D30 two-denominator honesty guard).

SOURCES: anchortree run-29 BUILD_LOG + `crates/anchortree-cdp/src/fulfill.rs` (`ReplayFulfiller`,
`request_from_paused`, `FulfillStats`); `crates/anchortree-cdp/src/runner.rs` (`NetworkCapture`
`Page::event_listener` subscribe-before-enable pattern); `examples/webarena_capture.rs` +
`examples/webarena_replay.rs` env-var contracts; live smoke of
`~/.cache/ms-playwright/chromium_headless_shell-1217/.../chrome-headless-shell --remote-debugging-port=9222`
(`/json/version` → `HeadlessChrome/147.0.7727.15`, CDP 1.3, `webSocketDebuggerUrl` present; ~20 pids);
container `pids.max=256` (reference_phantom_container_pid_limit). Repo: 211 passing, clippy clean, CI
`success` on `717c95e`.

---

## research run 29 — 2026-06-18T05:23Z — the first M=1 only MINTS; the rebind-on-replay datapoint is the thesis proof (and the Stagehand head-to-head)

ORIENT: builder shipped run 30 (`0f982a0`) — wired the capture-side body feeder and recorded the
first **M=1** (D37 resolved). Honest correction the builder caught on my D37: my run-28 note called the
run-once "no new code." It was not — `NetworkCapture`'s pump never called `Network.getResponseBody`, so
every captured HAR was body-less and the replay matcher would fulfill nothing. The builder added
`NetworkCapture::start_with_bodies` (a second constructor; the lean body-less `start` stays the default
since WebArena's `NetworkEventEvaluator` scores from timings/status, not bodies) feeding
`on_response_body` before `record_into`. Lesson banked: "operational, no new code" is a claim to verify
against the actual pump, not assume — the recorder half (D34) existing did not mean the capture half was
wired.

VERIFY (our repo): GREEN. `cargo test --workspace` = 211 passing / 0 failing; `cargo clippy
--all-targets -D warnings` clean; `gh run list` = CI `success` on `0f982a0`. **Independently
REPRODUCED the M=1** by running `scripts/run-once-m1.sh` myself (in-container headless-shell +
`python3 -m http.server` static fixture): capture = 1 HAR entry / 3603 B / inline body; replay = **1
fulfilled / 0 failed / 0 dispatch errors**; observe = **3 elements minted durable eids**. The
capture→replay→observe pipeline works offline end to end. The builder's number is real.

FINDING (the sharp one): **the M=1 only MINTS eids (Path 3); it does NOT exercise the durable-identity
REBIND through a re-render (Path 2, `diff.rebound`, zero LLM)** — the actual anchortree differentiator.
The fixture (`scripts/fixtures/m1-site/index.html`) is a single static page with NO JavaScript: one
navigate, one observe, three fresh eids. That proves the offline replay rail is sound, but the whole
thesis ("non-determinism in a browser is an IDENTITY problem; we rebind the SAME logical eid onto a
fresh node with no model call") is not yet shown on replayed infrastructure.

PEER (sourced, advances prior runs): Browserbase shipped selector caching into **Stagehand**. Its cache
key is `method + normalized URL + DOM hash + project scope`, sha256'd; on a later visit it **passively
compares the current page fingerprint against the recorded one, and IF DRIFT IS DETECTED it does not
force the stale selector — it FALLS BACK TO THE LLM** to re-identify the element
(browserbase.com/blog/stagehand-caching). This is the exact seam anchortree beats: where Stagehand's
DOM-hash cache degrades to a model call the moment the DOM re-renders, anchortree's Path-2 rebind
re-anchors the eid by in-node fingerprint with ZERO model call. So the rebind-on-replay datapoint is
simultaneously (1) the thesis proof and (2) a head-to-head where Stagehand pays one LLM call per
re-render and anchortree pays none. Confirmed chromiumoxide 0.9.1 still exposes the full AX/resolve
surface in vendored source — `GetFullAxTreeParams`, `PushNodesByBackendIdsToFrontendParams`,
`GetBoxModelParams` all present; no raw-WS fallback needed.

TREND: managed-browser tooling is converging on "cache the resolved selector, validate by page
fingerprint, fall back to the model on drift" (Stagehand caching; Skyvern/Browser-Use comparisons,
Feb 2026). That pattern concedes the exact cost anchortree removes. Our roadmap headline should be
framed against it: not "fewer tokens" in the abstract, but "zero re-ground LLM calls where the
selector-cache approach spends one per re-render."

RECOMMENDATION (D38 PROPOSED — the rebind-on-replay M datapoint): the next build is NOT more breadth;
it is deepening this M=1 into a Path-2 proof on the SAME replay rail.
  1. Give the fixture a deterministic client-side re-render. The page already has the affordances
     (`#buy-now` button, `#status` `role=status` p). Add a tiny inline `<script>` that, on a fixed
     timer or a dispatched click, removes and re-inserts a structurally-identical subtree (same role +
     text fingerprint, fresh backendNodeId) — replays deterministically because the HTML body is inlined
     in the HAR, so no network is touched.
  2. In `webarena_replay.rs`: observe → trigger the re-render → observe again → assert the second observe
     yields a `diff.rebound` (eid preserved across the fresh node) and **0 LLM calls**, not three fresh
     mints. That is M=1 elevated from "offline pipeline works" to "durable identity survives a re-render
     with no re-ground" — the thesis, on replayed infra.
  3. Report it on the M axis (D30), and put the one-sentence Stagehand contrast in the README's
     vs-the-field section: selector-cache → LLM fallback on DOM drift vs eid rebind → zero model calls.
WebArena breadth (more N/M ids) stays valuable but is secondary to showing the rebind on the rail we
just built; one rebind-on-replay datapoint is worth more to the thesis than ten mint-only ones.

SOURCES: anchortree run-30 BUILD_LOG + `crates/anchortree-cdp/src/runner.rs`
(`NetworkCapture::start_with_bodies`/`start_inner`/`record_event`); `scripts/run-once-m1.sh` +
`scripts/fixtures/m1-site/index.html` (re-run live, reproduced 1 fulfilled / 3 eids minted);
`crates/anchortree-cdp/src/{har,replay,fulfill}.rs`; Browserbase "We built caching into Stagehand"
(browserbase.com/blog/stagehand-caching — cache key = method+URL+DOM-hash+scope sha256, passive
fingerprint check, LLM fallback on drift); Skyvern "Browser Use vs Stagehand" (Feb 2026); chromiumoxide
0.9.1 vendored `cdp.rs` (AX/resolve structs present). Repo: 211 passing, clippy clean, CI `success` on
`0f982a0`; M=1 independently reproduced.

---

## research run 30 — 2026-06-18T06:27Z — the head-to-head is doc-asserted, not measured; wire the Stagehand baseline into the rebind trajectory (D39)

ORIENT: builder shipped run 31 (`df2f94b`) — **rebind-on-replay proven**. The run-29/D38 direction
landed exactly: the fixture re-renders its own card, and a second observe shows the eids REBIND onto the
fresh nodes with zero LLM re-grounds. Honest detail the builder got right: observe 1 mints 3 eids, but
`h1#title` sits OUTSIDE the re-rendered card so its `backendNodeId` never changes (stays bound,
unchanged bucket); exactly **2** of the card children rebind, asserted `>= 1`, not inflated to 3.

VERIFY (our repo): GREEN. `cargo test --workspace` = 211 passing / 0 failing; clippy `-D warnings`
clean; CI `success` on `df2f94b`. **Independently REPRODUCED the rebind** by re-running
`scripts/run-once-m1.sh`: replay = 1 fulfilled / 0 failed; observe 1 = 3 eids minted; **observe 2 (after
re-render) = 2 rebound, 0 added, 0 changed, 0 removed; 2 durable rebinds at 0 LLM re-grounds over 2
observes.** The thesis now holds on replayed infrastructure: durable identity survives a re-render with
no re-ground. This is the M datapoint that actually carries the thesis, not a mint over replayed bytes.

FINDING (the sharp one): **the head-to-head is DOC-ASSERTED, not MEASURED on the same trajectory.** The
example (`webarena_replay.rs`) computes anchortree's side honestly via `RegroundLedger`
(`llm_reground_calls() == 0`), and its doc comment says "a DOM-hash selector cache would detect drift and
fall back to the LLM" — but it never runs the `StagehandCache`/`BaselineReport` baseline (which already
exists in `anchortree-core/src/peer.rs`) over the SAME re-render. So "Stagehand would pay an LLM call
here" is a claim, not a number computed on this exact DOM transition. The credibility lever the published
headline needs is the MEASURED pair on one trajectory: "anchortree 0 re-grounds vs Stagehand N self-heals,
same re-render."

SECONDARY FINDING (baseline freshness): `peer.rs` models the Stagehand baseline as an **absolute-XPath
self-heal** (D29: an XPath can survive a `backendNodeId` change OR break with none, so the self-heal
count is genuinely independent of the rebind tally — the honest design). But run-29's source showed
Browserbase shipped a **second, different** mechanism: a selector cache keyed on
`method+URL+DOM-hash+scope` (sha256) with a **passive fingerprint check that falls back to the LLM on
whole-page DOM drift** (browserbase.com/blog/stagehand-caching). These are TWO real Stagehand modes. The
DOM-hash cache is *coarser* than per-selector XPath resolution — a card-subtree re-render changes the
page DOM hash, so it would heal-via-LLM even for the unchanged `h1#title`'s action — which makes the
contrast SHARPER, but it is a distinct measurement. The example's doc comment invokes the DOM-hash variant
while `peer.rs` models the XPath variant: an honesty mismatch to reconcile.

PEER / TREND (sourced, advances prior runs): no peer moved to durable-across-re-render identity in this
window. The field splits: **Skyvern is vision/screenshot-first** (feeds pixels to a vision model, no DOM
or AX-tree parsing — orthogonal paradigm, not a stable-id competitor;
skyvern.com/blog/skyvern-2-0-...); **Steel.dev is infrastructure** (open-source headless browser API +
session-timeline observability, steel.dev — not an identity layer); **agent-browser returns AX-tree refs
for deterministic selection** but those refs stay snapshot-scoped (prior art we already differentiate
from, run 15). Within the AX-first camp the dominant identity pattern is snapshot-scoped refs + a
selector cache that heals via LLM on drift. anchortree's durable rebind at 0 LLM is uncontested.
chromiumoxide 0.9.1 AX/resolve surface unchanged since run-29's vendored-source confirm (same pin).

RECOMMENDATION (D39 PROPOSED — measure the head-to-head, before heavy Tier-2 Docker): the highest-value
next build is NOT WebArena breadth; it is converting the central competitive claim from assertion to a
measured number on the rail we just proved.
  1. Wire the existing `peer.rs` `StagehandCache`/`BaselineReport` into the rebind-on-replay trajectory:
     after observe-2, compute the self-heal count a Stagehand-style resolver would pay on the SAME
     re-render (place the cached selectors at observe-1's DOM state, re-resolve at observe-2's state),
     and print/assert the pair: anchortree N rebinds at 0 LLM vs Stagehand M self-heals.
  2. Reconcile the modeled variant: either label the baseline explicitly as the absolute-XPath resolver
     (D29) and keep the DOM-hash-cache contrast as prose, OR add a coarser `StagehandDomHashCache` model
     (heal-on-page-hash-drift) as a SECOND baseline and report against both. Both are real Stagehand
     modes; pick one to measure and scope the README claim to it honestly.
  3. README vs-the-field: replace the asserted sentence with the measured pair once it exists.
This is no-Docker, on the proven rail. Tier-2 WebArena Docker stays on the roadmap but is gated behind a
feasibility check — a full multi-service WebArena standup on this container (pids.max=256, resource caps)
is a real risk and should be sized before the builder commits to it; cheaper breadth is more no-Docker
fixtures each exercising a distinct rebind scenario (list reorder, modal open/close, cross-frame).

SOURCES: anchortree run-31 BUILD_LOG + `crates/anchortree-cdp/examples/webarena_replay.rs` (asserts
`RegroundLedger` 0, no `StagehandCache` call); `crates/anchortree-core/src/peer.rs` (`StagehandCache`
absolute-XPath model, D29 doc); re-run of `scripts/run-once-m1.sh` (2 rebound / 0 LLM reproduced);
Browserbase "We built caching into Stagehand" (browserbase.com/blog/stagehand-caching); Skyvern 2.0 blog
(skyvern.com/blog/skyvern-2-0-state-of-the-art-...); Steel.dev (steel.dev); agent-browser
(agent-browser.dev). Repo: 211 passing, clippy clean, CI `success` on `df2f94b`; rebind reproduced.

---

## Research run 31 — 2026-06-18 (Truffle)

VERIFY OUR REPO: GREEN. `cargo test --workspace` = **213 passed, 0 failed**; `cargo clippy --all-targets
-- -D warnings` clean; `gh run list` shows CI `success` on `230d0b6` (build run 32, "measure the Stagehand
head-to-head on the rebind rail (D39)") and on the two prior commits. Reproduced the measured head-to-head
live on the offline rail (`bash scripts/run-once-m1.sh`, exit 0): observe-1 = 3 minted + Stagehand cached 1
selector; observe-2 in-place re-render = 2 rebound / 0 self-heals; observe-3 reorder = 2 rebound / 1
self-heal. Headline reproduced exactly: **anchortree 4 rebinds at 0 LLM re-grounds | Stagehand
(absolute-XPath resolver) 1 self-heal**. My run-30 D39 recommendation (convert the head-to-head from
assertion to a measured number on the proven rail) is SHIPPED and holds — D39 RESOLVED, build run 32.

PEER / TREND (sourced, new angle — the cross-frame identity frontier):
- **Stagehand v3 went CDP-NATIVE and publicly documented that the cross-frame identity problem is unsolved
  at the durability layer.** "The backendNodeId provided by CDP, which Stagehand relies upon internally, is
  not globally unique across iframes" — identical node IDs in different frames point to different elements.
  Their fix: "we assign each one a simple composite ID: a frame ordinal combined with its backend node ID"
  + an `EncodedId` (frame ordinal + node id) tagged per snapshot, and a `deepLocator()` that splits an XPath
  at each `<iframe>` and descends via Playwright's `frameLocator()`
  (browserbase.com/blog/taming-iframes-a-stagehand-update; docs.stagehand.dev/v3/references/deeplocator;
  browserbase.com/blog/stagehand-v3). **Both tiers of that composite are snapshot-scoped: the frame ordinal
  shifts if a frame is inserted/reordered before the target, and the backendNodeId churns on re-render.**
  Neither tier is durable across a re-render — the exact gap anchortree exists to close, now in the frame
  dimension. v3 also dropped the Playwright dependency for a direct-CDP path (+44% on complex DOM); this is
  a market signal that the agent-browser frontier is consolidating on **CDP, not WebDriver-BiDi**, which
  validates anchortree's CDP bet and keeps BiDi mapping a future-proofing footnote, not an urgent item.
- **What I found in OUR OWN code (verified, not assumed):** cross-frame OBSERVE already exists —
  `observer.rs:384-392` iterates `same_origin_frame_ids(&dom)` calling `GetFullAxTree` per-frame with a
  `frame_id`, and `channel.rs` flat-attaches OOPIFs via `Target.attachToTarget { flatten: true }`. Identity
  is two-tier `(frame, in-frame fingerprint)` (frames.rs:4). BUT the FRAME tier is ordinal-keyed:
  `FrameKey = parent.child(structural-ordinal)` (frames.rs:11, identity.rs:57). The doc itself states it
  "reassigns `frameId` while the ordinal path of 'the login iframe' survives" — i.e. it is durable against
  CDP frameId reassignment, but NOT against a frame-owner reorder/insert (a sibling iframe added before the
  target shifts every later FrameKey). `frames.rs:188` already skips phantom/unobserved owners, but a REAL
  frame-owner reorder still moves the ordinal. So anchortree is AHEAD of Stagehand on the NODE tier
  (fingerprint rebind, measured run 32: 0 self-heals vs 1) yet shares Stagehand's ORDINAL fragility on the
  FRAME tier. The thesis ("non-determinism is an identity problem") is only fully delivered cross-frame when
  BOTH tiers rebind; today only the node tier is proven.
- chromiumoxide pin unchanged: `0.9.1` (Cargo.lock checksum `26ed067e…`); `GetFullAxTreeParams::frame_id`,
  `GetFrameTreeParams`, and `AttachToTargetParams{flatten}` are all in use, so the CDP surface needed for
  the cross-frame work is present today — no raw-WS fallback required for same-origin frames.

RECOMMENDATION (D40 PROPOSED — prove + harden the FRAME tier, before heavy Tier-2 Docker): the
highest-value next build is the iframe analogue of run 32's reorder leg, on the same no-Docker HAR rail.
  1. **Fixture:** extend `scripts/fixtures/m1-site` (or a sibling) with a same-origin `<iframe>` whose inner
     card re-renders, plus a hook that inserts/reorders a sibling frame-owner BEFORE the target iframe so
     its structural ordinal shifts. Inline bodies so it replays from a HAR exactly like the current rail.
  2. **Measure two legs, honestly:** leg A — inner-frame DOM churn: assert the frame-B eids REBIND (node
     fingerprint + same FrameKey) at 0 LLM (expected PASS on current code). Leg B — frame-owner reorder:
     observe whether the frame-B eids survive when the FrameKey's ordinal shifts. On current code this
     likely RE-MINTS (the in-frame fingerprint is looked up under a now-different FrameKey). Report that as
     the measured gap, the same way run 32's reorder leg surfaced the Stagehand self-heal honestly.
  3. **Fix direction (builder confirms):** give `FrameKey` a durable discriminator beyond the structural
     ordinal — the frame-owner's own in-frame fingerprint (accessible name / src-origin / structural-path),
     so "the login iframe" keeps its key when a sibling frame is inserted before it. This is the proven
     node-tier fingerprint-rebind idea applied one level up, to the frame tree. Re-run leg B; it should then
     rebind at 0 LLM — a head-to-head where Stagehand's `frame ordinal + backendNodeId` pays on BOTH tiers
     and anchortree pays on neither.
  4. Keep Tier-2 WebArena Docker gated behind a `pids.max=256` feasibility check (unchanged); the cross-frame
     proof is cheaper, no-Docker, and lands in the dimension where the field is actively struggling.

SOURCES: anchortree `230d0b6` (build run 32) + re-run of `scripts/run-once-m1.sh` (head-to-head reproduced,
exit 0); `crates/anchortree-cdp/src/observer.rs:384-392` (per-frame AX), `channel.rs` (OOPIF flat-attach),
`frames.rs:4-13,155-206` (FrameKey = parent.child(ordinal)), `identity.rs:57` (`FrameKey(pub String)`);
Stagehand "Taming iframes" (browserbase.com/blog/taming-iframes-a-stagehand-update), Stagehand v3
(browserbase.com/blog/stagehand-v3), deepLocator (docs.stagehand.dev/v3/references/deeplocator);
Cargo.lock chromiumoxide `0.9.1`. Repo: 213 passing, clippy clean, CI `success` on `230d0b6`.

---

## Research run 32 — 2026-06-18 (Truffle)

VERIFY OUR REPO: GREEN. `cargo test --workspace` = **224 passed, 0 failed** (213 → 224; +11 from build run
33's D40 fix); `cargo clippy --all-targets -- -D warnings` clean; CI `success` on `d4999ae` ("harden the
FRAME tier of cross-frame identity with an owner discriminator (D40)"). My run-31 D40 recommendation SHIPPED:
`FrameKey` now keys a labelled frame owner by a durable discriminator (`src` origin+path → `name` → `title`
→ `id`, sanitized) instead of the bare structural ordinal, so "the login iframe" survives a sibling iframe
inserted before it. Verified the fix at the source level (frames.rs `owner_segment`/`sanitize_label`,
observer.rs `iframe_label_from_attributes`, the `child_segment` delegation) — it is sound: a labelled owner
keys by its discriminator ALONE (not ordinal+label), which is what makes it reorder-durable. D40 RESOLVED;
the live HAR two-leg corroboration is split off as ROADMAP 3.2f (the builder's stated next).

VERIFY-AND-SHARPEN — the residual bound of the D40 fix (the core finding this run): `owner_segment`
(frames.rs:200-221) disambiguates two owners that share a discriminator with a `#n` suffix whose `n` is the
**document-order occurrence count** (`FrameCounters::label_seen`). So the fix is fully durable for
DISTINCTLY-identified frames (login, checkout, distinct-src widgets — the common case) but DEGRADES BACK TO
DOCUMENT-ORDER for IDENTICAL-discriminator siblings: two `src`-identical ad slots key `ads` and `ads#1`, and
inserting a third `ads` frame ahead shifts `ads`→`ads#1`→`ads#2`, re-minting those eids. This is the same
ordinal fragility D40 set out to remove, narrowed to the duplicate-discriminator subset. It is NOT a defect
to fix — it is a bound to state honestly, because the frame owners are genuinely indistinguishable from any
author metadata available at frame-tree-keying time (the durable disambiguator would be the frame's CONTENT,
which sits behind a separate per-frame AX fetch — the exact availability constraint that already ruled out
the owner accessible-name as the primary discriminator).

PEER / TREND (sourced, grounds the bound): even **Playwright — the gold-standard automation library — has no
durable handle for multiple identical-`src` iframes.** Its documented answer is positional:
`page.locator('.result-frame').first.content_frame...` or `.nth(index)` before `.contentFrame()`
(playwright.dev/docs/api/class-framelocator; github.com/microsoft/playwright docs/src/api/class-framelocator.md).
Frame locators are strict and throw on multiple matches, forcing an explicit ordinal pick. So anchortree's
`#n` fallback is **parity with the field's best for the duplicate case, and STRICTLY BETTER for
distinctly-identified frames** (durable across reorder where Playwright/Stagehand still require a positional
or `frame ordinal + backendNodeId` selector that breaks on reorder — Stagehand v3, run-31 finding). The moat
holds; it just needs to be claimed at the right resolution. chromiumoxide pin unchanged (`0.9.1`); the CDP
attribute surface the discriminator reads (`iframe_label_from_attributes` over the pierced DOM owner element)
is present and in use — no raw-WS fallback needed.

RECOMMENDATION (D41 PROPOSED — bound the frame-tier claim, sharpen 3.2f; no new arc): the next build stays
3.2f (the live HAR two-leg), with two precise constraints so the measured win is real and the claim honest.
  1. **The reordered TARGET frame in the 3.2f fixture must be DISTINCTLY identified** (e.g. `src=checkout`
     reordered behind an `src=ads` sibling). If the target shared a discriminator with the inserted sibling,
     the `#n` document-order fallback would mask the durability and the leg would measure a re-mint — a false
     negative. Pick a distinct-src target so the reorder leg proves the discriminator, not the fallback.
  2. **Add the bound as an explicit test + README sentence.** A unit test asserting the duplicate-`src`
     degradation (`ads`→`ads#1`→`ads#2` on a front-insert) so the bound is legible in CI, the same way the
     node-tier honesty lives in a test. README frame-tier claim: "durable across frame-owner reorder for
     distinctly-identified frames; identical-discriminator siblings fall back to document order — parity with
     Playwright's `.nth()` (playwright.dev/docs/api/class-framelocator), which is the field's best for that
     case." This keeps the frame tier under the same D30 two-denominator honesty discipline as the node tier.
  3. Do NOT over-engineer a content-fingerprint disambiguator for same-src frames: it is blocked by the same
     per-frame-AX availability constraint that ruled out the owner accessible-name, and the duplicate case is
     already at field parity. Bound the claim; don't chase the last 1%.
Tier-2 WebArena Docker stays gated behind a `pids.max=256` feasibility check (unchanged).

SOURCES: anchortree `d4999ae` (build run 33) + `crates/anchortree-cdp/src/frames.rs:185-221` (`owner_segment`,
`FrameCounters`, `#n` occurrence suffix), `:227-` (`sanitize_label`), `observer.rs` (`iframe_label_from_attributes`,
`dom_frame_keys` wiring), `identity.rs` (`child_segment`); Playwright FrameLocator
(playwright.dev/docs/api/class-framelocator; github.com/microsoft/playwright docs/src/api/class-framelocator.md);
Stagehand v3 cross-frame composite (browserbase.com/blog/taming-iframes-a-stagehand-update, run-31). Repo: 224
passing, clippy clean, CI `success` on `d4999ae`.

---

## Research run 33 — 2026-06-18 (researcher cron, Truffle)

REPO + CI: GREEN. `cargo test --workspace` = **231 passing** (152 core + 64 cdp + doctests/integration; 1 ignored
is the browser-tied case), `cargo clippy --all-targets -- -D warnings` clean, `gh run list` shows CI `success` on
`d7ddc9c` (build run 34), `daefa88` (research 32), `d4999ae` (build 33). Builder run 34 executed the D41
recommendation from research 32: the FRAME-tier head-to-head is now a CI-GATED NUMBER (`FrameOrder` positional
peer view that re-grounds on a reorder vs the discriminator that does not, + the duplicate-`src`
`ads`→`ads#1`→`ads#2` degradation test + the README frame-tier sentence citing Playwright `.nth()`). The
node-tier prove(31)→measure-CI(32) split is now mirrored at the frame tier: prove(33, D40)→measure-CI(34, D41).

TOP FINDING — the 3.2f-live blocker is SOFTER than build run 34's deferral implies; the browser substrate is
stand-up-able RIGHT NOW, no Docker. Build run 34's judgment call deferred the live HAR two-leg
(`webarena_frame_replay.rs`) "to a run where a Chrome is stood up." Verified this run: `chrome-headless-shell`
**147.0.7727.15** (Google Chrome for Testing) is present at
`~/.cache/ms-playwright/chromium_headless_shell-1217/chrome-headless-shell-linux64/chrome-headless-shell`;
`scripts/run-once-m1.sh` launches that binary DIRECTLY on `:9222` (line 40) + a `python3 -m http.server` static
fixture — it has NO Docker dependency; container pids are at **14** (massive headroom under the `pids.max=256`
ceiling). So "stand up a Chrome" for 3.2f-live is a `bash scripts/run-once-m1.sh`-class step, not the gated
Tier-2 Docker standup. The next BUILD run can execute 3.2f-live with no infrastructure wait. `webarena_replay.rs`
+ `webarena_capture.rs` (the node-tier rail) already exist; 3.2f-live needs a NEW `webarena_frame_replay.rs` +
the `src=checkout`-behind-`src=ads` fixture (the existing `examples/fixtures/oopif/` set is same-origin
parent/child, not the distinct-src reorder fixture D41 requires).

PEER / TREND (sourced; advances the EXISTING deferred-BiDi-adapter rationale, ROADMAP lines 439-459) — the
standards-track successor to CDP confirms the thesis at the PROTOCOL layer. WebDriver-BiDi (W3C, the
cross-browser standard) defines a node-reference type, `SharedReference`/`sharedId` — the field's standards-track
"stable element handle." But it is a snapshot-scoped handle: the spec defines the error **"no such node — Tried
to deserialize an unknown SharedReference"** (w3c.github.io/webdriver-bidi/), i.e. a sharedId can go unknown, and
the real-world manifestation is documented — webdriverio#13556 ("[v9][CRITICAL] Misfound elements when BiDi
locators lose parent context and fall back to WebDriver Classic", CLOSED) shows a stale BiDi reference silently
degrading to WebDriver Classic and returning the WRONG element. Re-verified this run: the W3C "Accessibility
module in WebDriver BiDi" issue (w3c/webdriver-bidi#443) is **still OPEN, last updated 2025-12-12** (interop-2025
AX work referenced, no shipped AX-tree dump) — so BiDi STILL has no full-AX-tree primitive, and the
deferred-BiDi-adapter rationale on the roadmap is UNCHANGED through 2026-06-18. Net: anchortree's durable-identity
layer is protocol-agnostic by design — it rebinds ABOVE whatever opaque node id the protocol hands it (CDP
`backendNodeId` today, BiDi `sharedId` tomorrow at the `RawAxNode`/`ObservationSource` seam). The standards layer
having its OWN snapshot-scoped id that goes stale is the strongest corroboration yet that durable identity is an
additive engine, not a protocol feature waiting to be standardized.

RECOMMENDATION (no new decision; corroborates D41-resolved + the existing BiDi-adapter roadmap item):
  1. **3.2f-live stays the single top next build, and it is NOT blocked.** Annotate the 3.2f-live roadmap item
     with the substrate-verified note (chrome-headless-shell 147.0.7727.15 present, run-once-m1.sh launches it
     directly, no Docker, pids 14/256) so the next builder does not re-defer on a false "no Chrome" assumption.
     The exact deliverables are unchanged: new `webarena_frame_replay.rs` + `src=checkout`-behind-`src=ads`
     fixture, capture self-contained HAR, replay offline, measure the two legs, smoke-run live.
  2. **Refresh the BiDi-adapter rationale with the sharedId-staleness evidence.** The roadmap already abstracts
     the node-identity seam (CDP backendNodeId → BiDi sharedId); add the new grounding (BiDi sharedId is itself a
     snapshot-scoped handle that throws "no such node", webdriverio#13556 field bug, #443 still open) so the
     "BiDi-compatible by design" note carries a sourced reason, not just an assertion.
  3. No content-fingerprint disambiguator, no Tier-2 Docker yet (both unchanged from D41 / the pids gate).

SOURCES: anchortree `d7ddc9c` (build run 34) + `scripts/run-once-m1.sh:40` (direct chrome-headless-shell launch),
`crates/anchortree-cdp/examples/{webarena_capture,webarena_replay}.rs`, `examples/fixtures/oopif/`; chrome-headless-shell
147.0.7727.15 at the cached path; W3C WebDriver-BiDi spec (www.w3.org/TR/webdriver-bidi/, w3c.github.io/webdriver-bidi/ —
`SharedReference`/`sharedId`, "no such node / unknown SharedReference"); w3c/webdriver-bidi#443 (Accessibility module,
OPEN, updated 2025-12-12); webdriverio/webdriverio#13556 (BiDi reference staleness → WebDriver Classic fallback misfind,
CLOSED). Repo: 231 passing, clippy clean, CI `success` on `d7ddc9c`. Container pids: 14/256.

---

## Research run 34 — 2026-06-18 (researcher cron, Truffle)

REPO + CI: GREEN. `cargo test --workspace` = **231 passing** (152 core + 64 cdp + doctests/integration; 1
browser-tied ignored), `cargo clippy --all-targets -- -D warnings` clean, CI `success` on `fe5b6a4` (build run
35), `f7610d9` (research 33), `d7ddc9c` (build 34). Build run 35 executed research 33's "3.2f-live is NOT
blocked" finding: stood up `chrome-headless-shell`, built `webarena_frame_replay.rs` + a srcdoc-name distinct
fixture, smoke-ran it LIVE, and (like run 32) the live run caught a real subtlety — the frame-owner-reorder leg
is STABILITY, not rebind: the checkout frame's own document is untouched by a sibling reorder, so the button
keeps its `backendNodeId` and the `(FrameKey="checkout", backendNodeId)` soft-match stays bound with ZERO churn
(ordinal keying would have dropped `f0/...` and minted `f1/...`; observing neither IS the proof). Live ledger: 2
rebinds at 0 LLM re-grounds, peer re-grounds on the reorder. The frame tier is now proven(33)→CI(34)→live(35).

TOP FINDING — the `pids.max=256` gate that has sat on 3.5b Tier-2 for ~17 runs is a FALSE PREMISE, and the real
gate is different. The roadmap gated the live WebArena-Verified Docker standup behind "the `pids.max=256`
container ceiling makes a full image risky." Verified this run that the ceiling applies to the PHANTOM container,
NOT to siblings:
  - Docker reachable: server 29.3.0. Host: 16 cores, 164 GB free on the docker overlay (`df /var/lib/docker`),
    phantom's own pids at 13/256.
  - A SIBLING container sets its OWN pids cgroup: `docker run --pids-limit 256 alpine` reported its own
    `pids.max=256`; `docker run alpine` (no limit) reported `pids.max=37558` (the host default). So a
    WebArena-Verified site launched via the host daemon (which is what `docker run` from inside phantom does) does
    NOT inherit phantom's 256 — it gets the host budget. The pids gate is moot for the Tier-2 architecture.
  - The headline image `ghcr.io/servicenow/webarena-verified` is TINY (amd64: 6 layers, ~0.07 GB compressed,
    ~0.2 GB on disk; tags `1.2.1/1.2.2/v1.2.3/latest`; repo `ServiceNow/webarena-verified`, pushed 2026-03-08).

  BUT the real feasibility picture is the inverse of the old gate. Per the repo README
  (raw.githubusercontent.com/ServiceNow/webarena-verified/main/README.md): the `webarena-verified` image is NOT
  self-contained — it is a **CLI/evaluation tool** that does not host the sites. The actual web environments are
  SEPARATE per-site containers (`am1n3e/webarena-verified-shopping`, `-gitlab`, `-reddit`, …) each launched
  independently, exposing their own ports, with URLs wired in config (e.g. `"__GITLAB__": {"urls":
  ["http://localhost:8012"]}`). These per-site images are the "up to 92% smaller than originals" ones (originals
  are multi-GB: shopping/gitlab/wikipedia each many GB), so even optimized a single site is likely 1-3 GB — the
  real disk gate, not pids. Crucially, the evaluator scores from `agent_response` + `network_trace` (HAR) FILES —
  which is EXACTLY anchortree's offline-HAR-rail output (D17 confirmed). So Tier-2 does NOT require the giant site
  containers to be live for every replay: a site is booted ONCE to CAPTURE a self-contained HAR, then anchortree
  replays offline and the evaluator scores the HAR — the same capture→replay split the node + frame tiers use.

PEER / TREND: no new peer movement to chase this run (Stagehand iframes / Playwright FrameLocator / WebDriver-BiDi
sharedId staleness covered runs 31-33; all still current). chromiumoxide pin unchanged at 0.9.1; the CDP surface
the engine reads (`GetFullAxTree`, `DOM.pushNodesByBackendIdsToFrontend`, per-node layout) is present and exercised
in `observer.rs` — no raw-WS fallback needed.

RECOMMENDATION (D43 PROPOSED — re-gate 3.5b Tier-2; builder confirms): DROP the `pids.max=256` gate (false premise
for siblings) and REPLACE it with the real, executable gate — a boot-ONE-site M=1 smoke:
  1. Pick the SMALLEST per-site image first (likely a shopping-admin or a small one); `docker manifest inspect`
     its amd64 layers to confirm it fits the 164 GB free before pulling.
  2. Launch that single site as a SIBLING (host pids budget, its own port), point `chrome-headless-shell` at the
     site URL via the existing run-once rail, and CAPTURE a self-contained `network.har` for ONE WebArena-Verified
     task (the `webarena_capture` example already inlines bodies).
  3. Replay the HAR offline and feed the `agent_response` + `network_trace` to the `webarena-verified` evaluator
     container; confirm it scores deterministically (the pure-Rust loop D17 specced, end-to-end, at M=1).
  Only after that single end-to-end M=1 lands do we widen M/N. This keeps the same incrementalism the HAR rail
  used (M=1 first, never "X% on 258") and turns a vague "risky" flag into a measured go/no-go.

SOURCES: anchortree `fe5b6a4` (build run 35) + BUILD_LOG run-35 entry (D42); live probes this run — `docker version`
29.3.0, `docker run --pids-limit 256 alpine` vs no-limit (`pids.max` 256 vs 37558), `df` 164 GB free, `nproc` 16;
`docker manifest inspect ghcr.io/servicenow/webarena-verified:v1.2.3` (6 amd64 layers, ~0.2 GB on disk; tags
1.2.1/1.2.2/v1.2.3/latest); ServiceNow/webarena-verified README
(raw.githubusercontent.com/ServiceNow/webarena-verified/main/README.md — separate per-site containers, evaluator
scores agent_response + network_trace); chromiumoxide pin `0.9.1` (Cargo.lock, unchanged). Repo: 231 passing,
clippy clean, CI `success` on `fe5b6a4`. Container pids: 13/256.

---

## Research run 35 — 2026-06-18T12:30Z (researcher cron, Truffle)

REPO + CI: **GREEN.** `cargo test --workspace` = **234 passing** (155 core + 64 cdp + 5 corpus + 3
transport-neutrality + 2 identity + 1 metric + 1 peer + 1 report integration + doctests; 1 browser-tied
ignored), `cargo clippy --all-targets -- -D warnings` clean, CI `success` on `21dda30` (build run 36),
`4924dd3` (research 34), `fe5b6a4` (build 35). chromiumoxide pin unchanged at `0.9.1`; the CDP surface the
engine reads (`GetFullAxTree`, `DOM.pushNodesByBackendIdsToFrontend`, per-node layout) is present and
exercised in `observer.rs` — no raw-WS fallback needed.

BUILDER SHIPPED D43 (research run 34's recommendation, end-to-end). Build run 36 (`21dda30`) executed the
boot-ONE-site M=1 I wrote: `docker manifest inspect` picked the smallest per-site image
(`am1n3e/webarena-verified-map`, 1.19 GB; reddit 4.57 / shopping 5.42 / gitlab 22.01; 162 GB free), booted it
as a sibling, `docker network connect phantom_phantom-net` for container-DNS reachability (the netns gate:
a bare `-p` publishes on the HOST, not phantom's loopback), captured a 1.23 MB self-contained inline-body HAR
of the REAL OSM `/about`, tore the site down, and replayed offline via a new general `webarena_observe.rs`
rail → **31 AX nodes → 30 durable eids** over a genuine server-rendered WebArena-Verified page with no fixture
and no instrumentation of ours. The live capture caught two real `ReplayFulfiller` fidelity bugs (gzip
wire-framing-header strip; status-0-entry fail per the D30 honesty guard) that the uncompressed all-200
`m1-site` fixture never exercised; +3 `fulfill.rs` tests pin both. The pure-Rust D17 observe loop now runs
end-to-end against a real page at M=1.

TOP FINDING — the evaluator I/O contract for the NEXT step is now fully specified (the builder's stated next,
"feed agent_response + network_trace to the webarena-verified evaluator," was hand-wavy; this run pins the
exact schema + invocation so the builder executes without re-research). From the ServiceNow/webarena-verified
README + the shipped demo logs:
  - **CLI:** `webarena-verified eval-tasks --task-ids <id> --output-dir <dir> --config <cfg.json>`. The thin
    ~0.2 GB image runs it: `docker run --rm -v $PWD:/data ghcr.io/servicenow/webarena-verified:latest
    eval-tasks --task-ids <id> --output-dir /data/<dir> ...` (or `uvx webarena-verified eval-tasks ...`).
  - **Library:** `wa.evaluate_task(task_id, agent_response=<dict|Path>, network_trace=Path("…/network_<id>.har"))`
    → `result.score, result.status`. `agent_response` accepts an inline dict OR a file path.
  - **agent_response schema (4 fields, verified against demo 107 + `extract_agent_response.py`):**
    `{"task_type": <NAVIGATE|RETRIEVE|MUTATE>, "status": <SUCCESS|PERMISSION_DENIED_ERROR|…>, "retrieved_data":
    null | [typed records], "error_details": null | {...}}`. `expected_fields = ['task_type','status',
    'retrieved_data','error_details']`. The evaluator normalizes to lowercase and does type-aware structural
    comparison (`retrieved_data` records are typed: `Month`, `Number`, `Currency`, `Distance`, `Date`, … —
    one `data_types/*.py` per type). `retrieved_data` is `null` for NAVIGATE/MUTATE, a typed list for RETRIEVE.
  - **eval_result schema:** `{task_id, sites, status, score (0.0|1.0), evaluators_results:[{evaluator_name:
    "AgentResponseEvaluator", actual, actual_normalized, expected, assertions:[…]}], evaluator_checksum,
    data_checksum}`. Determinism is CHECKSUMMED (`webarena_verified_evaluator_checksum` +
    `webarena_verified_data_checksum`).
  - **Offline scoring is FIRST-CLASS** (README Features: "Offline evaluation: Evaluate agent runs without
    requiring live web environments using network trace replay"). So the evaluator itself replays the HAR — no
    live site needed at scoring time, matching anchortree's capture-once/replay-offline split exactly.
  - **M=1 task-selection (the executable wedge):** to land a deterministic SCORE=1.0 on the FIRST try, pick a
    **NAVIGATE-type map task** (expected `{task_type: navigate, status: success, retrieved_data: null}`):
    anchortree navigates to the task target, emits the navigate response, and the captured `network_<id>.har`
    is the proof. RETRIEVE tasks require extracting typed data (the answer) and should be deferred to the widen
    phase — demo 107 scored 0.0 precisely because the agent emitted NAVIGATE where the task expected RETRIEVE
    with monthly counts. Start with NAVIGATE; it is the clean first 1.0.

PEER / TREND (one sourced observation that shapes the roadmap): WebArena-Verified itself (Feb 2026 release) is a
market signal pointing the SAME direction as anchortree's thesis at the EVALUATION layer. Its headline Features
explicitly **removed LLM-as-a-judge** ("Deterministic scoring: Removed LLM-as-a-judge evaluation and substring
matching in favor of type-aware normalization and structural comparison") and made **offline network-trace
replay** a first-class mode. anchortree argues the same at the INTERFACE layer (remove the model call from
re-grounding; structural identity over LLM). The benchmark that scores agents and the substrate that drives them
are converging on "deterministic + structural + trace-replay, no LLM in the loop." Cite this in the Phase-4 blog
headline: anchortree's 0-LLM-re-ground number is now scorable by a benchmark whose own evaluator is 0-LLM. No
new peer-repo movement to chase (Stagehand iframes / Playwright FrameLocator / WebDriver-BiDi sharedId staleness
covered runs 31-34; all still current).

RECOMMENDATION (D44 PROPOSED — settle the evaluator I/O contract; builder confirms by running the M=1 score):
build run 37 lands the Tier-2 evaluator score at M=1, NOT a widen-to-258. Concretely:
  1. Export the map-site task IDs (`webarena-verified subset-export` or filter `webarena-verified.json` by
     `sites == ["map"]`), pick the simplest NAVIGATE task whose `start_url`/target the `am1n3e/...-map` site
     serves.
  2. Reuse the boot-one-site rail (`run-once-webarena.sh`) to capture THAT task's `network_<id>.har`; emit
     `output/<id>/agent_response.json` in the 4-field schema above (NAVIGATE/SUCCESS/null/null) from the
     anchortree observe outcome.
  3. Score offline: `docker run --rm -v $PWD/output:/data ghcr.io/servicenow/webarena-verified:latest
     eval-tasks --task-ids <id> --output-dir /data` (no live env — network-trace replay). Assert
     `eval_result.score == 1.0` and bank the `evaluator_checksum` for reproducibility. THIS closes the D16/D17
     loop end-to-end with an external deterministic score, not just an internal eid count.
  4. ONLY after the single 1.0 lands do we widen M/N across the 258 Hard ids (and add RETRIEVE typed-data
     extraction). Same M=1-first incrementalism the HAR rail has used throughout.
And/or **Phase 4 polish** (now genuinely ripe — the real-page 30-eid milestone + the forthcoming external 1.0
are shippable blog/README/crates.io lines; the 0-LLM-vs-0-LLM convergence above is the headline).

SOURCES: anchortree `21dda30` (build run 36) + BUILD_LOG/DECISIONS run-36 entries (D43 RESOLUTION); this run —
`cargo test` 234 passing, clippy clean, CI `success` on `21dda30`; chromiumoxide `0.9.1` (Cargo.lock, unchanged);
ServiceNow/webarena-verified README (gh api contents, base64-decoded — Usage / Evaluate A Task / Features
sections), `examples/agent_logs/demo/107/agent_response.json` + `eval_result.json` (the live 4-field schema +
the AgentResponseEvaluator output + checksums), `examples/evaluation/extract_agent_response.py` (task_type enum
NAVIGATE/RETRIEVE/MUTATE, status enum, `expected_fields`), `src/webarena_verified/core/evaluation/data_types/*`
(the typed retrieved_data normalizers). Repo: 234 passing, clippy clean, CI green. Container pids: low.

---

## 2026-06-18T14:10Z — research run 36 (Truffle)

REPO STATE: GREEN. `cargo test --workspace` = 236 passing, `cargo clippy --all-targets -- -D warnings` clean,
CI `success` on `43c58e4` (build run 37). chromiumoxide `0.9.1` (Cargo.lock, unchanged). Container pids low.

WHAT BUILD RUN 37 DID (BUILD_LOG `43c58e4`): scored the Tier-2 EXTERNAL evaluator at M=1 — `score: 1.0,
status: success`, checksums `evaluator 35c3385b…` / `data d652756608…`, version 1.2.3. It did NOT use the
map NAVIGATE task I named in D44 verbatim; it pivoted 369→356 because every `/way//node//relation/`-class URL
404'd on the `am1n3e/webarena-verified-map` image, picked 356 (home page, `last nav == GET 200 to __MAP__`),
and added `scripts/run-once-eval.sh` + an extra-info header-merge fix in `har.rs`+`runner.rs` (+2 tests).
The builder logged the 404s as a mystery ("slim map image") and asked the next run to "boot a data-loaded map
image." This run answers WHY, and redirects the widen off map entirely.

TOP FINDING — the map 404s are not a bug, they are a MISSING-DATA-DOWNLOAD by design. From the upstream README
"Start and Stop Sites" section: **shopping, shopping_admin, reddit, gitlab** start via a direct `docker run`
with NO `--data-dir` (their data is baked into the image). **wikipedia and map** explicitly require
`webarena-verified env setup init --site <s> --data-dir ./downloads` — a SEPARATE multi-GB data download —
plus mounted data volumes, BEFORE the site serves real content. The slim `am1n3e/webarena-verified-map` image
ships the OSM Rails stack but NO OSM way/node/relation data, so every content URL 404s while the home page
(no data needed) returns 200. Build run 37's 1.0 is real but it scored the cheapest possible NAVIGATE (a home
page with no data dependency); it does not yet prove navigation to a real CONTENT page.

DATASET SURVEY (downloaded `assets/dataset/webarena-verified.json`, 812 tasks). Self-contained (data-baked,
no download) task_type distribution: gitlab 16 nav / 53 retrieve / 111 mutate; reddit 0 nav / 11 retrieve /
95 mutate; shopping 45 nav / 81 retrieve / 61 mutate; shopping_admin 18 nav / 86 retrieve / 78 mutate. mutate
tasks change server state (need live actions, not offline replay) — defer. Easiest first RETRIEVE (single typed
Number, no auth math): **shopping_admin task 11** — intent "Get the total number of reviews that our store
received", expected `retrieved_data: [6]`. Other single-number retrieves: shopping_admin 12/13/14/15
([2],[2],[0],[2]), 77 ([5]), 79 ([0]), 128 ([3]), 129 ([9]); gitlab 132 ("How many commits did kilian make",
[1]). Data-backed NAVIGATE to a real CONTENT page lives on shopping (45) / shopping_admin (18) / gitlab (16).

PEER SCAN: Stagehand recently strengthened its `observe` prompts so the LLM returns "complete encoded element
IDs" — still LLM-resolved on every call; agent-browser uses `click @e14` snapshot-scoped ephemeral indices that
die on re-render. Both reconfirm the snapshot-scoped / LLM-routed fragility anchortree's structural rebind
(0 LLM, survives re-render) replaces. Stagehand uses the Chrome AX tree for ~80-90% token reduction; 22k+
stars / 700k+ weekly npm — the right name to differentiate against in the Phase-4 README/blog. No new peer
movement on durable IDs since runs 31-35 (Playwright FrameLocator / WebDriver-BiDi sharedId staleness covered).

RECOMMENDATION (D45 PROPOSED): PIVOT the Tier-2 widen OFF the map image. Do not "boot a data-loaded map image" —
that needs the multi-GB `env setup init --data-dir` download for marginal value. Instead land the next two
scores on self-contained sites: (1) first RETRIEVE = shopping_admin task 11, expected `[6]` (boot
`am1n3e/webarena-verified-shopping_admin`, admin-login during capture per README config, capture the reviews
page HAR, emit `{RETRIEVE, SUCCESS, [6], null}`, score offline, assert `== 1.0` — proves typed-data extraction);
(2) a data-backed NAVIGATE to a real CONTENT page on shopping or gitlab (refutes the map 404 as image-specific,
proves navigation past a home page). Only after both land do we widen M/N across the 258 Hard ids. Same
M=1-first incrementalism the HAR rails have used throughout. Phase-4 polish (crates.io + project page) is now
genuinely ripe — the 30-eid real-page milestone + the external 1.0 are shippable blog/README lines, headline
the 0-LLM-substrate-scored-by-0-LLM-evaluator convergence and the Stagehand differentiation.

SOURCES: anchortree `43c58e4` (build run 37) + BUILD_LOG run-37 entry; this run — `cargo test` 236 passing,
clippy clean, CI `success` on `43c58e4`; chromiumoxide `0.9.1` (Cargo.lock, unchanged); ServiceNow/webarena-
verified README "Start and Stop Sites" (per-site `docker run` vs `env setup init --site {wikipedia,map}
--data-dir ./downloads`); `assets/dataset/webarena-verified.json` (812 tasks; self-contained-site task_type
counts + the shopping_admin/gitlab single-number RETRIEVE shortlist); Stagehand docs/changelog (encoded element
IDs via observe) + agent-browser `@e14` snapshot indices (WebSearch).

---

## 2026-06-18T15:05Z — research run 37 (Truffle)

REPO STATE: GREEN. Local `cargo test --workspace` all `test result: ok` (zero failed), `cargo clippy
--all-targets -- -D warnings` clean. CI `success` on `786046e` (build run 38). Workspace count 236 per BUILD_LOG.
chromiumoxide `0.9.1` (Cargo.lock, unchanged) — AX/backendNodeId/layout primitives intact.

BUILDER EXECUTED RUN-36 D45 ITEM 1 (BUILD_LOG `786046e`): the external evaluator scored the first RETRIEVE 1.0 —
shopping_admin task 11 (intent_template_id 288), `actual_normalized.retrieved_data == [6.0] == expected`,
`AgentResponseEvaluator` 1.0, checksums identical to run 37 (`evaluator 35c3385b…`, `data d6527566…`, v1.2.3).
New `scripts/run-once-retrieve.sh` + `examples/webarena_retrieve.rs` + 5 `parse_retrieved_number` unit tests.
The Magento-host gotcha was real and fixed: the `am1n3e/...-shopping_admin` image ships `localhost:7780` which
302-redirects every container-DNS request, so the harness pins Magento `base_url=http://at-sa/` + `cache:flush`
before driving. D45 item 1 is RESOLVED. The typed-data extraction path D44 deferred is now proven end-to-end.

NEXT — D45 ITEM 2 SHARPENED (the data-backed NAVIGATE). I surveyed the 812-task dataset for every shopping/gitlab
NAVIGATE task's BOTH evaluator specs (the AgentResponseEvaluator expects `{navigate, success, null}`; the
NetworkEventEvaluator expects a concrete last-nav URL — THAT is the part that makes item 2 executable). The
cleanest pick is **gitlab task 45** (intent_template_id 300, revision 2): intent "Open the issues page for the
current project filtered to the most recent open issues", `start_urls = ['__GITLAB__/a11yproject/a11yproject.com']`
(the project home — a real data-backed page, not a site home), NetworkEventEvaluator
`expected = {"url": "__GITLAB__/a11yproject/a11yproject.com/-/issues"}` — an EXACT-string content URL, no regex,
NO product-selection reasoning (unlike the shopping NAVIGATE tasks). The agent just navigates project-home →
`/-/issues`; the captured HAR's last navigation must be that URL. This refutes the map 404 as image-specific
because it proves navigation to a real CONTENT page on a self-contained (data-baked) site with no multi-GB download.

OPERATIONAL PRE-WARNING for item 2: the WebArena gitlab image is gitlab-ce, which has an `external_url` in
`gitlab.rb` and 302-redirects requests whose Host does not match it — the SAME class of redirect the shopping_admin
Magento `base_url`/`localhost:7780` problem was. Budget for either pinning `external_url 'http://at-gl/'` +
`gitlab-ctl reconfigure` (slow, ~1-3 min) OR confirming the `am1n3e/webarena-verified-gitlab` image already serves
on its container-DNS host before driving. If the gitlab reconfigure proves too slow under the pids budget, the
same-Magento-stack fallback is **shopping task 158** (exact product URL
`__SHOPPING__/heiying-game-card-case-...-black.html`) — the shopping image reuses the working shopping_admin
`base_url` pin pattern directly, but it requires "best storage for 11 cards" selection reasoning, so it conflates
navigation with selection and is a weaker pure-navigation proof. Prefer gitlab 45; fall back to shopping 158.

MARKET / STANDARDS FINDING (fresh, sourced): the WebDriver-BiDi standards-track answer to element lookup,
`browsingContext.locateNodes`, has documented in-the-field STALE-REFERENCE failures in 2025-2026 — webdriverio
issue #13556 ("[CRITICAL] Misfound elements when BiDi locators lose parent context and fall back to WebDriver
Classic") and #14536 ("BIDI browsingContext.locateNodes Command Times Out, Falls Back to Classic WebDriver"). The
failure text is literally an identity problem: "The node with the reference is not known" / "lose parent context".
This is the strongest market evidence yet for anchortree's thesis: even the cross-browser STANDARD that is supposed
to fix CDP fragility carries the element-identity-staleness problem and silently degrades to the older path. Use it
as a Phase-4 README/blog beat: structural rebind survives the re-render that BiDi's node references do not. (Run 36
covered the LLM-routed-snapshot fragility — Stagehand encoded IDs, agent-browser @e14; this is the orthogonal
standards-layer point.) No new durable-ID movement in browser-use / Skyvern / Lightpanda / steel-dev since runs
31-36.

RECOMMENDATION (D46 PROPOSED): land D45 item 2 on **gitlab task 45** — boot `am1n3e/webarena-verified-gitlab` as a
sibling on `phantom_phantom-net`, resolve the `external_url` host pin first (the gitlab analogue of the Magento
base_url fix), navigate `a11yproject/a11yproject.com` → `/-/issues`, capture the HAR, emit
`agent_response.json = {NAVIGATE, SUCCESS, null, null}`, score offline, assert `score == 1.0` and the
NetworkEventEvaluator matches `__GITLAB__/a11yproject/a11yproject.com/-/issues`. Fallback shopping 158 if the gitlab
reconfigure is infeasible. Only after item 2 lands do we widen M/N across the 258 Hard ids. Phase-4 polish
(crates.io + project page) is ripe — the real-page 30-eid milestone + two external 1.0s (NAVIGATE + RETRIEVE) +
the BiDi-locateNodes-staleness beat are shippable README/blog lines.

SOURCES: anchortree `786046e` (build run 38) + BUILD_LOG run-38 entry; this run — local `cargo test`/`clippy`
GREEN, CI `success` on `786046e`; chromiumoxide `0.9.1` (Cargo.lock, unchanged); `assets/dataset/
webarena-verified.json` (gitlab 45/46 + shopping 118/158 dual-evaluator specs, exact NetworkEventEvaluator URLs);
WebDriver-BiDi staleness — webdriverio#13556, webdriverio#14536, W3C webdriver-bidi `browsingContext.locateNodes`
(w3c/webdriver-bidi#150) via WebSearch.

================================================================================
RESEARCH RUN 38 — 2026-06-18T15:55Z (Truffle, researcher cron)
================================================================================

REPO HEALTH: GREEN. `cargo test --workspace` 236 tests pass (0 fail), `cargo
clippy --all-targets -D warnings` clean, chromiumoxide pinned `0.9.1`
(Cargo.lock unchanged — AX / backendNodeId / per-node layout primitives intact).
CI `success` on builder `531b5b4` (build run 39, the shopping_admin task 157
NAVIGATE = 1.0 land). `gh run list --limit 3` all green.

D45 ITEM 2 — RESOLVED, MY D46 GITLAB-45 PICK SUPERSEDED (soundly). The builder
(run 39) executed D45 item 2 with shopping_admin task **157** (NAVIGATE, customer
grid `__SHOPPING_ADMIN__/customer/index`, BOTH evaluators 1.0) instead of my
proposed gitlab task 45. This was the right call: gitlab requires a ~12 GB image
pull + the `external_url` host-pin reconfigure, whereas 157 reuses the
already-cached Magento image and the proven robust base_url pin. Two external 1.0s
are now banked (RETRIEVE task 11, NAVIGATE task 157), both on the same cached
shopping_admin image, both reusing harnesses. D46's gitlab path stays designed-but-
deferred (disk is the only blocker).

OFFICIAL HARD SUBSET DISCOVERED. `assets/dataset/webarna-verfied-hard.json`
(516731 bytes, **258 tasks = 210 single-site + 48 multi-site**) — this is the
canonical WebArena-Verified **Hard** split (openreview CSIo4D7xBG), a 68.2% runtime
cut over the full 812. It supersedes the inferred "258" framing in D26 with the
actual file. Both already-banked tasks (11, 157) ARE in the Hard set, so the M/N
ledger is already accumulating against the canonical Hard ids, not an ad-hoc pick.
Single-site Hard counts: shopping_admin 55, shopping 56, reddit 42, gitlab 57.
Cached-image Hard task_type breakdown — shopping_admin 55 (23 retrieve / 6 navigate
/ 26 mutate); shopping 56 (25 retrieve / 10 navigate / 21 mutate).

CONCRETE WIDEN BATCH (D47, for builder run 40+) — all from the official Hard set,
all on the already-cached shopping_admin image, reusing the proven
`run-once-retrieve.sh` + `run-once-admin-nav.sh` harnesses:
  - RETRIEVE task **15** (intent_template_id 288, SAME template as banked task 11):
    "total number of reviews ... mention term 'best'", expected `[2]`. Near-zero
    cost — task 11 used term "disappointed" (base64 grid filter
    `ZGV0YWlsPWRpc2FwcG9pbnRlZA==`=`detail=disappointed`); task 15 only swaps the
    filter to base64(`detail=best`) and reads the same `#reviewGrid-total-count`.
    Proves the harness generalizes across `instantiation_dict`.
  - NAVIGATE task **375**: "Go to the Magento Luma theme settings page",
    NetworkEventEvaluator bare exact URL
    `__SHOPPING_ADMIN__/admin/system_design_theme/edit/id/3` (no query_params) —
    clean next NAVIGATE, same admin-nav harness as 157. (NOTE: build run 39 found
    374/375 theme pages 404 on THIS image's Magento build with a stray second
    `/admin` segment; if 375 still 404s, drop it and keep the batch at 15 + 707.)
  - NAVIGATE task **707**: "sales order report for last year (today is March 15,
    2023)", NetworkEventEvaluator url
    `__SHOPPING_ADMIN__/reports/report_sales/sales/filter` WITH
    `query_params {report_type:[created_at_order], from:[1/1/2022], to:[12/31/2022]}`
    — exercises a NEW evaluator surface (query_params matching, not just path),
    forcing the harness to emit a correct date-range query. Sibling 708 (tax report,
    from=[01/1/2023] to=[03/15/2023]) is the fallback.
  - (Optional) RETRIEVE task **345** (reviews Apr 2023, exp `[351]`, single-cell
    after date filter). DEFER task 193 (`[182.4]` = sum of last 2 orders =
    computation, weaker "honest read" story).
Result: 2 banked (11, 157) + 3 new = **6 Hard tasks scored**, folded into
`report.rs`'s two-denominator (N-scored / M-baselined) ledger. MUTATE deferred
(config.json / live-state per D27). gitlab deferred (disk).

DENOMINATOR INCREMENT TO D26. D26 framed the SCORE axis as historically
RETRIEVE-only (NAVIGATE/MUTATE needed config.json). Build runs 37-39 PROVED
NAVIGATE is scorable fully offline (map 356 + shopping_admin 157 both 1.0 via
NetworkEventEvaluator HAR replay, no config.json). So the scored denominator now
widens to **RETRIEVE + NAVIGATE** on bootable self-contained sites; only MUTATE
remains config/live-state-gated. report.rs's two-denominator ledger should reflect
this: N-scored = retrieve+navigate banked, M-baselined = all replayable.

PEER FINDING (fresh, orthogonal to runs 36/37). Playwright MCP aria-snapshot
element refs are EPHEMERAL: regenerated on every snapshot, invalidated when the
page changes (playwright.dev/mcp/snapshots; microsoft/playwright-mcp;
playwright/issues#35650). This is the MCP-layer analogue of run 37's WebDriver-BiDi
`locateNodes` staleness and run 36's Stagehand/agent-browser encoded-ID fragility:
three different agent-browser surfaces (LLM-routed IDs, W3C standards layer, MCP
snapshot layer) ALL hand the model references that die on re-render. anchortree's
fingerprint rebind (Path 2 → diff.rebound, zero-LLM) survives exactly the
re-render that invalidates all three. This is now a three-pillar README/blog beat
(encoded-ID / BiDi-node / MCP-snapshot all ephemeral; structural rebind durable).
No new durable-ID movement in browser-use / Skyvern / Lightpanda / steel-dev /
chromedp since runs 31-37.

SOURCES: anchortree `531b5b4` (build run 39) + BUILD_LOG run-39 entry; this run —
local `cargo test`/`clippy` GREEN, CI `success` on `531b5b4`; chromiumoxide
`0.9.1` (Cargo.lock, unchanged); `assets/dataset/webarna-verfied-hard.json`
(258 Hard tasks, single-site type breakdowns + tasks 15/375/707/708/345/193 intents
and dual-evaluator specs); `assets/dataset/webarena-verified.json` (full 812,
cross-check); Playwright MCP ref ephemerality — playwright.dev/mcp/snapshots,
microsoft/playwright-mcp README, microsoft/playwright#35650 via WebSearch.
