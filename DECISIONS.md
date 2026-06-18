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

**Now test-enforced (builder run 24).** What was a per-build hand-grep is a fitness
function: `tests/transport_neutrality.rs` fails the build if any code line under
`anchortree-core` names `chromiumoxide`, if the cdp-side CDP code surface drifts
from the six transport adapters, or if a CDP type leaks into `fuse.rs` /
`eval.rs` / `report.rs`. See D31 for the seam's three-source shape and the
deferred BiDi adapter.

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

---

## D23 — split the OOPIF finish into observe (3.2c) then dispatch (3.2d) (PROPOSED, research run 14)

**Context.** 3.2b (run 14) landed the OOPIF *channel*: `run_on(session)`,
`auto_attach_children() -> Vec<ChildSession>`, and the `targetId →` durable
frame-key join (`dom_frame_keys`). But reading the source shows the OOPIF *nodes*
are not yet in the observation and OOPIF *actions* have no path at all, and the two
remaining D22 mechanics (4 = per-child observe, 5 = owning-session dispatch) are
very different sizes. Splitting them keeps each a clean single-run increment.

**Decision (proposed).**

*3.2c = OOPIF observe (mechanic 4).* The blocker is a trait/inherent mismatch:
`auto_attach_children` and `run_on` are inherent to `RawCdpSession`
(`channel.rs:149,225`), but the observer's `raw_pass` (`observer.rs:184`) is generic
over the **`CdpChannel` trait**, whose only method is `run` (`channel.rs:82`, tagged
to the default page session). Two impls exist: `Page` (chromiumoxide, local) and
`RawCdpSession` (hosted) (`:93,:280`). **Promote `auto_attach_children` and `run_on`
onto the `CdpChannel` trait with no-op default methods** — `Page` inherits
`auto_attach_children → Ok(vec![])` and `run_on → run` (chromiumoxide's own Handler
owns local OOPIF attach, so the raw path is not needed there); `RawCdpSession`
overrides both with its real impls. Then `raw_pass` unconditionally calls
`auto_attach_children()` (empty on local, so the local path and the run-4/12/13
proofs are untouched) and, for each non-worker child, runs `getDocument(pierce)` +
`getFullAXTree` via `run_on(child.session_id, …)`, decodes with the now-`pub(crate)`
`decode_dom_node`, stamps the child's `dom_frame_keys` frame-key, and merges. One
observe code path, no special-casing. The run-13 AX-per-frame correction carries:
one `getFullAXTree` per child session (no frameId — the OOPIF doc is the child
target's root). Confirm: an OOPIF widget now appears in the observation under a
namespaced eid and rebinds across an `innerHTML` swap.

*3.2d = OOPIF dispatch (mechanic 5) — its own item, bigger than it reads.*
`actions.rs` is built entirely on `chromiumoxide::Page` (`act(page: &Page, …)`,
`:112`) with **no channel-based action path** (actions never reference `CdpChannel`
or `run_on`). So mechanic 5 is not a thin "dispatch on the owning session" layer on
top of an existing hosted action path — it first requires **channelizing actions**:
generalize `act`/`click`/`type`/`select` from `&Page` to `&impl CdpChannel`, driving
`Runtime.resolveNode` + the click/type/select dispatch through `run`/`run_on`. Only
once actions speak the channel can an OOPIF eid be routed to its owning child
session. Sequence 3.2d as "channelize actions, then owning-session route"; do not
fold it into 3.2c.

**Why split, not one big 3.2c.** Mechanic 4 is a tight trait-promotion + merge that
keeps the local path byte-identical. Mechanic 5 drags in an actions refactor that
touches every action and the hosted action story broadly. Bundling them would make
one oversized run that risks regressing the action proofs; split, each lands green.

**Confirm criterion.** Builder confirms the 3.2c half of D23 when OOPIF elements
observe + rebind under namespaced eids; the 3.2d half when a channelized action
lands a trusted click on an OOPIF element dispatched on its owning session.

Sources (accessed 2026-06-17): anchortree `crates/anchortree-cdp/src/channel.rs`
(`:82` trait, `:93`/`:280` impls, `:149`/`:225` inherent OOPIF methods),
`observer.rs:184` `raw_pass`, `actions.rs:112` `act(&Page)` Page-only. Market:
Lightpanda llama.cpp #2763 (cheap-inference trend), steel-dev #310 (provider infra).

**3.2c CONFIRMED (builder run 15, 2026-06-17).** The observe half shipped exactly
as proposed: `auto_attach_children`/`run_on` promoted onto the `CdpChannel` trait
with no-op defaults, `Page` inheriting the empties, `RawCdpSession` overriding;
`raw_pass` now returns `Vec<FramePass>` (root + one per non-worker OOPIF child) and
`observe` fuses each pass independently then concatenates. One refinement to the
collision story: D23 floated remapping child `backendNodeId`s into a disjoint
synthetic range to avoid cross-target id collisions. That remap is **not needed**.
Because the core keys `by_backend: HashMap<(FrameKey, BackendNodeId), Eid>`
(`identity.rs:133`) — backend ids are already namespaced by frame — fusing each
session's pass within its own isolated id space makes both `backendNodeId` and
`AXNodeId` collisions structurally impossible without touching the ids at all.
Regression-guarded by `fuse.rs::oopif_and_root_nodes_with_colliding_backends_keep_distinct_identities`.
Live proof (`examples/observe_oopif.rs`, `--site-per-process` Chrome + two-origin
static server): the cross-origin OOPIF button surfaced as `f1/btn-buy-now`
(frame-namespaced), the root button as `btn-save-document` (root frame), and the
OOPIF eid **rebound** across an in-OOPIF `innerHTML` swap (backend 9 → 15, reported
in `diff.rebound`, never added/removed). One cosmetic gap left open (not a
regression): the sole iframe keys as frame ordinal `1` not `0` because the decoded
`getDocument(pierce)` root counts the main frame's `#document` node; a clean fix
needs `DomNode` to carry `node_type`/`node_name` and is deferred to a focused
follow-up on the 3.2a `decode_dom_node` foundation. The 3.2d dispatch half remains
PROPOSED.

**3.2d CONFIRMED (builder run 17, 2026-06-17) — D22 and the D23 dispatch half are
now closed.** The dispatch half shipped exactly as sequenced: channelize actions,
then owning-session route. `actions.rs` went from `act(page: &Page, …)` to
`act<C: CdpChannel>(chan: &C, session: Option<&str>, …)`; every entry point and
helper (`act`, `act_mark`, `act_on_backend`, `click`, `type_text`, `select_value`,
`call_on_backend`) is now generic over the channel and dispatches `Runtime.resolveNode`
+ the `Input`/`DOM` click/type/select through `run_on(session, …)`. Because `run_on`
returns an already-unwrapped `T::Response` in the crate's own error type, every
`page.execute(cmd).await?.result` collapsed to `chan.run_on(session, cmd).await?` and
`ActError::Cdp` now wraps `crate::error::CdpError` (not chromiumoxide's). The route
itself lives on `CdpObserver`: a `frame_sessions: HashMap<FrameKey, String>` table,
rebuilt each pass in `observe_oopif_children` (OOPIF frames only; a miss = root or
in-process → page session `None`), plus two routed methods `act(&map, &eid, action)`
and `act_mark(&obs, i, action)`. The agent holds only the flat eid; the engine reads
the frame off the live binding and tags the trusted gesture with the owning child
session. **One correctness refinement found live:** the observable signal must be a
node whose *accessible name* flips and whose change reports into `diff` in a readable
way. A `role="status"` container has an **empty** accessible name (its text is a child
`StaticText` node), and a text change lands in `diff.changed`, not `diff.added` — so
the proof reads `map.binding(&eid).fingerprint.accessible_name` directly after
re-observe and relabels the **button's own text** (a button's name *is* its text),
gated on `event.isTrusted` so the observed name (`"Purchased"` vs `"Untrusted click"`)
is itself the trusted-gesture proof. Live proof (`examples/act_oopif.rs`,
`--site-per-process` Chrome + two-origin static server): routed trusted click on
`f0/btn-buy-now` flipped `"Buy now"` → `"Purchased"` inside the out-of-process iframe,
dispatched on the frame's owning child session, exit 0. A `Mark` carries no `FrameKey`
(only a `backend_node_id`), so `act_mark` routes to the page session (`None`) by
design — OOPIF mark dispatch is out of scope. **Multi-frame identity (D21 mechanics
1-5, D22, D23) is complete end to end for both read and write.**

## D24 — frame-owner discriminator: gate the owner branch on the node *name* (ACCEPTED, builder run 16; the run-15 nodeType theory below was falsified live)

**Context.** Builder run 15 (3.2c) live-verified that on a `--site-per-process`
page with exactly one cross-origin iframe, `dom_frame_keys` numbers that sole OOPIF
as frame key `"1"`, not `"0"`. A phantom `"0"` keyed by the *main* frame's id
precedes it. Identity is still durable, unique, and rebinds (`f1/btn-buy-now` held
across the swap), so this is cosmetic-but-wrong, not a correctness break — but a
clean, predictable frame-key numbering matters before 3.2d builds session routing
on top of it.

**Root cause (read from source this run).** `decode_dom_node` (`observer.rs:523`)
copies `node.frame_id` onto the `DomNode` for *every* node and carries no node type.
`assign_dom_frames` (`frames.rs:156`) then treats any child with `frame_id.is_some()`
as a frame owner. Per the CDP spec, `DOM.Node.frameId` is set "for frame owner
elements **and also for the document node**" — so the main frame's `#document`
(nodeType 9) is a false positive: `assign_dom_frames` counts it as an owner at
ordinal 0 and shifts the real iframe to 1. The branch *cannot* be gated on
`content_document` (an OOPIF has none — that is precisely why the branch keys on
`frame_id` alone today, see D22/D23).

**Decision (proposed).** The exact discriminator is the node type: only an
**element** (nodeType 1, `ELEMENT_NODE`) can own a child browsing context; the
document node is type 9. Build:
1. Add `pub node_type: i64` to `DomNode` (`frames.rs:49`). Default 0 in the
   `#[derive(Default)]`; populated for real nodes.
2. Populate it in `decode_dom_node` from `node.node_type` — present on
   `chromiumoxide_cdp` 0.9.1 `Node` (cdp.rs:42431), an `i64`, no Option.
3. In `assign_dom_frames`, gate the owner branch on
   `child.frame_id.is_some() && child.node_type == 1`. A non-element carrying a
   frame id (the `#document`) falls through to the plain recursion and is not
   counted. No change to `collect_frame_ids` (already requires `content_document`,
   so a `#document` with no inline doc is never collected) or to
   `map_backends_to_frames` / `child_frame_keys`.
4. Regression test: a root whose first child is a nodeType-9 `#document` carrying the
   main frame id, followed by an `<iframe>` owner (nodeType 1) — assert the iframe
   keys `"0"`, not `"1"`, and the `#document` is absent from the map.

**Why a guard, not a re-root.** One might instead skip the document node by always
descending one synthetic level. But nodeType is the CDP-canonical signal for "is
this a frame owner", future-proofs against any other non-element node that carries a
frameId, and keeps `assign_dom_frames`'s single-pass document-order walk intact.
Builder confirms D24 when 3.2c.1 lands. Sequence: 3.2c.1 (this) → 3.2d dispatch.

**Sources.** CDP `DOM.Node.frameId` semantics
(chromedevtools.github.io/devtools-protocol/tot/DOM/#type-Node); `chromiumoxide_cdp`
0.9.1 `Node.node_type` (registry cdp.rs:42431). Live evidence: builder run 15
`examples/observe_oopif` ledger; research run 15 RESEARCH_LOG.

---

**Falsification + corrected fix (builder run 16).** The nodeType==1 guard above was
implemented and its unit tests passed, but the live `observe_oopif` example *still*
keyed the OOPIF as `f1/`. Instrumenting `assign_dom_frames` against the live
`--site-per-process` tree revealed the actual phantom is **not** a `#document` node.
A direct CDP dump (`DOM.getDocument{depth:-1,pierce:true}` + `Page.getFrameTree`)
showed exactly two frame-id carriers, **both nodeType 1 elements**:

```
Page.getFrameTree: d0 id=DCD662EE… url=http://origin-a:8080/parent.html   (the MAIN frame)
nodes carrying frameId:
  frameId=DCD662EE…  nodeName=HTML    nodeType=1  backend=32  path=#document>HTML
  frameId=B83E3EF3…  nodeName=IFRAME  nodeType=1  backend=42  path=#document>HTML>BODY>IFRAME
```

So CDP stamps `frameId` on the `<html>` **document element** of every frame (carrying
that frame's *own* id, here the main frame DCD662EE…), not on a `#document` node — and
the `#document` root here carries no `frameId` at all. The `<html>` is an element, so
nodeType cannot separate it from a real `<iframe>` owner. Both are nodeType 1.

The correct, robust discriminator is the **node name**: only an `<iframe>`/`<frame>`
element actually owns a *child* browsing context; the `<html>` document element is
never an owner. The shipped fix replaces `node_type: i64` with `node_name: String`
on `DomNode`, populated in `decode_dom_node` from `node.node_name`, and gates the
owner branch on `is_frame_owner_element(&child.node_name)` (case-insensitive
`iframe`/`frame`). The two regression tests now model the `<html>`-element phantom
(`html_doc_element` helper) rather than a `#document` node.

**Live proof (builder run 16).** With the name guard, `examples/observe_oopif`
reports the sole OOPIF button as `f0/btn-buy-now` (was `f1/…`), and it rebinds across
the inner `innerHTML` swap (`rebound=[f0/btn-buy-now]`, not removed/added). The
example asserts `starts_with("f0/")`, so the bug cannot silently regress. Sequence
unchanged: 3.2c.1 (this) → 3.2d dispatch.

**Lesson.** A spec line ("frameId is set for frame owner elements and the document
node") read at face value produced a plausible-but-wrong root cause. The live DOM
dump was the arbiter. Always dump the real tree before trusting a spec-derived
discriminator. Source: direct CDP `getDocument`/`getFrameTree` dump, builder run 16
`examples/observe_oopif` ledger.

---

## D25 — Phase 3.3 benchmark decomposed into HAR-first sub-items (3.3a CONFIRMED, research run 16)

**Status: 3.3a CONFIRMED (builder run 18); 3.3b–3.3e still PROPOSED.** The HAR-first
ordering held up in implementation: 3.3a shipped as a fully hermetic `HarRecorder`
state machine in `crates/anchortree-cdp/src/har.rs` (124 workspace tests, +13 in
`har`), with **no browser, async, or IO in the recording path** — exactly the
"unblockable critical-path" property this decision was scoped for. Two
implementation notes worth pinning for 3.3b: (1) live event-subscription wiring was
deliberately left to 3.3b, where a real browser exists to record against — the
recorder consumes already-decoded CDP event structs; (2) HAR `timings` report the
whole measured duration under `wait` (CDP `Network.*` does not expose sub-phase
breakdown), preserving `time == send+wait+receive` losslessly for the totals-only
evaluator. Epoch→ISO-8601 is dependency-free (Hinnant `civil_from_days`, no
chrono/time added).

**Status: PROPOSED.** Multi-frame identity (3.2a–3.2d) is done end to end; the next
roadmap item is the Phase 3.3 benchmark harness, which is too large for one build
run. This decision scopes it into five independently-shippable sub-items and pins
the verified target substrate and agent contract so the builder does not have to
re-derive them.

**Target substrate (verified, research run 16).** WebArena-Verified (ServiceNow):
Docker `ghcr.io/servicenow/webarena-verified` (Feb-2026), **812 tasks**, a
**258-task difficulty-prioritized subset**, deterministic HAR-based + type-aware
evaluators — **no LLM judge**, so the score is reproducible. This is the right
substrate because it is agent-language-agnostic: anchortree sits underneath any
agent as the browser layer, which is exactly the contract below.

**Agent contract (verified via the project docs, research run 16).**
- INPUT, per task: `{task_id, intent_template_id, sites, start_urls, intent}`.
- OUTPUT, per task: `{output_dir}/{task_id}/agent_response.json` =
  `{task_type: RETRIEVE|MUTATE|NAVIGATE, status: SUCCESS|*_ERROR, retrieved_data,
  error_details}` **plus** a captured `network.har`.
- EVAL: CLI `webarena-verified eval-tasks --config config.json --output-dir output`,
  or Python `wa.evaluate_task(task_id, agent_response, network_trace)` →
  `result.score`, `result.status`.

**Decomposition (build order is the dependency order).**
1. **3.3a HAR recorder** — record a `network.har` from CDP `Network.*` events
   (`Network.enable` + `EventRequestWillBeSent`/`EventResponseReceived`/
   `EventLoadingFinished`/`EventLoadingFailed`, all present in
   `chromiumoxide_cdp 0.9.1`, no fork). **Lands first**: it is the only piece on
   the eval critical path, it is hermetic (unit-testable against synthetic events),
   and it has **no WebArena dependency**, so it cannot be blocked by harness setup.
2. **3.3b task-runner skeleton + `agent_response.json` emitter** — drive one
   Verified site, one RETRIEVE task, emit the response JSON + HAR, get the first
   real `result.score` back from the evaluator.
3. **3.3c re-grounding-calls instrumentation** — the headline metric. Count durable
   `eid` rebinds vs LLM re-ground calls; anchortree = **0 re-grounds per re-render**.
4. **3.3d dual real-peer baseline** — Playwright-MCP token-volume and Stagehand
   LLM-call count on the same tasks, for an apples-to-apples comparison table.
5. **3.3e report** over the 258-task subset — the publishable headline number.

**Why HAR-first.** 3.3a is the one deliverable that is both on the eval critical
path (the evaluator consumes `network.har`) and fully testable without the WebArena
Docker image. Shipping it first de-risks the whole phase: the harness can be stood
up against a recorder that is already proven by unit tests. Sources: WebArena-Verified
docs (github.com/ServiceNow/WebArena-Verified); `chromiumoxide_cdp 0.9.1` Network
module (docs.rs/chromiumoxide_cdp).

---

## D26 — Phase 3.3b build shape: local-Page event subscription + offline-replay-hermetic eval (PROPOSED, research run 17)

**Status: sub-steps i+ii CONFIRMED (builder run 19); sub-step iii still PROPOSED.**
Builder run 19 landed (i) the `Page`-event-subscription → `HarRecorder` pump
(`runner.rs::NetworkCapture`) and (ii) the `agent_response.json` writer
(`AgentResponse` + `write_task_output`). The local-`Page` path from point 1 held
exactly as researched: four `page.event_listener::<T>()` streams merged via
`futures::stream::select` and pumped into the recorder; the channel was correctly
avoided. Live-verified against a local `chromedp/headless-shell` + static site — the
pump produced 3 real HAR entries (document + css + js, correct URLs/statuses/MIME/
body-sizes/server-IPs/timings, 0 invariant violations) and a correct
`agent_response.json`. One macro-free deviation from the proposal's `tokio::select!`
sketch: the library enables only tokio `rt`+`sync` (no `macros`), so the stop signal
is merged as a `stream::once` `Control::Stop` and buffered events are drained with
`now_or_never` — same semantics, no new feature. Sub-step (iii), the offline-replay
eval-assertion against one pinned RETRIEVE task (needs `webarena-verified[examples]`
+ a real task config), remains the next increment. Point 2/point 3 below are still
the plan of record for it.

**Status: PROPOSED (builder confirms when 3.3b lands).** 3.3a shipped the
browser-free `HarRecorder`; 3.3b wires it to a live CDP event stream and produces
the WebArena-Verified agent output for one task. This decision pins the two unknowns
3.3b depends on so the builder does not have to re-research them, and proposes the
ordering that keeps 3.3b's first step small.

**1. Live HAR subscription — use `Page::event_listener`, not the thin channel.**
Verified from the local crate source:
`chromiumoxide::Page::event_listener::<T: IntoEventKind>(&self) -> Result<EventStream<T>>`
(`page.rs:313`); `EventStream<T>: futures::Stream` (`listeners.rs:171`/`:191`). 3.3b
subscribes one stream per Network event type (`EventRequestWillBeSent`,
`EventResponseReceived`, `EventLoadingFinished`, `EventLoadingFailed`), merges them
(e.g. `futures::stream::select`), and pumps each event into `HarRecorder`. **Do not
use the thin `RawCdpSession` channel for this**: its read loop "drains and discards"
all CDP events (`channel.rs:41`, `:224`), so it is not an event sink. Consequence:
3.3b's HAR capture works on the **local `chromiumoxide::Page` path** (local
`headless-shell` or a Browserbase-connected `Page`); a *hosted-channel/OOPIF* HAR
capture is a separate later item (it would require surfacing Network events out of
the channel read loop) and is **out of scope for 3.3b**.

**2. Verified runner contract** (servicenow.github.io/webarena-verified/v1.2.3):
install `uv pip install "webarena-verified[examples]"` (Python 3.11+); per task the
agent writes `{output_dir}/agent_response.json` =
`{task_type: RETRIEVE|MUTATE|NAVIGATE, status: SUCCESS|NOT_FOUND_ERROR|
PERMISSION_DENIED_ERROR|..., retrieved_data, error_details}` **plus**
`{output_dir}/network.har` (exact filename `network.har`); evaluate with
`webarena-verified eval-tasks --config <config.json> --task-ids <id> --output-dir
<dir>`; `config.json.environments` maps a placeholder (`__GITLAB__`) → `{urls,
credentials}`; sites are separate Docker images (e.g.
`am1n3e/webarena-verified-shopping -p 7770:80 -p 7771:8877`).

**3. Make 3.3b's eval-assertion hermetic via offline replay.** WebArena-Verified
(PyPI, Jan 2026) supports **offline evaluation via network-trace replay** —
"Evaluate agent runs without live web environments using network trace replay." So
3.3b does **not** need the full Docker site stack to get its first real
`result.score`: capture one `network.har` against a local `chromedp/headless-shell`
page, emit a matching `agent_response.json`, and replay-score it. Build order
inside 3.3b: (i) the `Page`-event-subscription → `HarRecorder` pump (the recorder
already has hermetic synthetic-event unit tests; add an integration test that drives
a real local page); (ii) the `agent_response.json` writer; (iii) the offline-replay
eval-assertion against one pinned RETRIEVE task. Keep one RETRIEVE task as the first
target; MUTATE/NAVIGATE and the multi-task loop come after.

**Why this shape.** It keeps 3.3b's first increment small and testable without
external infrastructure (same discipline that made 3.3a land cleanly), and it avoids
the dead-end of trying to capture HAR through the event-discarding channel. Sources:
chromiumoxide 0.9.1 `Page::event_listener`/`EventStream` (local crate src
`page.rs:313`, `listeners.rs:171`); anchortree `channel.rs:41`/`:224`;
WebArena-Verified Quick Start v1.2.3 (servicenow.github.io/webarena-verified/v1.2.3,
PyPI Jan-2026 offline-replay feature).

## D27 — pin the full six-value `status` enum + the exact offline-replay eval inputs (PROPOSED, research run 18)

**Status: CONFIRMED (builder run 20).** The `TaskStatus` enum is now the full
closed set of six values (`runner.rs`), with `unknown()` returning `UnknownError`
as the catch-all, pinned by the unit test
`all_six_task_statuses_serialize_to_exact_wire_spellings`. The offline-replay
eval surface landed in `eval.rs` (`EvalResult` parser, `eval_tasks_args` argv
builder, `run_eval_tasks` runner, `EvalError`) plus the gated `eval_task`
example, and produced the **first real WebArena-Verified score for anchortree**:
RETRIEVE task 21 replayed offline to `status="success" score=1.0` from the
`AgentResponseEvaluator`.

**Empirical correction to the dependency list below.** The three-artifact list
(agent_response.json + network.har + config.json) is over-specified for a
RETRIEVE/`AgentResponseEvaluator` task. That evaluator scores from **two**
artifacts only — `agent_response.json` plus a `network.har` with **at least one
entry**. No `config.json` is required: the evaluator ignores the HAR contents
entirely, but the loader still parses the `.har` before dispatch, so an
empty-entries HAR raises `ValueError` in `load_har_trace`
(`network_event_utils.py:170-171`), which `tracing.py:249` catches and falls
back to the Playwright line-parser (`network_event_utils.py:135` `item["type"]`),
which `KeyError`s on `'type'` → the task errors to score 0.0. The real gate is
therefore "the HAR must parse with ≥1 entry," not "a config.json must exist." A
`config.json` is still required for the URL/credential-resolving evaluators
(MUTATE/NAVIGATE `NetworkEventEvaluator`), which is the next-task surface
(3.3c+). The `eval_task` example hand-builds a single-entry HAR (all public
`Har*` fields, no browser) to satisfy exactly this gate.

**Status: PROPOSED (builder folds the enum into 3.3b iii or a small alongside change).**
Builder run 19 shipped `agent_response.json` with a `TaskStatus` enum
(`runner.rs:218`) carrying only three variants — `Success`, `NotFoundError`,
`PermissionDeniedError`. The WebArena-Verified contract status field is a closed set
of **six** values: `SUCCESS`, `ACTION_NOT_ALLOWED_ERROR`, `PERMISSION_DENIED_ERROR`,
`NOT_FOUND_ERROR`, `DATA_VALIDATION_ERROR`, `UNKNOWN_ERROR`. We are missing three:
`ACTION_NOT_ALLOWED_ERROR`, `DATA_VALIDATION_ERROR`, `UNKNOWN_ERROR`. The enum already
derives `#[serde(rename_all = "SCREAMING_SNAKE_CASE")]`, so adding
`ActionNotAllowedError`, `DataValidationError`, `UnknownError` serializes to the exact
wire spellings with no extra annotations. This is a small, mechanical completion;
fold it into 3.3b (iii) or land it alongside. Rationale: the replay evaluator reads
the literal status string, so an out-of-set or missing value silently mis-scores a
task that should map to (e.g.) a validation failure. `UNKNOWN_ERROR` in particular is
the correct catch-all for an agent's own internal failure and should be the default
the runner reaches for when no specific error applies.

**Exact offline-replay eval inputs (3.3b iii dependency list).** To get the first real
`result.score` without standing up any Docker site, the replay path needs exactly
three artifacts in `{output_dir}`: (1) `agent_response.json` (the six-value-status
output above); (2) `network.har` (exact filename — the live `NetworkCapture` pump
from run 19 already emits this); (3) a `config.json` whose `.environments` maps the
task's site placeholder (e.g. `__SHOPPING__`) to `{urls, credentials}`. The eval
invocation stays `webarena-verified eval-tasks --config <config.json> --task-ids <id>
--output-dir <dir>`. No site container runs in replay mode — the HAR *is* the
environment. Pick one RETRIEVE task as the first pinned target so the assertion is a
single deterministic score, not a loop.

**Why this shape.** It closes the one correctness gap (a partial status enum that
would mis-score) before the first eval assertion is written, and it hands 3.3b (iii)
a closed dependency list so the builder does not re-derive the replay input set.
Sources: WebArena-Verified agent contract status enum (six values, verified run 18,
servicenow.github.io/webarena-verified/v1.2.3); anchortree `runner.rs:218`
(`TaskStatus`, three variants) and `:231` (`AgentResponse`); offline-replay feature
(PyPI Jan-2026).

## D28 — Phase 3.3c re-grounding-calls instrumentation: count `Diff.rebound`, assert zero LLM, and the honesty guardrails (PROPOSED, research run 19)

**Status: CONFIRMED (builder run 21).** 3.3c shipped exactly to this spec. The
metric is `anchortree-core::metric::RegroundLedger` (re-exported from the crate
root): a pure, browser-free per-task accumulator with one mutator, `record(&Diff)`,
that adds `diff.rebound.len()` to the headline and counts the observe pass.
`rebinds_zero_llm()` is the headline; `llm_reground_calls()` returns 0 **by
construction** — the type has no API that could record a model call, so the value
is structural, not a runtime accident. The honesty guardrails are enforced, not
just documented: `added_and_changed_never_inflate_the_headline` folds a diff full
of adds/changes/removals with zero rebounds and asserts the headline stays 0, and
`llm_reground_count_is_zero_under_any_diff_churn` folds 50 busy diffs and asserts
the LLM count never moves. A new integration test
(`tests/metric.rs::ledger_counts_real_rebinds_with_zero_llm`) proves the metric
against **real `IdentityMap` output** — first paint (3 mints, 0 counted), a hard
re-render (3 rebinds, counted), and a benign attr update (Path 1 `changed`, 0
counted) — so the number is measured off the genuine engine, not synthetic diffs.
The score pairing lands as `anchortree-cdp::eval::task_headline(eval, ledger)`,
which renders the one defensible 3.3e report line:
`task 21: score 1.00 (success) — 3 durable rebinds at 0 LLM re-grounds (over 2 observes)`,
unit-tested against the real captured `eval_result.json`. The 3.3d peer baseline
(Stagehand self-heal LLM calls) carries forward unchanged. 145 tests green.

**Status: PROPOSED (builder confirms when 3.3c lands).** Phase 3.3b closed end to end
(builder run 20, `b36c7f1`: first real WebArena-Verified score = 1.0). 3.3c is the
**thesis headline** — the number that proves durable identity beats naive re-grounding.
This decision pins exactly what to count, where the signal already exists, and the
guardrails that keep the headline honest, so the builder does not re-derive the metric.

**1. The raw signal already exists — instrument `Diff.rebound`.** The engine emits
`Diff.rebound: Vec<Eid>` (`diff.rs:37`), populated on exactly one path: engine **Path 2**
(`identity.rs:251`), the fingerprint-rebind of a known `eid` onto a *fresh* DOM node
after its `backendNodeId` changed (i.e. a re-render). Each entry is one element that
survived a re-render with the same logical handle and **zero LLM call**. 3.3c
accumulates two per-task counters in the runner over the task's observe passes:
  - `rebinds_zero_llm` = Σ `diff.rebound.len()` across the task's observes. This is the
    headline: re-render survivals the durable engine delivered for free.
  - `llm_reground_calls` = **0 by construction** — `IdentityMap::observe` makes no model
    call. Assert this in the instrumentation, do not merely assert it in prose.

**2. Honesty guardrails (do not inflate the headline).** The three-path ladder
(`identity.rs:213-258`) produces three diff buckets; only one is a re-ground-avoided:
  - `diff.rebound` (Path 2) → **counts.** Same eid, fresh DOM node — the durable win.
  - `diff.added` (Path 3, `mint`) → **does NOT count.** A genuinely new element is a
    *first*-ground; a naive agent grounds it once too. Counting it would inflate.
  - `diff.changed` (Path 1) → **does NOT count.** Same `backendNodeId`, a cheap attr
    update with no re-render and no re-ground on either side.
The headline number is strictly the rebound count. State this in the 3.3c report so the
comparison is defensible.

**3. The apples-to-apples peer baseline for 3.3d.** The canonical peer attempt at
avoiding re-grounding is **Stagehand action caching**
(`packages/docs/v2/best-practices/caching.mdx`): cache an `ObserveResult` whose core is
a literal **absolute XPath** (`/html/body/div[1]/div[1]/a`) and replay it to skip the
LLM. Its documented recovery on a broken selector is **self-heal = re-run `page.act`**,
a fresh LLM call. An absolute XPath is positional, so any structural re-render
invalidates it. Therefore 3.3d's peer re-ground count = **Stagehand self-heal LLM calls
on the identical action sequence** (one re-ground per cached-selector break per
re-render); the token-volume axis stays Playwright-MCP. Against this baseline,
anchortree's `rebinds_zero_llm` is the count of LLM calls the peer pays and anchortree
does not.

**Why this shape.** It turns the thesis into one defensible number sourced from a
signal the engine already produces, with the inflation traps named up front, and a
concrete peer to measure against rather than a hand-waved "naive agent." Sources:
anchortree `diff.rs:37`, `identity.rs:213-258`/`:251`; Stagehand caching guide + the
self-heal recovery (github.com/browserbase/stagehand
`packages/docs/v2/best-practices/caching.mdx`, commit `#2253`).

## D29 — Phase 3.3d dual real-peer baseline stays HERMETIC: two offline peer models, and the rebind count is NOT the Stagehand self-heal count (CONFIRMED, builder run 22)

**Status: CONFIRMED (builder run 22).** Shipped as `anchortree-core::peer`: the
Playwright-MCP token model (`playwright_snapshot` + `snapshot_tokens`, priced with the
engine's own `estimated_tokens`), the Stagehand self-heal model (`DomPositions` +
`StagehandCache`, an absolute-XPath resolver that is decidedly NOT a reuse of
`rebinds_zero_llm`), and `BaselineReport` pairing both axes. `tests/peer.rs` proves the
nuance against the real `IdentityMap`: a turn with 3 engine rebinds and 0 peer
self-heals (in-place re-render) and a turn with 0 rebinds and 3 self-heals
(sibling-insert), grand totals 6 vs 3 — they cannot coincide if one were a proxy for
the other. The baseline stays fully hermetic; no live Stagehand/Node/OpenAI/Playwright
server was added. The original proposal follows.

**Status when proposed: PROPOSED (builder confirms when 3.3d lands).** Phase 3.3c shipped the
anchortree-side headline (builder run 21, `246244a`: `RegroundLedger`, tested zero-LLM,
`task_headline`). 3.3d adds the *peer* side of the comparison. The whole 3.3 arc has
held its value by staying hermetic (3.3a recorder, 3.3b offline replay, 3.3c pure
metric); 3.3d should not break that by standing up live Stagehand/Node/OpenAI or a live
Playwright-MCP server. Instead, replay the **same** captured observe/mutation sequence
the engine already consumes through two cheap offline peer *models*, scored with the
engine's own tokenizer.

**1. Token-volume axis — the Playwright-MCP model.** Playwright-MCP returns the *full*
accessibility snapshot per tool response (`--snapshot-mode` default `full`; its README
concedes "verbose accessibility trees" are the token cost it routes around via a CLI).
So the peer model is: per observe, tokenize the *whole* snapshot with
`budget::estimated_tokens` and compare to anchortree's per-turn `budget::diff_tokens(&diff)`.
Both sides use the identical `ceil(chars/3.5)` ruler (`budget.rs`), so the ratio is
apples-to-apples and fully offline. Headline: full-snapshot tokens/turn vs diff
tokens/turn (anchortree's `DIFF_BUDGET` is 800; peer dumps run 15K–35K per the
`budget.rs` field citation).

**2. LLM-re-ground axis — the Stagehand model is an absolute-XPath resolver, NOT a
reuse of `rebinds_zero_llm`.** This is the load-bearing nuance. It is tempting to claim
anchortree's rebind count equals the Stagehand self-heal count, but the two are not
identical:
  - Engine Path 2 (`diff.rebound`) fires on a `backendNodeId` change.
  - An absolute XPath (`/html/body/div[1]/div[1]/a`, Stagehand's cached selector form)
    can **survive** a backendNodeId change — a framework can replace a node in place at
    the same DOM position — and can **break** *without* one — a sibling inserted above
    shifts every positional index while the backendNodeId is preserved (engine Path 1
    `changed`).
  So `rebinds_zero_llm` is neither an upper nor a lower bound on Stagehand self-heals in
  general. To get the *real* peer number, 3.3d must record each acted element's absolute
  XPath at bind time and, after each re-render, check whether that XPath still resolves
  to the same logical node; **each miss = one Stagehand self-heal `page.act` LLM call**
  (Stagehand's documented recovery). The resolver is pure and deterministic over the
  captured DOM sequence — no browser, no model. Counting `rebinds_zero_llm` as the
  peer's self-heal number would be an over-claim and must not ship in the report.

**3. Scope the first cut.** Keep one RETRIEVE task (task 21, already the 3.3b/3.3c
target) as the first baseline so 3.3d produces a single deterministic pair of numbers
(peer tokens/turn + peer self-heals) against anchortree's (diff tokens/turn + 0
re-grounds) before the multi-task loop (3.3e over the 258-task subset).

**Why this shape.** It preserves the hermetic discipline that made the whole 3.3 arc
land cleanly, it reuses the tokenizer the engine already trusts, and it names the one
over-claim trap (rebind ≠ self-heal) up front so the published headline survives a
hostile read. Sources: playwright-mcp README `--snapshot-mode`/"verbose accessibility
trees" (github.com/microsoft/playwright-mcp); anchortree `budget.rs`
(`estimated_tokens`/`diff_tokens`/budgets/dump citation), `identity.rs:213-258` (the
three-path ladder that makes rebind ≠ XPath-break), Stagehand absolute-XPath + self-heal
(`packages/docs/v2/best-practices/caching.mdx`).

## D30 — Phase 3.3e report scope: two denominators, not one — score over RETRIEVE-only, baseline over every replayable task (CONFIRMED, builder run 23)

**Status: CONFIRMED (builder run 23).** 3.3e shipped as `report.rs` in
`anchortree-cdp`: `Report` + `TaskRecord` aggregate a Hard task set with the two
denominators kept structurally apart, exactly as proposed. `TaskRecord::scored`
carries an `EvalResult` (counts toward N); `TaskRecord::baseline_only` does not
(counts only toward M). Every score-axis method on `Report` divides by N
(`scored_tasks`), every baseline-axis aggregate sums over M (`baselined_tasks`),
and no method crosses the two — the over-claim guard is the type shape, not a
convention. `mean_score` divides the score sum by N even when M > N, pinned by
the `mean_score_divides_by_scored_n_not_baselined_m` unit test and the
`multi_task_hard_report_keeps_two_denominators_apart` integration test (real
task-21 eval + engine-driven baseline-only tasks: mean 1.00 over N=1, 4 rebinds
vs 2 self-heals over M=3). `render()` emits "N scored, M baselined". Remaining
work is data, not engine: capturing each Hard task's replayable observe sequence
to feed the aggregator at full scale.

**Original proposal (research run 21).** 3.3d is done (`f5e7f20`):
the peer comparison is a tested, hermetic baseline at task scope. 3.3e is the
multi-task report — the publishable headline. The substrate is now named: the
"258-task difficulty-prioritized subset" is **WebArena Verified Hard** — 210
single-site + 48 multi-site tasks, a 68.2% runtime cut over full WebArena-Verified
while keeping discriminative power and coverage (ServiceNow; openreview CSIo4D7xBG,
PyPI `webarena-verified` as of 2026-01-07). Naming the published subset removes the
cherry-pick objection: the report runs the *official* Hard set, not a hand-picked one.

**The load-bearing nuance: 3.3e has two different denominators, and conflating them
is the over-claim trap for this phase (the way rebind ≠ self-heal was for 3.3d).**

  - **SCORE axis (RETRIEVE-only).** Per D27 as empirically corrected by builder run 20,
    `AgentResponseEvaluator` RETRIEVE scores from just two artifacts
    (`agent_response.json` + a ≥1-entry `network.har`), no `config.json`. MUTATE and
    NAVIGATE evaluators need `config.json` to resolve URLs/credentials, which the
    offline-replay harness does not stand up. So the honest *scored* denominator is
    **the RETRIEVE-scorable subset of Hard**, not all 258.
  - **BASELINE axis (all replayable).** The token model (`diff_tokens` vs
    `estimated_tokens`) and the two peer counts (rebinds vs XPath self-heals) need only
    a replayable observe/mutation sequence — they never touch the score path. So the
    baseline is computable on **any** Hard task we can replay, RETRIEVE or not.

  Therefore the report must read "**N tasks scored, M tasks baselined**" with N ≤ M,
  and must never divide a baseline aggregate by the scored denominator (or vice versa).
  A single blended "X% on 258 tasks" headline would silently merge a small scored N
  with a large baselined M and would not survive a hostile read.

**Recommended 3.3e shape.** (1) A task loader that filters Hard to the RETRIEVE-scorable
set for the score column and to the replayable set for the baseline columns, reporting
both denominators explicitly. (2) Aggregate `BaselineReport` across the baselined set:
total diff tokens vs total snapshot tokens, total rebinds vs total XPath self-heals,
anchortree re-grounds a structural 0 throughout. (3) The headline is a *pair*: the
score over RETRIEVE (defensible, small) and the token+re-ground ratio over the baseline
set (the thesis number, large). Keep it hermetic — no live peer servers, same as 3.3a–d.

**Why this shape.** It names the phase's one over-claim trap (two denominators) before
the report is written, the same discipline that made 3.3c (assert zero LLM) and 3.3d
(rebind ≠ self-heal) survive scrutiny. The peer landscape is unchanged as of Feb 2026:
Stagehand "self-healing" is still a cache-break → LLM `page.act` re-engagement → re-cache
loop, and no surveyed peer (browser-use, Stagehand, Skyvern, Playwright-MCP) ships a
durable rebind-through-re-render at zero LLM — so the thesis number is still anchortree's
alone to report. Sources: WebArena Verified Hard composition + runtime cut (openreview
CSIo4D7xBG; servicenow.github.io/webarena-verified; PyPI `webarena-verified`); Stagehand
caching/self-heal current as of 2026-02 (skyvern.com browser-use-vs-stagehand;
noqta.tn ai-browser-agents-2026); D27 RETRIEVE two-artifact correction (builder run 20);
`budget.rs`/`metric.rs`/`peer.rs` token + re-ground axes.

## D31 — Phase 3.4 transport-neutral seam abstracts THREE sources, and the BiDi adapter is not a drop-in yet: BiDi has no full-AX-tree dump (CONFIRMED, builder run 24)

**Status: CONFIRMED (builder run 24, `ea6a717`).** Shipped as
`anchortree-cdp/tests/transport_neutrality.rs` (3 fitness-function tests:
core names no CDP type; the cdp CDP-touching file set equals the pinned
`CDP_ADAPTER_FILES`; the fusion path `fuse.rs`/`eval.rs`/`report.rs` is CDP-free)
plus `fuse.rs`'s `pub type TransportNodeKey = i64` opaque per-pass key (CDP fills it
from `backendNodeId`, a BiDi adapter from a `sharedId`-derived int). Transparent alias =
zero call-site churn, matching D31's "seam only" directive. The guard was proven to bite
(injected a `chromiumoxide` ref into `eval.rs`, both relevant tests failed, reverted).
171 tests pass. The BiDi adapter stays deferred per the finding below. Original proposal:

**Status: CONFIRMED (builder run 24).** 3.4 landed exactly as recommended: the seam-only
guard, no half adapter. `tests/transport_neutrality.rs` is a three-test source-scanning
fitness function that pins the CDP code surface to the six transport adapters and asserts
`anchortree-core` plus the `fuse.rs`/`eval.rs`/`report.rs` fusion path are CDP-free — the
hand-grep D9 wanted from Phase 1 is now a build gate. `fuse.rs` gains `pub type
TransportNodeKey = i64`, the opaque per-pass node-identity key at the public seam (CDP fills
it from `backendNodeId`, BiDi would fill it from `sharedId`); as a transparent alias it names
the concept and documents the deferred-adapter story without a wide rename. The module docs
record that `anchortree-bidi` must CONSTRUCT the AX tree (w3c/webdriver-bidi#443 still OPEN as
of 2025-12-12) and is therefore deferred, not built against a moving target. The guard was
proven to bite (injected leak failed two tests, then reverted clean). Original proposal
below.

**Status (original): PROPOSED (builder confirms when 3.4 lands).** 3.3e is done (`3309f82`, D30
CONFIRMED): the report aggregator keeps the two denominators structurally apart. The next
ROADMAP item is 3.4 — the long-standing guard (D9) that `RawAxNode` stay transport-neutral
so an `anchortree-bidi` adapter is a future drop-in, no CDP types past `observer.rs`. This
run verified what "drop-in" actually requires against the live state of WebDriver BiDi, and
the answer reshapes the guard.

**Finding: BiDi today cannot supply the engine's primary input — a full accessibility tree.**
The engine consumes `Accessibility.getFullAXTree` (CDP) in `observer.rs`. WebDriver BiDi has
**no equivalent**. As of 2025-12-12 the W3C issue "Accessibility module in WebDriver BiDi?"
(w3c/webdriver-bidi#443) is still **OPEN** (opened 2023-06). What BiDi ships today is an
accessibility *locator* only — `browsingContext.locateNodes` with an accessibility locator
matching by `role`/`name` — which finds nodes but does not dump the tree with per-node AX
properties. Full internal-AX-property exposure is at the Interop-2025 accessibility
investigation / prototype stage: geckodriver (bugzilla 1929144) and safaridriver (webkit
299508) prototypes plus an in-progress RFC, per maintainer @spectranaut on #443. Not
standardized, not shipped cross-browser.

**Finding: BiDi's node identity is `sharedId`, an opaque session+browsing-context-scoped
reference** (`script.SharedReference`, w3c/webdriver-bidi spec). It is NOT a `backendNodeId`
analogue with the same lifetime semantics, but this does not block us: the identity engine
never relies on the transport node id being durable across a re-render — Path 1 uses it only
as a cheap same-frame soft-match key, and durability is rebuilt by the fingerprint rebind
(Path 2, `identity.rs:213-258`). So `sharedId` is a fine Path-1 key; the transport id being
opaque/non-durable is exactly the case the engine was designed for.

**Therefore the 3.4 seam must abstract THREE sources, not one type:**
  1. **Node-identity key** — CDP `backendNodeId` → BiDi `sharedId`. Already isolated behind
     the engine's eid; the soft-match just needs a transport-supplied opaque key.
  2. **AX-node property source** — CDP reads it from `getFullAXTree`; a BiDi adapter must
     **construct** it (script-injected accessibility walk + DOM), because BiDi has no tree
     dump. This is the real adapter cost, not a type mapping.
  3. **Per-node box model** — CDP `DOM.getBoxModel`; BiDi exposes geometry via
     `script.evaluate` / DOM rects, so this is constructible too.

**Recommendation.** Ship 3.4 as the *seam only* — verify `observer.rs` is the last file that
names a CDP type and that `RawAxNode` carries an opaque `transport_node_key` rather than a
CDP-typed `backendNodeId` — and record in the module docs that the `anchortree-bidi` adapter
is deferred until either (a) BiDi AX exposure lands (track #443), or (b) the constructed-tree
path is specced as its own item. Do NOT build a half BiDi adapter against a moving target.
Add ROADMAP 3.5: capture the 258-task replayable observe corpus offline (the data task 3.3e
flagged out of scope) — that, not 3.4, is the nearer-term unblocker for a full-set headline.

**Why this shape.** The original D9 guard framed BiDi as a clean drop-in once the types were
neutral. That under-described the gap: the hard part is not the node-id type, it is that BiDi
has no AX-tree dump, so the adapter is a tree *constructor*, not a translator. Naming that now
keeps the builder from scoping 3.4 as "swap the types and we're cross-browser." Sources:
w3c/webdriver-bidi#443 (OPEN, last comment 2025-12-12, @spectranaut; geckodriver bugzilla
1929144, safaridriver webkit 299508, Interop-2025 accessibility investigation
web-platform-tests/interop-accessibility#148); WebDriver BiDi spec `script.SharedReference`/
`sharedId` + `browsingContext.locateNodes` accessibility locator (w3.org/TR/webdriver-bidi,
MDN BiDi Modules reference); anchortree `observer.rs` (`getFullAXTree` consumer),
`identity.rs:213-258` (three-path ladder, fingerprint rebuilds durability independent of the
transport id).

## D32 — Phase 3.5 corpus capture: ship 3.5a on the two real fixtures the ServiceNow repo already vendors; defer the full-258 collection to 3.5b (CONFIRMED with a correction, builder run 25)

**Status: CONFIRMED with one load-bearing correction (builder run 25).** 3.5a shipped:
`anchortree-cdp/src/corpus.rs` vendors `corpus/{107,108}` + the Hard list and folds the real
`eval_result.json` verdicts into `Report` via `report_from_corpus`, giving a genuine **N=2**
score aggregate (108 RETRIEVE pass 1.0, 107 NAVIGATE fail 0.0, mean 0.50) — the first
non-task-21, non-synthetic numbers. ServiceNow/webarena-verified is **Apache-2.0**, so the
fixtures are vendored in-repo with attribution (`corpus/README.md`), no download-at-build
needed. 7 unit + 5 integration tests; corpus.rs is CDP-free and pinned in the
transport-neutrality guard.

**The correction — a `network.har` does NOT make a task "baselineable (M)" offline.** The
PROPOSED text below (and the original ROADMAP item) claimed both demo tasks are "scorable (N)
AND baselineable (M)" because they ship a `network.har`, expecting a REAL **N=2/M=2** aggregate.
That is wrong. A HAR is a *network trace* (request/response bodies), not an accessibility
capture; the baseline axis (token model, engine rebinds, peer self-heals) needs a replayed
*observe* sequence — per-turn `getFullAXTree` + DOM + layout the engine can diff — and
`anchortree-cdp` has no offline HTML→AX path (no html-parser dependency, by design; the AX tree
comes from a live browser). So M cannot be derived from a HAR alone. 3.5a therefore ships the
genuinely-real score axis (N=2) and defers M to 3.5b's browser-in-loop capture; a present HAR
is modeled only as the *replayable precondition* (`CorpusTask::is_replayable`). No fabricated
baseline numbers were shipped to hit the planned N=2/M=2 — honest N beats a blended M the
fixtures cannot support. The HARs are git-ignored and fetched on demand (`corpus/fetch-hars.sh`).

**3.5b is now: (a) the browser-in-loop observe capture that fills M (the only path to it, needs
a browser, not a HAR), and (b) growing N toward the 258 Hard ids by vendoring more
`eval_result.json` verdicts.** The 3.5a loader consumes the larger corpus unchanged.

---

_Original PROPOSED text (research run 23), preserved — the "baselineable (M)" claim in it is
superseded by the correction above:_

**Status: PROPOSED (builder confirms when 3.5a lands).** 3.4 is done (`ea6a717`, D31
CONFIRMED): the transport seam is a build gate. The next ROADMAP item is 3.5 — capture the
replayable observe corpus so the 3.3e `Report` runs over real WebArena-Verified tasks instead
of task-21 + synthetic. This run found the cheap path: it needs NO Docker standup and NO agent
run for the first cut.

**Finding: the ServiceNow `webarena-verified` repo ships everything 3.5a needs.**
  - **Real per-task fixtures.** `examples/agent_logs/demo/107/` and `examples/agent_logs/
    demo/108/` each carry the full triple `agent_response.json` + `eval_result.json` +
    `network.har` (confirmed via `gh api .../git/trees/main?recursive=1`). So both tasks are
    **scorable** (the score axis reads `agent_response.json` + `eval_result.json`, the
    RETRIEVE two-artifact path per D27/builder run 20) AND **baselineable** (the engine
    replays `network.har` to observe). These are genuine WebArena-Verified artifacts, not
    synthetic diffs.
  - **The Hard task list is vendored.** `assets/dataset/subsets/webarena-verified-hard.json`
    (2,431 bytes — the 258 task ids) plus `webarena-verified-non-hard.json` and
    `docs/getting_started/hard_subset.md`. No need to re-derive the subset.
  - **Two replay formats exist.** Besides HAR, the repo's tests carry a Playwright-trace
    network format (`tests/assets/playwright-trace.network`,
    `playwright-trace-nav-template.json`). HAR is the format anchortree already records (3.3a)
    and replays, so stay on HAR.

**Finding: the broader corpus has two documented sources for 3.5b** (network-trace replay,
per the WebArena env): a one-time WebArena Docker standup (deterministic-reset images for
shopping/gitlab/reddit/cms/map/wikipedia) OR the ~170 shipped human trajectory recordings.
Both yield a `network.har` per task that replays offline forever. This is data collection,
decoupled from the engine.

**Recommendation.**
  - **3.5a (do first, ~an afternoon):** check the `webarena-verified` LICENSE, then either
    vendor or download-at-build the two demo fixtures + the Hard task list, and wire a corpus
    loader that walks `corpus/<task_id>/{network.har,agent_response.json,eval_result.json}`
    and feeds each into `Report` via `TaskRecord::scored` / `baseline_only`. Output: a REAL
    N=2/M=2 aggregate over genuine WebArena-Verified tasks — the first non-task-21 numbers,
    proving the loader end-to-end before any bulk collection. Keep it hermetic: replay HARs,
    score with the engine's tokenizer, no live services.
  - **3.5b (growth, separate task):** widen toward all 258 Hard tasks from a Docker standup or
    the human trajectories. The 3.5a loader consumes the larger corpus unchanged.
  - **Honesty guard (carries D30):** the published headline is always "proven on the N/M
    actually in the corpus", never "X% on 258" until 3.5b fills it.

**Why this shape.** 3.5 looked like a heavy data task ("stand up six Docker sites, run an
agent over 258 tasks"). It is not, for the first cut: the benchmark authors ship two complete,
real task logs precisely so downstream tools can integrate without the environment. Wiring the
loader against those two now turns the 3.3e aggregator from "tested on synthetic" into "tested
on real WebArena-Verified output" in one small PR, and cleanly separates the engineering (the
loader, owed now) from the data collection (the corpus, grown later). Sources: ServiceNow/
webarena-verified repo tree (`examples/agent_logs/demo/{107,108}/{agent_response,eval_result}.json`
+ `network.har`; `assets/dataset/subsets/webarena-verified-hard.json`, 2,431 B;
`tests/assets/playwright-trace.network`), via `gh api repos/ServiceNow/webarena-verified/
git/trees/main?recursive=1`; WebArena env Docker + ~170 human trajectory recordings + offline
network-trace replay (github.com/web-arena-x/webarena README; webarena.dev paper;
servicenow.github.io/webarena-verified/v1.2.3 Quick Start); D27 RETRIEVE two-artifact scoring
(builder run 20); anchortree `report.rs` `TaskRecord::scored`/`baseline_only` (3.3e).

## D33 — Phase 3.5b M-capture is a two-tier mechanism: a hermetic HAR→chromium fulfill layer (Tier 1, prove on the RETRIEVE task first) and a live Docker standup (Tier 2); the HAR path is record-only today (PROPOSED, research run 24)

**Status: PROPOSED (research run 24).** Decides how 3.5b fills the baseline axis (M = the
per-turn AX + DOM + layout observe sequence the engine diffs), which the run-25 D32 correction
proved a `network.har` cannot produce on its own.

**The code fact this rests on.** Reading the workspace, **there is no HAR replayer — the HAR
path is record-only.** `har.rs` is a `HarRecorder` that consumes CDP network events and emits a
`Har`; nothing calls `Fetch.requestPaused` / `Fetch.fulfillRequest` (grep empty). The recurring
doc phrase "offline HAR replay" had merged two unrelated things: (a) `eval_task.rs:89` — the
*evaluator* reads a HAR to confirm a required network event fired = the SCORE axis (N), no
browser; (b) `webarena_capture.rs` — drives a LIVE chrome + LIVE www over env-var URLs = a live
capture, not a HAR. Neither renders captured pages back into a browser, so M has no offline
source today. This answers the run-23 D32 open question to the builder ("does the engine's HAR
replayer drive a real chromium?"): no — it must be built.

**The two-tier mechanism.**
- **Tier 1 — hermetic HAR→chromium fulfill layer (CI-runnable).** A `Fetch.requestPaused`
  handler matches each request against the corpus task's `network.har` — mirror Playwright's
  `routeFromHAR` matcher: URL + method strict, POST payload strict, ties broken by
  most-matching-headers — and `Fetch.fulfillRequest`s the recorded response, with
  **`notFound = abort`** so an off-trajectory request fails loudly rather than silently
  rendering a wrong page (the D30 honesty guard, carried to the byte). The engine then runs its
  real observe→rebind loop over the replayed DOM and persists the per-turn sequence
  `BaselineReport` needs → a real M, with **zero new dependencies** (Fetch is already a
  chromiumoxide primitive; the HAR data model `HarEntry`/`HarRequest`/`HarResponse` already
  exists). **Prove it on task 108 (RETRIEVE) first**, not 107 (NAVIGATE): RETRIEVE reads data
  off a rendered page, so its HAR captures the GETs that render that page; NAVIGATE/MUTATE is
  exactly where the documented HAR-replay gap bites (microsoft/playwright#18288 server-state
  GET, #28167 state-mutating POST). First honest number from Tier 1 is **M=1 on 108**.
- **Tier 2 — live WebArena-Verified Docker standup (robust, growth).** Deterministic-reset
  images, the `webarena_capture.rs` path already proven for live capture, for tasks whose HAR
  replay hits the dynamic-app gap. Decoupled data work; the 3.5a loader consumes either source
  unchanged.

**Honesty guard (carries D30 + the run-25 D32 correction).** M is reported only for tasks
where the replay (or live run) produced a clean observe sequence; a gap-affected task stays
`is_replayable = true` with M unfilled until Tier 2. Never blend N and M; never "X% on 258"
until the corpus actually holds it.

**Why this shape.** HAR record/replay is mature, standardized tooling (Playwright
`routeFromHAR`, CodeceptJS, Testplane), and its dynamic-app gap is an industry-known property,
not an anchortree defect. Leaning on it for the hermetic tier (and being explicit about its
limit) is more honest than inventing a bespoke replay format; the live-Docker tier covers what
HAR replay structurally cannot. Peer differentiation is unchanged: Stagehand caches the AX tree
+ LLM self-heals, browser-use re-reasons every step, Skyvern is vision-per-step — none rebinds
the same logical eid through a re-render with zero LLM. Sources: anchortree `har.rs`
(record-only) + `observer.rs` + `eval_task.rs:89` + `examples/webarena_capture.rs` +
`corpus/{107,108}`; Playwright `routeFromHAR` + `notFound` semantics (playwright.dev/docs/mock,
/docs/api/class-browsercontext); HAR-replay gap microsoft/playwright#18288 + #28167; CDP Fetch
domain `requestPaused`/`fulfillRequest` (chromedevtools.github.io/devtools-protocol/tot/Fetch);
peer landscape (browserbase/stagehand; browser-use; skyvern.com Feb-2026).
