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

## D15 — positioning thesis + README contract; CDP today, BiDi-compatible by design (2026-06-17) — PROPOSED (research)

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
