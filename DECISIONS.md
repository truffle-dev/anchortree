# DECISIONS — choices and the reasoning behind them

> Append-only. Never rewrite an entry. If a decision is reversed, add a new
> entry that supersedes the old one and say so. Check here before
> re-litigating.

## D1 — anchortree is a library over CDP, not a browser or a fleet (2026-06-16)

We do not build or host browsers. Browserbase/Kernel run Firecracker microVMs;
Steel/Hyperbrowser run containers. That is an infra business. Our differentiator
is the *interface*: durable element identity + diff observations over **any**
CDP endpoint (local Chrome, Lightpanda, Browserbase, Cloudflare Browser Run).
This keeps the core small, testable, and useful to anyone regardless of where
their browser runs.

## D2 — the problem is identity, not rendering (2026-06-16)

Existing agent-browser tooling re-grounds every turn (screenshot, re-run
selectors). The expensive, non-deterministic part is *re-finding* the element
the agent already knew about after the agent's own click or a framework
re-render swapped the DOM node. We solve that once, durably, and everything
else (diffs, action space) follows.

## D3 — Rust, edition 2024 (2026-06-16) — operator override

The design doc (`docs/DESIGN.md`) originally recommended TypeScript for
ecosystem reach. Operator directed "build it in a cutting-edge language."
Chosen: Rust. Reasoning: single static binary any agent can just run; mature
CDP via `chromiumoxide`; matches the Lightpanda/agent-browser performance
ethos; the durable-identity core is pure logic that benefits from Rust's
exhaustive enums and ownership. The TS recommendation in DESIGN.md is retained
as historical context but is **superseded by this decision**.

## D4 — durable-identity core FIRST, browser plumbing second (2026-06-16)

The identity engine is the differentiator and is pure logic, so it is fully
unit-testable without driving Chrome. Building it first gives us a green,
proven core before we take on the messier CDP integration. The core crate
(`anchortree-core`) is deliberately browser-free; it operates on `ObservedNode`
values that a later `anchortree-cdp` crate will produce.

## D5 — `backendNodeId` is the primary key; fingerprint is the rebind (2026-06-16)

CDP `backendNodeId` is document-lifetime-stable, so it is the cheap primary key
while a DOM node lives. When a node is destroyed and recreated (hard
re-render), we rebind the logical `eid` to the new node by scoring the old
fingerprint against candidates. The rebind ladder, strongest rung first:
stable attribute (id/name/data-testid/aria-label) → (role, accessible-name) →
structural path → geometry. Threshold `REBIND_THRESHOLD = 0.6`; below it we
mint a fresh identity rather than risk a wrong rebind. A role mismatch or two
disagreeing stable attributes hard-veto a match.

## D6 — eids are human-and-agent readable (2026-06-16)

Minted ids carry a role prefix and a slug of the accessible name, e.g.
`btn-sign-in`, `inp-email`. An agent reading the id infers the action space
without a second lookup. Collisions disambiguate with a numeric suffix
(`btn-edit`, `btn-edit-1`). Slugs truncate to 24 chars then trim trailing
separators so an id never ends in a dash.

## D7 — coordination via git docs (primary) + transcripts (secondary) (2026-06-16)

Two crons (hourly builder, 45-min researcher) hand off through structured
markdown committed to git: `STATE`, `DECISIONS`, `ROADMAP`, `BUILD_LOG`,
`RESEARCH_LOG`, `HANDOFF`, `LOCK`. These are authoritative. Raw session
transcripts on the volume are a secondary deep-context source, pointed to from
`STATE.md`. Rationale: an agent cannot reliably introspect its own live session
id, and structured docs are smaller and intentional, whereas transcripts are
large and noisy.

## D8 — `anchortree-cdp` supports `ws://` only; `wss://` deferred (2026-06-17)

The `anchortree-cdp` crate depends on `chromiumoxide` with **default features
only** — no `rustls`, no `native-tls`. Reasoning: those features exist to
configure the browser *fetcher* (`chromiumoxide_fetcher`, which downloads a
Chrome build), not the CDP WebSocket transport. The WS transport rides
`async-tungstenite` with only its `tokio-runtime` feature, which has no TLS, so
it speaks plain `ws://` regardless. Enabling `rustls` pulls `aws-lc-sys` and
`native-tls` pulls system OpenSSL — both need a C/cmake toolchain the phantom
container does not have by default. Since the durable-identity value is entirely
in the (browser-free) fusion logic and a local headless Chrome exposes a plain
`ws://` `webSocketDebuggerUrl`, dropping TLS costs nothing for development and
keeps the build dependency-light.

Consequence: TLS CDP endpoints (`wss://`, e.g. Browserbase) are not reachable
yet. Lifting this means adding a TLS-capable WS stack once a C toolchain is
confirmed available (the `cc-userland` restore now puts `cc` at `~/.local/bin`;
whether that compiles `aws-lc`/OpenSSL-sys cleanly is an open question in
`STATE.md`). Superseded only by a future entry that adds `wss://`.

## D9 — `RawAxNode` is the transport-neutral fusion boundary (2026-06-17) — CONFIRMED (builder)

*Proposed by the research cron; confirmed by the builder during Phase 1.3.*
The 1.3 recorded-reply decode test is the first concrete consumer of this seam:
it loads a canned `getFullAXTree` JSON through `chromiumoxide`'s `AxNode`, decodes
into `RawAxNode`, and runs `fuse::fuse` unchanged — proving a non-live source can
drive the engine without any CDP types reaching fusion. Re-verified clean this
run (grep: `fuse.rs` and `anchortree-core` chromiumoxide refs = 0).

The fusion logic (`anchortree-cdp/src/fuse.rs`) imports **zero** chromiumoxide
and operates only on plain `RawAxNode` / `RawAxProperty` value structs; only the
thin `observer.rs` adapter touches CDP types. This run verified that boundary is
already clean (grep: `fuse.rs` chromiumoxide imports = 0). Decision: **keep it
that way deliberately.** `RawAxNode` is the single transport seam — any future
adapter (`anchortree-bidi`, a raw-WS fallback, a recorded-fixture loader for the
1.3 decode test) decodes its source into `RawAxNode` and reuses `fuse::fuse`
unchanged.

Why this matters now, not later: agent-browser transport is bifurcating. CDP
remains correct for every Chromium agent-browser we target today (Browserbase,
Lightpanda, Cloudflare Browser Run, Playwright-MCP), but WebDriver BiDi is the
rising cross-browser standard (Firefox dropped CDP by Cypress 15, Aug 2025;
Selenium/cloud-grid vendors are migrating — developer.chrome.com/blog/webdriver-bidi).
BiDi has **no durable element-identity primitive of its own** (realm-scoped
shared refs, invalidated on re-render), so the identity engine is the value on
*either* transport. Holding `RawAxNode` transport-neutral costs nothing today
and makes a BiDi adapter a drop-in rather than a rewrite. Constraint for the
builder: do not let CDP-shaped types leak past `observer.rs` into `fuse.rs` or
`anchortree-core`.

## D10 — live smoke goes over local `ws://` first; `wss://` lift uses rustls+ring (2026-06-17) — PROPOSED (research)

*Proposed by the research cron after empirically testing the toolchain; builder
confirm before treating as settled. Resolves the D8 "can cc-userland compile a
TLS WS stack?" open question with measured results.*

Empirical results (throwaway `/tmp` crate, repo untouched):
- The userland C toolchain **works for real C** once a session exports
  `LD_LIBRARY_PATH=~/.local/lib/x86_64-linux-gnu` (for `cc1`'s libisl/libmpc/
  libmpfr) and `C_INCLUDE_PATH=~/.local/include:~/.local/include/x86_64-linux-gnu`
  (for libc headers). Proof: `ring` 0.17 compiles clean in 3.82s. The
  `cc-userland` "cc ok" smoke is misleading — it sets those inline; a default
  session does not, so real C fails with `cc1: libisl.so.23` then `stdint.h: No
  such file or directory`.
- `cmake`, `nasm`, `make` are MISSING → `aws-lc-sys` and vendored `openssl` can
  not build. No libssl `-dev` headers → non-vendored openssl-sys also out.
- chromiumoxide 0.9.1 `rustls` feature resolves to **rustls 0.23 + aws-lc** (no
  ring); `native-tls` resolves to openssl-sys. **Both off-the-shelf TLS features
  are therefore blocked here.**

Decisions:
1. **Phase 1.5 splits.** 1.5a = first live smoke over a **local headless
   chromium `ws://`** endpoint (zero TLS), proving observe→re-render→rebind
   end-to-end. This is the critical path to an "alive" demo and must not wait on
   any TLS work. 1.5b = the `wss://`/Browserbase lift, deferred.
2. **When 1.5b is taken, lift D8 via rustls + the `ring` crypto provider**, not
   aws-lc-rs and not native-tls. ring is proven to compile on this box; aws-lc
   needs cmake+nasm we do not have. The work is feature surgery: force rustls
   onto ring and purge `aws-lc-rs` from `hyper-rustls` / `rustls-platform-
   verifier` defaults. Alternative (install cmake+nasm static binaries into
   `~/.local/bin`, like cc-userland) is recorded as a fallback only.
3. Supersedes the D8 open question. D8's `ws://`-only stance stands for now;
   this entry says *how* to lift it and *which path first*.

## D11 — local 1.5a CDP target is chromedp/headless-shell, connected by IP (2026-06-17) — proposed

Run 2 left 1.5a needing "a chromium binary somewhere." Run 3 tested the cheapest
option end-to-end and pins it so the builder doesn't re-fight Docker/Chrome.

Target: `docker run -d --name <chrome> --network phantom_phantom-net
chromedp/headless-shell:latest` with **no extra Chrome flags**. The image
entrypoint already runs `socat TCP4-LISTEN:9222,fork TCP4:127.0.0.1:9223` and
launches Chrome on 9223. Two gotchas, both verified by repro:
- **Do not pass `--remote-debugging-address/-port`.** They make Chrome also bind
  9222 → `bind() failed: Address already in use (98)`; Chrome falls back to
  `ws://[::1]:9222` and socat connection-refuses. Default entrypoint is correct.
- **Connect by container IP, not name.** `GET http://<name>:9222/json/version`
  trips Chrome's CDP host-header guard ("Host header ... is not an IP address or
  localhost"). The container IP clears it and the returned
  `webSocketDebuggerUrl` is IP-based, so the WS upgrade clears it too
  (confirmed `HTTP/1.1 101 WebSocket Protocol Handshake`). Alt: `-H "Host:
  localhost"` on the probe.

This is a **plain ws://** path: D8/D10 (TLS/ring) do **not** gate 1.5a. Lightpanda
was evaluated as an alternative target and rejected (no real Accessibility tree;
`LP.getSemanticTree`/`getInteractiveElements` are snapshot-only with no durable
handle) — it cannot feed our `getFullAxTree` fusion. headless-shell is the target
for the first live smoke; Lightpanda stays out until/unless it ships a full AX
tree.

## D12 — action dispatch: through backendNodeId, via the CDP Input domain (2026-06-17) — CONFIRMED

**Confirmed in builder run 5.** Implemented exactly as proposed in
`crates/anchortree-cdp/src/actions.rs` and proven live by
`examples/act_after_rerender.rs`: after a full `innerHTML` swap, `click`, `type`,
and `select` issued against the *post*-swap eids all land; the click reads back
`isTrusted: true` and the typed/selected values read back from the live DOM. Two
small realisations during wiring: (1) `type` uses `Input.insertText` only — no
per-keystroke `dispatchKeyEvent` is needed for the common "set this field's text"
case, so 2.1 ships insertText-based typing and leaves true keystroke emulation
(for key handlers / shortcuts) to a later action; (2) `getContentQuads` returns
each quad as 8 numbers, so the hittable point is the centroid of the four corners
(robust to rotation), not a box-model rect.

Phase 2.1 turned an `eid` into a real click/type. Two axes, both upheld:

**Resolution key = the IdentityMap's `backendNodeId`, not coordinates or
selectors.** This is the whole point of the project: we already hold a *durable*
eid→backendNodeId binding (rebound through re-renders, proven in 1.5a). Resolve
`eid → backendNodeId`, then `DOM.scrollIntoViewIfNeeded(backendNodeId)` and
`DOM.getContentQuads(backendNodeId)` for a fresh hittable point at action time
(content-quads handle inline/multi-line/rotated boxes that a single getBoxModel
rect misses). Prior art: browser-use's "super-selector" also keys on
`backend_node_id` (+ x/y + fallback selectors), but theirs is recomputed per
step so they *need* the fallback ladder; ours is durable, so the common
re-render case needs no fallback (browser-use.com/posts/playwright-to-cdp).

**Dispatch layer = CDP `Input` domain, not page-context `element.click()`.**
`Input.dispatchMouseEvent` / `dispatchKeyEvent` / `insertText` inject at the
browser input layer and are observed as trusted gestures; a click run via
`Runtime.callFunctionOn`→`element.click()` executes in page context and is
`isTrusted:false` (MDN Event.isTrusted). Trusted input is both more faithful to
a real user and less likely to be rejected by listener guards. So: click =
`dispatchMouseEvent` (pressed+released at quad center); type = `DOM.focus` then
`dispatchKeyEvent`/`insertText`. The **one** sanctioned page-context exception is
`select` on a native `<select>` (no clean trusted-gesture path): set value +
dispatch `input`/`change` via `callFunctionOn`.

All primitives verified present in `chromiumoxide_cdp` 0.9.1 (`ResolveNode`,
`DispatchMouseEvent`, `DispatchKeyEvent`, `InsertText`, `CallFunctionOn`,
`Focus`, `SetAttributeValue`, `ScrollIntoViewIfNeeded`, `GetContentQuads`,
`GetBoxModel`). No driver gap; no raw-WS fallback needed for 2.1. (Of these, 2.1
exercises `ResolveNode`, `DispatchMouseEvent`, `InsertText`, `CallFunctionOn`,
`Focus`, `ScrollIntoViewIfNeeded`, and `GetContentQuads`.)

## D13 — the 2.2 "set-of-marks" fallback is TEXTUAL, not the visual SoM screenshot (2026-06-17) — CONFIRMED

The ROADMAP item is named after "Set-of-Mark" prompting (Microsoft Research,
arXiv 2310.11441), which is a *visual* technique: numbered marks overlaid on a
**screenshot** fed to a **VLM**. We deliberately diverge from the visual form as
the default, because it contradicts our token-cheap thesis: a page is ~5,000
vision tokens vs ~500 accessibility-tree tokens (an order of magnitude), and a
screenshot loop runs ~$0.01/image over 10–30 images/task (research run 5
sources). The whole field is moving text-first (Playwright MCP reads the AX tree
as YAML; Playwright CLI hands agents compact `e15` refs and writes snapshots to
disk).

Decision (builder to confirm before wiring):
1. A "mark" is a **transient textual handle**, not an image overlay. It is
   emitted only for a node `fuse` kept (passed the observable filter) whose
   rebind ladder produced **no durable identity** — these are exactly the nodes
   the IdentityMap cannot give a stable `eid`.
2. Marks live in a **parallel `Vec<Mark>` on the Observation**, not a synthetic
   `Eid` variant — `Eid` keeps meaning "durable." `Mark { index, backend_node_id,
   role, label_snippet, geometry }`. `index` is positional and **recomputed every
   observation** (NOT stable across observations — that is the contract that
   distinguishes a mark from an eid).
3. Distinct namespace (e.g. `m12`) so a one-turn mark is never confused with a
   durable eid in logs or agent prompts.
4. `act` stays unchanged (D12). Add a thin `act_mark(obs, index, Action)` that
   resolves the mark to its carried `backend_node_id` and calls the same path.
   If the page re-rendered between observe and act, the captured backendNodeId is
   stale → surface `NotHittable`/`UnknownEid` so the agent re-observes. Marks are
   single-turn by design; this is correct behavior, not a bug.
5. The **visual / screenshot SoM** form is deferred to an optional **2.2b**
   escalation, feature-gated, reserved for the genuinely DOM-less case
   (canvas/WebGL/`<embed>` with no backendNodeId to mark). Text path stays the
   default; the heavy vision path is opt-in.

**CONFIRMED (builder run 6).** Wired exactly as proposed, with one design choice
the proposal left to the builder: the "no durable identity" test is made
**intrinsic**, not cross-node. `Fingerprint::is_durably_anchorable()` returns
true iff the node has a stable attribute OR a non-empty accessible name (a
structural path alone scores 0.3, below the 0.6 `REBIND_THRESHOLD`; geometry is
excluded because a re-render is free to move an element). This cleanly captures
the primary case — unlabeled icon buttons, generic clickables — without any
cross-node "duplicate role+name" analysis, and it preserves the existing
`duplicate_labels_disambiguate` behavior (those buttons have distinct structural
paths AND names, so they stay anchorable and earn eids). The cross-node ambiguity
case named loosely in the proposal ("duplicate role+name") is left as a
documented future refinement; the intrinsic empty-identity rule is the load-
bearing 80%. `IdentityMap::observe` now returns `Observation { diff, marks }`
(the existing three-path resolution body was extracted to a private `resolve`);
`act_mark(page, &obs, index, Action)` resolves the mark straight from the
observation (a mark was never bound, so it does NOT go through the map) and
funnels through a shared `act_on_backend` with `act`. Added `ActError::UnknownMark`
for an out-of-range/stale index. Proven live in `examples/act_on_mark.rs`.

## D14 — token-budget estimator is tokenizer-free, divisor chars/3.5 not chars/4 (2026-06-17) — CONFIRMED (builder run 7)

**Context.** Phase 2.3 wants a guardrail: a baseline `Observation` must stay
≤5,000 tokens and a per-`Diff` payload ≤800 tokens, so an agent can poll the
page every turn without blowing its context window. STATE's prior Next-action
said "chars/4 is fine and avoids a tokenizer dep."

**Decision (proposed).**
1. **No BPE tokenizer dependency.** Estimate token cost from the serialized
   string length with a fixed divisor. Justification that a fixed divisor is
   reliable for *this* payload: arXiv 2508.04412 ("Beyond Pixels: DOM
   Downsampling for LLM Web Agents") measures byte-size↔token-size correlation
   **r = 0.9994** for DOM content. chars/N is also established tooling practice —
   LangChain's `count_tokens_approximately` defaults `chars_per_token = 4.0`.
2. **Divisor is 3.5, not 4.** The chars/4 rule is calibrated to English *prose*.
   Our payload (AX-tree / YAML / `role`/attribute markup / short refs) is
   markup-dense, where empirical ratios run **2.5–3.8 chars/token** (BPE merges
   English words but fragments brackets/attribute-names/indentation into many
   short tokens). chars/4 therefore *under*-counts an AX payload, and a guardrail
   must fail safe by *over*-estimating. chars/3.5 sits conservatively inside the
   measured band. (chars/3 is the harder-margin option if real payloads prove
   even denser; revisit if the measuring test shows headroom is illusory.)
3. **Integer-math form:** `estimated_tokens(s) = (s.chars().count() * 2).div_ceil(7)`
   (= ceil(chars / 3.5)). Pure, deterministic, browser-free — lives in a new
   `budget` module in `anchortree-core` next to `diff`/`observation`.
4. **Caps unchanged: 5,000 baseline / 800 per-diff.** Sane vs peers — compact AX
   snapshots land ~200–1,000 tokens (Playwright-MCP ~200–400; Stagehand 80–90%
   smaller than raw DOM), so 5K is roomy yet well below the 15K–35K of an
   uncompressed full AX dump and the 25K–200K context-window failures peers
   actually hit (Skyvern#1712, playwright-mcp#1216). 800/diff is tight enough to
   keep every-turn polling cheap.

**Test shape.** A measuring test in `anchortree-core` builds a realistic ~40-node
observation, serializes it, asserts baseline `estimated_tokens ≤ 5000`, and a
representative single-node `Diff` asserts `estimated_tokens ≤ 800`. Builder
confirms or refines the divisor after seeing the real numbers.

Sources: arxiv.org/html/2508.04412v1; reference.langchain.com
(`count_tokens_approximately`); developers.openai.com/api/docs/concepts;
community.openai.com/t/…/622947; browserbase.com/blog/ai-web-agent-sdk;
github.com/microsoft/playwright-mcp/issues/1216; github.com/Skyvern-AI/skyvern/issues/1712.

**CONFIRMED (builder run 7, 2026-06-17).** Shipped as the `budget` module in
`anchortree-core`: `estimated_tokens(s) = (s.chars().count() * 2).div_ceil(7)`,
caps `BASELINE_BUDGET = 5_000` / `DIFF_BUDGET = 800`, plus
`{observation,diff}_tokens` and `{observation,diff}_within_budget`. The estimator
counts Unicode scalars, not bytes, so a 4-byte emoji label costs one token, not
four (a byte-length estimate would make the guardrail jump on non-ASCII names).

To measure honestly the module needed a *serialized form* to count, so this run
also added the agent-facing render: `Diff::render` (line-oriented, sigils
`+`/`-`/`*`/`~` for added/removed/rebound/changed, deterministic section order)
and `Observation::render` (the diff plus one `m{i} {role} "{snippet}" @x,y` line
per transient mark). The render is deliberately lean — an eid like `btn-sign-in`
already carries role and name, so the inventory needs no second column; richer
state stays queryable on demand via `IdentityMap::binding`.

The measuring test settled the divisor question with real numbers: a realistic
**40-element** baseline observation (nav rail + header + project form + a table
with duplicate-disambiguated row actions + status/headings + footer) plus two
unanchorable icon marks renders to **200 estimated tokens** — an order of
magnitude under the 5K cap and squarely in the ~200–400 band of peers' *compact*
snapshots, while a raw AX dump of the same page would be 15K–35K. A steady-turn
diff (two status lines tick, one button rebinds, one toast appears) is **28
tokens**. The divisor stays at 3.5: at these margins chars/3 buys no safety the
headroom does not already provide, and 3.5 keeps the over-estimate honest rather
than alarmist. The `< 600` baseline / `< 100` steady-turn assertions in the test
are tripwires — if a future render grows chatty enough to cross them, that is the
signal to investigate before touching the cap.

## D15 — positioning thesis + README contract; CDP today, BiDi-compatible by design (2026-06-17) — CONFIRMED (builder run 8)

**Context.** Phase 2.4 writes the first README — the artifact a human or agent
reads to decide whether to adopt anchortree. Research run 7 surveyed the five
peer READMEs (browser-use, Stagehand, Skyvern, Playwright-MCP, steel-dev) and
verified the competitive gap against primary sources. This decision pins the
positioning so the README, the Phase 3 benchmark, and the Phase 4 blog all
inherit one consistent frame instead of re-deriving it.

**Decision (proposed).**
1. **The gap is primary-source confirmed on BOTH axes, and unoccupied.**
   - Durable cross-render identity: Playwright MCP docs state verbatim *"refs are
     invalidated when the page changes"* and *"re-snapshot after navigation"*
     (playwright.dev/mcp/snapshots); Playwright **declined** to persist element
     identity for performance (microsoft/playwright-mcp#1488, NOT_PLANNED, Gozman:
     "Playwright does not store any prebuilt locators … precisely because it's not
     free in terms of performance"). Stagehand's `EncodedId` is
     `frameOrdinal-backendNodeId` (snapshot-scoped, source-confirmed in
     `lib/v3/types/private/internal.ts`) and re-grounds via an LLM `observe` call.
     browser-use uses per-snapshot integer indices that shift on re-render
     (browser-use#1686).
   - Diff / only-what-changed observations: targeted `gh search` across stagehand,
     browser-use, playwright-mcp found **zero** diff-observation features; the
     peer norm is the opposite (re-snapshot the whole a11y tree each step).
   Both of anchortree's wedges are open as of 2026-06-17.
2. **Frame the saving on TWO axes, not one.** Managed browsers bill per
   session-minute (Browserbase: Developer $20/mo = 100 hrs, Startup $99/mo =
   500 hrs; browserbase.com/pricing). A no-LLM rebind + diff observation cuts
   **both** LLM tokens **and** billable browser-minutes (fewer round-trips, no
   re-grounding inference). State both.
3. **README contract: the hello-world must DEMONSTRATE the rebind.** No peer's
   hero example does this. The canonical snippet is: act on a stable id
   (`btn-sign-in`) → force a re-render → act on the *same* id again, no
   re-observe-for-grounding. That single snippet is the entire differentiation;
   lift it from `examples/act_after_rerender.rs` so it cannot drift from
   compiling code. Lead with the one-sentence identity thesis (4 of 5 peers are
   thesis-first); runnable example within the first screenful (browser-use shape);
   a prose "vs" section framed on token+minute cost (Playwright-MCP shape);
   one-line CDP connect (every peer hides the wiring).
4. **CDP today, BiDi-compatible by design.** Playwright is investing heavily in
   WebDriver-BiDi in June 2026 (microsoft/playwright `main`: prototype-pollution
   fix in BiDi deserialization `722b776` 2026-06-16, MCP moz-firefox BiDi channel
   `123cc42` 2026-06-08, plus a month of Firefox/BiDi test work). BiDi is
   maturing as the cross-browser transport but is not displacing CDP for Chromium
   agent work today. anchortree's CDP-only stance is correct now; the
   `ObservationSource` trait already keeps `anchortree-core` transport-neutral
   (D9), so a future `anchortree-bidi` adapter is a clean axis. Say "CDP today,
   BiDi-compatible by design" in the README rather than being silent — it is the
   one axis a peer could later differentiate on.

**Builder note.** This is positioning, not architecture — no code shape changes.
Lift the named primary sources into the README's "vs the field" section so the
claim is verifiable, not hand-waved. Confirm/refine when the README lands.

Sources (accessed 2026-06-17): playwright.dev/mcp/snapshots;
github.com/microsoft/playwright-mcp/issues/1488; github.com/browserbase/stagehand
(`lib/v3/types/private/internal.ts`, releases 2.5.9 / 3.5.0);
github.com/browser-use/browser-use/issues/1686; github.com/microsoft/playwright
commits/main (BiDi stream, June 2026); browserbase.com/pricing.

**CONFIRMED (builder run 8, 2026-06-17).** Shipped the README exactly to this
contract. Five parts in order: (1) the one-sentence identity thesis as the very
first line; (2) a runnable Quickstart whose hero block is the act → re-render →
act-on-the-same-id rebind, lifted from `examples/act_after_rerender.rs` so it
cannot drift, with the one-line `connect(ws_url)` and an `obs.render()` /
`budget::observation_within_budget` token-cost callout in-band; (3) "How it
works" as the three numbered advantages (durable identity / diff observations /
any CDP browser); (4) an "anchortree vs the field" prose section naming the
three peers with their primary sources, framed on the two-axis token+
browser-minute cost; (5) the "CDP today, BiDi-compatible by design" note tied to
the `ObservationSource` boundary. One refinement vs the proposal: dropped
"geometry" from the fingerprint-rung list in "How it works" to match the
shipped ladder (stable attr → role+name → landmark-scoped structural path); the
old genesis README still listed geometry as a rung. No code changed; tree stayed
green at 62 tests.

## D16 — Phase 3.3 benchmark: WebArena substrate, LLM-calls-saved headline, dual real-peer baseline (2026-06-17) — PROPOSED (builder confirms when 3.3 lands)

The exit-condition check (does durable-identity rebind measurably beat naive
re-grounding?) needs a benchmark whose substrate actually exercises the thing we
claim. The decision has three parts.

**Substrate: self-hosted WebArena, not a live-web suite.** WebArena ships as
deterministic Docker apps (a forum, a CMS, a shopping site, GitLab) with
scripted, reproducible state — driven through BrowserGym/AgentLab. We need
determinism (the same task must re-run identically to A/B the two identity
strategies) AND real client-side re-renders (so the rebind is exercised, not
bypassed). Reject the live-web suites — WebVoyager and WebBench run against the
production internet, so they are non-deterministic and cannot isolate a
re-grounding delta. Reject Mind2Web — its tasks are static DOM snapshots, which
cannot exercise a live cross-render rebind at all. WebArena is the only widely
cited substrate that is both deterministic and live-rendering.

**Headline metric: LLM re-grounding calls eliminated per re-render (0 vs 1),**
supported by "% of per-turn token budget cut." This is the metric no prior art
isolates — peers fold re-identification cost into end-to-end task success, which
confounds it with model reasoning quality. anchortree's claim is narrow and
measurable: after a DOM swap, we rebind the same logical id with **zero** model
calls where the snapshot-scoped peers spend **one** (Stagehand's `observe`
re-ground). Count those calls directly.

**Baseline: two real peers, one per axis — not a strawman.**
- Playwright-MCP for the **token-volume** axis: it re-snapshots the whole a11y
  tree each step and invalidates refs on page change, so it measures the
  full-tree-resend cost our diff observations avoid.
- Stagehand v3 for the **LLM-call** axis: its act-cache re-grounds via an LLM
  call on any structural change, so it measures the inference-per-re-render cost
  our durable rebind avoids.
  A single baseline would let a reader attribute the win to the wrong axis;
  pairing them isolates each saving cleanly.

**Why this is the right shape:** the benchmark must measure
*re-identification-after-re-render* in isolation, with confounds (model choice,
task-success rate, network) held constant by the deterministic substrate. This
is bigger than one builder run — scope it as its own arc (its own branch, its
own log), with the harness, the two baselines, and the metric collection as
separable deliverables. The output feeds both the week-3 exit-condition check
and the Phase 4.3 blog headline.

Sources (accessed 2026-06-17): webarena.dev + github.com/web-arena-x/webarena
(deterministic self-hosted Docker task environments); github.com/ServiceNow/
BrowserGym + github.com/ServiceNow/AgentLab (WebArena driver/harness);
WebVoyager (arXiv 2401.13919) and Mind2Web (arXiv 2306.06070) evaluated and
rejected as substrates for the reasons above; playwright.dev/mcp/snapshots
(per-step re-snapshot + ref invalidation); github.com/browserbase/stagehand
(LLM re-ground on structural change).

## D17 — Phase 3.3 substrate = WebArena-Verified (not WebArena-via-BrowserGym); Phase 3.1 target = Cloudflare Browser Run CDP (2026-06-17) — PROPOSED (builder confirms when 3.1/3.3 land); refines D16

Two primary-source findings from research run 9 sharpen the Phase 3 plan.

**3.3 substrate: switch from WebArena-via-BrowserGym to WebArena-Verified.** D16
named "WebArena (deterministic Docker apps via BrowserGym/AgentLab)." BrowserGym/
AgentLab are Python and would force either a Python shim around our Rust client or
a re-implementation of the task driver. WebArena-Verified removes that coupling:
its docs state the agent "can use any programming language ... no dependency on
the benchmark's libraries." The contract is file-based — the agent reads a JSON
task (`intent`, `start_urls`, `task_id`), drives the browser itself, and emits a
JSON response + a HAR network trace; the `ghcr.io/servicenow/webarena-verified`
Docker image scores it via `AgentResponseEvaluator` (type-aware normalization, no
LLM judge) + `NetworkEventEvaluator` (HAR-trace analysis, no DOM selectors). So the
benchmark harness is **pure Rust**: anchortree drives the WebArena-Verified Docker
sites over CDP, writes JSON+HAR, the verified image scores. Bonus: the
deterministic evaluator removes the LLM-judge confound from D16's
LLM-calls-saved headline (the only LLM calls in the loop are the agent's own
re-grounding calls, which is exactly what we are counting). D16's headline metric
(LLM re-grounding calls eliminated per re-render, 0 vs 1) and dual real-peer
baseline (Playwright-MCP token-volume axis + Stagehand v3 LLM-call axis) carry
over unchanged.

**3.1 target: Cloudflare Browser Run is a managed plain-CDP `wss://` endpoint —
question resolved.** D1 said we host no browsers and connect to any CDP endpoint;
the open 3.1 question was "Browser Run (managed) vs Container (own Lightpanda
image)." As of the 2026-04-10 GA, Browser Run exposes the full Chrome DevTools
Protocol over a WebSocket:
`wss://api.cloudflare.com/client/v4/accounts/${ACCOUNT_ID}/browser-rendering/devtools/browser`
(optional `keep_alive`), authed by a custom API token with **Browser Rendering -
Edit** permission, accepting raw CDP commands (not a Puppeteer-only wrapper). That
makes Browser Run the obvious managed target — no container to build. The single
prerequisite is the `wss://` TLS lift: chromiumoxide's rustls path forced onto the
`ring` provider (D10; ring compiles in this toolchain, aws-lc does not). That makes
**1.5b the shared unlock for Cloudflare (3.1) AND Browserbase** — it climbs above
3.1 in priority, because 3.1 is a one-line `connect()` retarget once 1.5b lands.

Builder confirms each half when the respective phase lands.

Sources (accessed 2026-06-17): developers.cloudflare.com/browser-run/cdp/;
developers.cloudflare.com/changelog/post/2026-04-10-browser-rendering-cdp-endpoint/;
blog.cloudflare.com/browser-run-for-ai-agents/;
servicenow.github.io/webarena-verified/dev/; github.com/ServiceNow/webarena-verified.

## D18 — Phase 3.1 connect model: REST-acquire-session → header-less `wss://` connect with the credential in the URL query string (2026-06-17) — CONFIRMED for the acquire leg (builder run 11); connect leg superseded by D19

Tracing the actual Cloudflare Browser Run / Browserbase connection mechanics
against the chromiumoxide 0.9.1 source settles how the Phase 3.1 example must be
shaped, and rules out a tempting wrong turn.

**Two hard constraints from chromiumoxide 0.9.1 (read from the crate):**
- `Connection::connect` (`src/conn.rs:36`) hands the WS URL straight to
  `async_tungstenite::tokio::connect_async_with_config` with no header argument.
  There is **no hook to set an `Authorization` header on the WebSocket
  handshake.** Header-based auth is structurally impossible through
  `Browser::connect`.
- `Browser::connect_with_config` (`src/browser/mod.rs:87`) performs the
  `/json/version` HTTP discovery **only when the URL starts with `http`**. A
  `wss://` URL bypasses discovery and connects directly — so we never hit a
  `/json/version` probe against a hosted gateway that would not answer it.

**Both hosted targets fit one model.** Cloudflare Browser Run mints a session
over HTTP (`POST /devtools/browser` with `Authorization: Bearer`, then
`GET .../{session_id}/json/list`, `DELETE .../{session_id}`); Browserbase's
create-session returns a `connectUrl` of the form
`wss://connect.browserbase.com/v1/sessions/<id>?apiKey=<key>`. In both cases the
credential travels in the **URL** (query string / session-scoped path), and the
subsequent WebSocket upgrade carries no auth header. This is the only model that
works given the chromiumoxide constraint above, and it is the model anchortree's
existing `connect(wss_url)` already supports (D17's 1.5b made the WS leg
TLS-capable).

**Decision.** The Phase 3.1 example adds exactly one new piece: a thin
per-provider **session-acquire HTTP helper** (reqwest, already transitively in
the tree via chromiumoxide; `POST`/`GET` with the Bearer/apiKey header) that
returns the self-authenticating `wss://` URL, which is then passed to the
existing `connect()` — header-less, `wss://` direct. **Do NOT** try to inject an
auth header into the WS handshake; chromiumoxide gives no hook and it is
unnecessary. Keep the helper out of `anchortree-core` (it is provider plumbing,
not identity logic); it belongs in `anchortree-cdp` or the example. The shipped
`observe_wss` example already proves the connect leg from an out-of-band
`ANCHORTREE_WSS_URL`; 3.1's increment is the acquire helper so the example mints
the URL itself. Builder confirms when 3.1 lands.

Sources (accessed 2026-06-17): chromiumoxide 0.9.1 (`src/conn.rs:36`,
`src/browser/mod.rs:80-130`); developers.cloudflare.com/browser-run/cdp/;
docs.browserbase.com/reference/api/create-a-session; github.com/miantiao-me/
cf-browser-cdp; stagehand#1381; vercel-labs/agent-browser#169.

## D19 — Phase 3.1 splits: the acquire leg ships; the hosted *connect* leg is blocked by chromiumoxide 0.9.1 and is the next increment (2026-06-17) — CONFIRMED (builder run 11)

D18 assumed the acquire helper was Phase 3.1's only new piece because
`observe_wss` "already proves the connect leg." Building 3.1 against a **real**
Browserbase session showed that assumption was half right. The acquire half is
exactly as D18 described and now ships live-verified. The connect half is a
separate, real problem: chromiumoxide 0.9.1 cannot cleanly attach to the page a
hosted browser **already has open**, and a hosted browser does not let us create
our own.

**What ships (the acquire leg, live-verified).** `gateway.rs`:
`cloudflare::devtools_ws_url(account, token)` builds the Browser Run `?token=`
URL with no round-trip; `browserbase::acquire(project, key)` mints a session over
REST and returns its `connectUrl`. `observe_hosted` ran against real Browserbase
credentials and minted live sessions every invocation, returning
`wss://connect.<region>.browserbase.com/?signingKey=…` plus a replay link
(empirical note: Browserbase's current `connectUrl` carries the credential as
`signingKey`, not the `apiKey` query param the older docs showed — the helper is
agnostic, it returns whatever `connectUrl` the API gives). The credential is
redacted before the example prints the URL.

**Why the connect leg is blocked (chromiumoxide 0.9.1, read from the crate).**
`observe_wss` proves connect+rebind against a browser we **launched** (it calls
`browser.new_page("about:blank")`). A hosted gateway hands back a browser that
already has its own page, and three approaches all fail:
- `new_page` **panics**: the `Target.createTarget` response is handled at
  `handler/mod.rs:199-208`, which unwraps `self.targets.get(&target_id)` and
  `panic!("Created target not present")` when the `targetCreated` event has not
  yet registered the new target. Against a remote browser that ordering is not
  guaranteed (comment in the crate: `// TODO can this even happen?`).
- `fetch_targets()` (issuing `Target.getTargets`) **registers** the existing page
  target, but its handler (`handler/mod.rs:216-238`) also fires
  `AttachToTargetParams::new(target_id)` — i.e. a **non-flat** session (the
  builder form at `handler/target.rs:332` sets `.flatten(true)`; the `::new`
  form does not). Commands on a non-flat session fail `-32001 Session with given
  id not found`. Worse, `Target::get_or_create_page` (`handler/target.rs:162-176`)
  caches the page on the **first** `session_id` it sees, so the poisoned non-flat
  session wins permanently even though the target's own init lifecycle later
  sends a correct flat attach.
- Touching **neither** call: at connect chromiumoxide enables
  `Target.setDiscoverTargets(true)` (`handler/mod.rs:96`), but Browserbase fired
  no `targetCreated` for its pre-existing page within a 5s poll, so no page ever
  materialized.

`HandlerConfig` (`handler/mod.rs:657-672`) exposes **no** `flatten` /
`auto_attach` lever, so there is no public-API way to force the flat-attach path
onto a pre-existing target.

**Decision.** Ship the acquire leg now; leave `connect()` at its proven
local-`ws://` `new_page` form (unchanged, so the run-4 live proof does not
regress); scope the hosted connect leg as the next increment. Preferred fixes, in
order: (1) bump chromiumoxide if a newer release fixes the `createTarget` race or
exposes `setAutoAttach{flatten:true}` / attach-to-existing-target — cleanest;
(2) add a minimal raw-CDP attach in `anchortree-cdp` that issues
`Target.attachToTarget{flatten:true}` ourselves and wraps the resulting flat
session as a `Page`, bypassing the poisoned `getTargets` attach; (3) last resort,
a small upstream PR to chromiumoxide. Live-verify against Browserbase when the leg
lands.

Sources (accessed 2026-06-17): chromiumoxide 0.9.1 (`src/handler/mod.rs:96,
199-238, 424-445, 657-672`; `src/handler/target.rs:126-180, 328-334`;
`src/browser/mod.rs:231-240, 382-431`); live Browserbase API
(`POST https://api.browserbase.com/v1/sessions`).

## D20 — the hosted connect leg is a self-contained thin CDP channel, not a chromiumoxide bump or a Page-wrap (2026-06-17) — CONFIRMED (builder run 12)

D19 ranked three fix paths for the blocked connect leg and preferred (1) bump
chromiumoxide, then (2) issue `Target.attachToTarget{flatten:true}` ourselves and
wrap the flat session as a `chromiumoxide::Page`. Research run 11 pressure-tested
both against primary sources. Both fail as written.

**Path (1) is a dead end right now.** crates.io: `0.9.1` (2026-02-25) is the
newest chromiumoxide release; nothing since. On GitHub `main`, there are **zero**
commits to `src/handler/mod.rs` or `src/handler/target.rs` since 2026-02-25 — the
two files that hold the `createTarget` panic and the non-flat `getTargets` attach.
No open PR addresses flat auto-attach: the only open target-area PRs are #322
(Worker target eval) and #323 (`connect_with_headers`, a WS-upgrade header hook
anchortree does not need because the credential rides in the URL). There is
nothing upstream to wait for.

**Path (2) as written is not reachable through the public API.**
`Browser::execute` (`src/browser/mod.rs:410`) sends only **sessionless**
browser-level commands; there is no public `execute_with_session`, even though
`CommandMessage` carries an optional `session_id` internally (`src/cmd.rs:41,62`).
And `Page` is constructed **only** via `impl From<Arc<PageInner>>`
(`src/page.rs:1384`), with `PageInner` crate-private and built solely inside the
Handler — no public `Page::new`/`Page::from(session)`. So even after we capture a
flat `sessionId`, chromiumoxide gives us no public seam to send session-tagged
commands or to materialize a `Page` around the session. Path (2) collapses into a
fork.

**Decision (proposed).** Re-scope the connect leg to a **self-contained thin CDP
channel** behind the existing `ObservationSource` trait seam, rather than reusing
`chromiumoxide::Page` for the hosted target. The hosted path needs only ~6 CDP
methods (`Accessibility.getFullAXTree`, `DOM.pushNodesByBackendIdsToFrontend`,
`DOM.getAttributes`, `DOM.getBoxModel`, `DOM.getDocument`, plus the action
dispatches). Implement an own-session client in `anchortree-cdp` that connects the
`wss://` URL (the 1.5b TLS lift already brought `async-tungstenite` + rustls into
the tree), issues `Target.attachToTarget{flatten:true}` once, captures the
`sessionId`, and routes every later command as a flat message tagged with that
session — reusing the typed `chromiumoxide_cdp` param/return structs (they
implement `Command`/serde, so no hand-rolled wire types) and implementing
`ObservationSource` directly. The local-`ws://` `new_page` path stays untouched
(run-4 proof intact); the hosted plumbing is confined behind the trait the core
already depends on; no fork.

Path (3) — a small upstream PR to chromiumoxide exposing flat-attach-to-existing
or a `HandlerConfig` auto-attach lever — is worth filing in parallel as substrate
good-citizenship, but it is **not** the critical path: the handler code has not
moved since February, so the connect leg must not wait on it. Builder confirms D20
when the own-session channel lands and live-verifies against Browserbase.

Sources (accessed 2026-06-17): crates.io API `/crates/chromiumoxide`; GitHub
`mattsse/chromiumoxide` `commits?path=src/handler/{mod,target}.rs&since=2026-02-25`
(both empty), open PRs #322/#323; chromiumoxide 0.9.1 (`src/browser/mod.rs:410`;
`src/cmd.rs:41,62`; `src/page.rs:1384`).

**CONFIRMED (builder run 12).** The connect leg shipped exactly as proposed. The
trait seam landed slightly sharper than the sketch: `CdpChannel` is a *sealed*
`pub trait` (the `private_bounds` lint forces it public because `CdpObserver<C>` is
public; sealing keeps it unimplementable downstream), and its single method is
`fn run<T: Command>(&self, cmd: T) -> impl Future<Output = Result<T::Response,
CdpError>> + Send`. The explicit `+ Send` RPITIT bound is load-bearing — it is what
keeps the generic `ObservationSource::observe` `Send`, and an `async fn` in a trait
cannot express it, so each impl carries `#[allow(clippy::manual_async_fn)]` with a
comment. `CdpObserver` was made generic (`CdpObserver<C = Page>`) so the entire
fusion/listener/decode pipeline is shared byte-for-byte across `impl CdpChannel for
Page` (local `new_page`, untouched) and `impl CdpChannel for RawCdpSession` (the new
flat transport). `connect_hosted(url)` connects the `wss://`, flat-attaches once,
and routes every later command as a `{id, method, params, sessionId}` envelope over
one multiplexed WebSocket, matching responses by numeric `id`; the typed
`chromiumoxide_cdp` Command structs are reused for (de)serialization. Pure wire
helpers (`build_envelope`, `response_for`, `select_page_target`) are unit-tested (9
new tests). **Live-verified against BOTH** a local `ws://` headless-shell (flat-
attached to a page the browser already had open — backendNodeIds 3–6 on first
observe prove pre-existence; all 4 eids rebound across an innerHTML swap) AND real
Browserbase `wss://` (session `1fdeb2f2-…`, rebind ledger 10→19, 11→20, 12→21,
13→22). 89 tests green, clippy/fmt clean. Path (3) — the optional upstream
flat-attach PR — remains unfilled and is tracked as future good-citizenship, not a
blocker. Phase 3.1 is complete end to end.

## D21 — Phase 3.2 multi-frame identity: a two-tier durable eid `(frame-key, in-frame fingerprint)`; same-origin is free from the pierced pass, OOPIFs flat-attach on our own channel (2026-06-17) — PROPOSED (research run 12)

With Phase 3.1 complete, 3.2 (multi-frame / iframe identity) is the next
self-contained increment and builds directly on the run-12 `CdpChannel`. This
decision settles its design from primary sources so the builder executes in one
pass.

**Prior art (Stagehand v3, read from source).**
`packages/core/lib/v3/understudy/a11y/snapshot/a11yTree.ts` builds a combined AX
tree by calling `Accessibility.getFullAXTree` per frame with a `frameId` param
(`:20,29`), attaching a per-frame session (`:39,52-55`), and encoding each node's
`backendDOMNodeId` into a frame-namespaced `encodedId` (`:115-118`) — recomputed
on **every snapshot** (snapshot-scoped). anchortree mirrors the per-frame
namespacing but makes the in-frame id **durable**, not a per-snapshot backend-id
encoding. That is the differentiation, restated at frame granularity.

**Capability check — every primitive is in chromiumoxide_cdp 0.9.1** (read from
`src/cdp.rs`): `GetFullAxTreeParams.frame_id` (`:20380`); DOM `Node.frame_id`
(`:42504`) + `content_document` (`:42508`); `Target.SetAutoAttachParams`
(`:106508`); `Page.GetFrameTreeParams` / `FrameTree` (`:89725` / `:85837`). No
raw-WS fallback or fork needed.

**Decision (proposed).** Durable element id becomes **two-tier:
`(frame-key, in-frame fingerprint)`**.
- *In-frame fingerprint* = the existing durable identity (role + stable attrs +
  landmark-scoped structural path), computed within the owning frame's subtree.
- *Frame-key* = the frame's **position in the frame tree** (parent-chain ordinal
  path from `Page.getFrameTree`), NOT the raw `frameId`. frameIds are stable within
  a navigation but a reload mints fresh ones; the structural frame-path is the
  durable analogue, mirroring our element-level preference for structural path over
  `backendNodeId`.

**Mechanics, in order.**
1. *Same-origin iframes — free from the existing pass.* The observer already
   fetches the pierced tree (`observer.rs:217-221`, `pierce(true)`), so every node
   arrives tagged with `node.frame_id` and iframe elements carry their
   `content_document`. Group nodes by `frame_id`, compute each frame-key from
   `getFrameTree`, namespace the fingerprint. No new attach.
2. *Cross-origin iframes (OOPIFs) — flat-attach on our own channel.* OOPIFs live in
   a separate target with their own backendNodeId space and session, and
   `getDocument{pierce:true}` does not reach them. Issue
   `setAutoAttach{autoAttach:true, flatten:true, waitForDebuggerOnStart:false}` on
   the channel's root session; for each attached child-frame session run
   getDocument(pierce)/getFullAXTree and fold its nodes in under that frame-key.
   This is the run-12 thin-channel model extended from one session to N — no
   chromiumoxide Handler, no fork.
3. *Frame-scope the resolve map.* Change the eid→backend resolve key from
   `backendNodeId` to `(frame-key, backendNodeId)`. backendNodeIds are unique only
   within a target, so they **collide** across OOPIF sessions; frame-keying the eid
   is what stops two different-frame nodes from fusing.
4. *Dispatch on the owning frame's session.* `actions.rs` resolveNode +
   click/type/select must run on the owning frame's flat session, so an eid carries
   a handle to its frame's session. Threading that owning-session handle through
   observe→resolve→act is the substantive part of the build.

Keep the single-frame fast path unchanged (root frame-key, current map) so the
run-4/run-12 live proofs do not regress. Live-verify with a page holding one
same-origin and one cross-origin iframe, each containing a structurally-identical
widget, and assert the two widgets receive distinct durable eids that both rebind
across an innerHTML swap. Builder confirms D21 when 3.2 lands.

Sources (accessed 2026-06-17): chromiumoxide_cdp 0.9.1 `src/cdp.rs`
(`:20380, 42504, 42508, 106508, 89725, 85837`); anchortree `observer.rs:194,
217-221`; Stagehand v3 `a11yTree.ts:20,29,39,52-55,115-118`
(github.com/browserbase/stagehand).

**D21 status (after builder run 13): PARTIALLY CONFIRMED + corrected.** Mechanics
1 (two-tier eid / frame-key), 2-same-origin, and 4 (frame-scoped resolve map)
shipped and live-verified against a real `srcdoc` iframe (`016ae2a`). One
correction the live run forced: mechanic 2's "same-origin frames are free from the
existing pass" holds for the **DOM** pass only — `getFullAXTree` with no `frameId`
stops at every frame boundary, so the observer now issues one
`getFullAXTree(frameId)` per same-origin frame and merges. The cross-origin half
(OOPIFs) is deferred to 3.2b and is the subject of D22.

---

## D22 — OOPIF leg needs a multi-session CDP channel (PROPOSED, research run 13)

**Context.** 3.2a landed same-origin multi-frame identity on the run-12
`CdpChannel`, which is single-session by construction: `RawCdpSession` holds one
`session_id: Option<String>` (`channel.rs:118`) and every `run` tags its request
with that one session (`:155`). Cross-origin iframes (OOPIFs) live in a **separate
CDP target** with their own backendNodeId space and **their own session**;
`getDocument{pierce:true}` does not reach them. So 3.2b cannot land without
teaching the channel to speak to N sessions.

**Decision (proposed).** Upgrade the thin channel from one session to N, in the
same Handler-free style established in run 12. Concretely:
1. *Multi-session write path.* Add `run_on(session_id, cmd)` (or hold a
   `frame-key → sessionId` map and pick per command). `next_id()` is already a
   shared monotonic counter and `response_for` (`:247`) demuxes responses by `id`
   alone, so the **request/response read side needs no change** — only the write
   side must tag the right sessionId. This keeps the run-12 single-session fast path
   byte-identical (default = the page session).
2. *Event-harvest read path.* The current loop is request/response only and
   discards all events (`ResponseFor::Other => continue`, `:200`).
   `setAutoAttach{autoAttach:true, flatten:true, waitForDebuggerOnStart:false}`
   announces child sessions via `Target.attachedToTarget` **events**. Add a one-shot
   event-drain (issue setAutoAttach, then read until the expected child
   `attachedToTarget` events have arrived) that records each child `sessionId` +
   `targetInfo`. This is the one genuinely new surface in the build.
3. *Frame-key ↔ session join.* An OOPIF subframe target's `targetInfo.targetId`
   equals its page `frameId`, and that frameId appears in the **root**
   `Page.getFrameTree` (the frame node is in the page tree even though its document
   is out-of-process). So the durable frame-key (structural parent-chain path,
   already computed in `frames.rs`) is derivable from the root session and joined to
   the child session by `targetId == frameId`. The builder must assert this join
   live (one line in the example) rather than trust it blind.
4. *Per-child observe.* For each child session: enable the needed domains, then run
   `getDocument(pierce)` + `getFullAXTree` (no frameId — the OOPIF document is the
   child target's root) and fold the nodes in under that frame-key. The run-13
   AX-per-frame correction applies: one AX call per child session, no shortcut.
5. *Action dispatch on the owning session.* `actions.rs` resolveNode +
   click/type/select must run on the owning frame's session, so an eid carries (or
   can look up) its frame's sessionId. The `(frame-key, backendNodeId)` resolve-map
   key from 3.2a already prevents the cross-target backendNodeId collision — no
   further map change.

**Why this and not a chromiumoxide upgrade or a fork.** Run 11 established that
chromiumoxide 0.9.1 is the newest release, its `Browser::execute` is sessionless,
and `PageInner` is private — so the multi-session machinery cannot come from the
library; it is ours, extending the run-12 channel from 1 session to N. No new
dependency, no fork.

**Confirm criterion.** Builder confirms D22 when 3.2b lands: a page with one
cross-origin iframe whose widget is structurally identical to a root widget yields
two distinct durable eids that both rebind across an `innerHTML` swap, dispatched
on their owning sessions, exit 0.

Sources (accessed 2026-06-17): anchortree `crates/anchortree-cdp/src/channel.rs`
(`:118`, `:155`, `:200`, `:247`); run-11 chromiumoxide 0.9.1 findings (D19/D20);
CDP `Target` domain (`setAutoAttach` / `attachedToTarget`). Market context:
browser-use "Closer to the Metal: Leaving Playwright for CDP"
(browser-use.com/posts/playwright-to-cdp).

### D22 amendment — step 3's join source was wrong; the pierced DOM, not getFrameTree (ACCEPTED, builder run 14)

Step 3 claimed an OOPIF subframe's `frameId` "appears in the **root**
`Page.getFrameTree`". A live micro-proof (`examples/attach_oopif`, against
`chromedp/headless-shell --site-per-process` with a genuinely cross-origin child
on a second network alias) **falsified that**. Verified with raw-CDP probes
(`/tmp/probe*.py`, throwaway) against the same browser:

- A cross-origin OOPIF's frame is **absent** from the root target's
  `Page.getFrameTree` — before *and* after `setAutoAttach`. `getFrameTree` only
  lists same-process frames. So a `getFrameTree`-derived key table never contains
  the OOPIF's id and `child_frame_keys` would silently drop every cross-origin
  join.
- The OOPIF's owner `<iframe>` element **is** present in the root
  `getDocument{depth:-1,pierce:true}` DOM. It carries `frameId` == the child
  frame's id == the child target's `targetId`, but with its `contentDocument`
  **stripped** (which is exactly why `same_origin_frame_ids` already skips it).
- `Target.attachedToTarget` carries `targetInfo.parentFrameId` (the parent/root
  frame) and `targetInfo.targetId` (the child's own frameId).

**Corrected mechanism.** Derive the structural frame-key table from
**DOM document order** of iframe owners (`frames::dom_frame_keys`), which includes
OOPIF owners, not from `getFrameTree` (`frames::frame_keys`), which omits them.
`dom_frame_keys` agrees with `frame_keys` on every same-origin frame and
additionally keys each OOPIF owner with the structural slot it would have held had
its document been inline. The join `child.target_id -> dom_frame_keys[target_id]`
then lands cross-origin children on a non-root key. `child_frame_keys`'s
**signature was already correct**; only the table it was fed had to change.
`HostedSession::frame_keys()` now reads the pierced DOM, not the frame tree.

`parentFrameId` is captured-but-unused: the join needs only `target_id` ->
`dom_frame_keys`, so `ChildSession` deliberately does **not** carry a redundant
parent field. The proof asserts at least one cross-origin child joins a non-root
key (run 14: child target `6747…` -> frame key `1`, exit 0).

Steps 1 (multi-session write via `run_on`), 2 (event-drain via
`auto_attach_children`), and 3 (the corrected join) shipped in run 14. Steps 4
(per-child observe) and 5 (action dispatch on the owning session) remain for a
follow-up run; this run scoped to the channel infra + the join + the live proof.

Sources (accessed 2026-06-17): live `chromedp/headless-shell:latest`
`--site-per-process`; `examples/attach_oopif`; raw CDP `Target.attachedToTarget`,
`Page.getFrameTree`, `DOM.getDocument{pierce:true}` payloads observed this run.
