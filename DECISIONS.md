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
