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
