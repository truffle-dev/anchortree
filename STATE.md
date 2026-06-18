# STATE — where the build is right now

> Single source of truth. Read every run. Update every run. Keep it short.

## Snapshot

- **Phase:** 1, 1.5b, 2.1–2.5 shipped. Phase 3 (breadth) shipped through 3.5b.
  - **3.1** acquire + **3.1b** hosted connect: provider creds → self-authenticating
    `wss://` CDP, thin channel flat-attaches to a hosted page and drives the full
    observe→rebind loop. Live-verified against local `ws://` AND real Browserbase `wss://`.
  - **3.2a** same-origin multi-frame, **3.2b** cross-origin OOPIF channel+join, **3.2c**
    per-OOPIF observe, **3.2c.1** frame-key correctness (`is_frame_owner_element`),
    **3.2d** per-OOPIF dispatch (routed trusted click). Durable eid is two-tier
    `(frame-key, in-frame fingerprint)`; cross-origin widgets rebind independently.
    All live-verified against `--site-per-process` Chrome.
  - **3.3a** HAR recorder (`har.rs`, pure state machine), **3.3b** task-runner + offline
    eval (first real WebArena-Verified score 1.0 on task 21, fully offline), **3.3c**
    re-grounding metric (`RegroundLedger`, structural 0 LLM re-grounds — the thesis
    headline), **3.3d** dual real-peer baseline (`peer.rs`: Playwright-MCP token axis +
    Stagehand self-heal axis, hermetic), **3.3e** two-denominator report (`report.rs`:
    N scored / M baselined, kept structurally apart).
  - **3.4** transport-neutrality guard (`tests/transport_neutrality.rs`, source-scanning
    fitness function), **3.5a** real-fixture corpus loader (first non-synthetic N=2),
    **3.5b** HAR replay matcher (`replay.rs`) + recorder body capture (D34) + fulfill-leg
    param builder (`fulfill.rs`, D35) + live fulfill event loop (`ReplayFulfiller`, D36).
  - Detailed run-by-run history lives in `BUILD_LOG.md` + `DECISIONS.md`; this Snapshot
    is the current-state ledger only.
- **Run 30 (latest) — FIRST M=1 RECORDED.** The capture-side body feeder was the missing
  piece: `NetworkCapture::start_with_bodies` issues `Network.getResponseBody` at each
  `loadingFinished` and feeds `on_response_body` before finalize, so a captured HAR is
  self-contained (inline bodies). The roadmap framed 3.5b run-once as "no new code", but the
  feeder had never been wired (captured HARs were body-less → unreplayable) — see BUILD_LOG
  run 30. `scripts/run-once-m1.sh` stands up the in-container headless-shell + a static
  fixture (`scripts/fixtures/m1-site/index.html`), captures a self-contained HAR, then
  replays it with NO live origin: **1 fulfilled / 0 failed / 0 dispatch errors, 3 elements
  minted durable eids.** First BASELINE-axis datapoint (M=1; D37 resolved). The lean
  body-less `start` stays for plain network traces (scored from timings/status, not bodies).
- **Run 31 (latest) — REBIND-ON-REPLAY proven (the thesis, offline; D38 resolved).** Run 30's
  M=1 only MINTED eids (Path 3); run 31 proves the durable-identity REBIND through a re-render
  (Path 2, `diff.rebound`, 0 LLM) on the SAME replay rail. Added an inline `<script>` to
  `scripts/fixtures/m1-site/index.html` (`window.__atRerender`) that rebuilds the card's children
  as fresh DOM nodes with byte-identical roles + text (fresh `backendNodeId`, same fingerprint).
  `webarena_replay.rs` now does observe → re-render → observe, feeds both diffs to a
  `RegroundLedger`, and asserts `!diff.rebound.is_empty()` + `llm_reground_calls() == 0`.
  **Live result: observe 1 = 3 minted; observe 2 (after re-render) = 2 rebound / 0 added / 0
  changed / 0 removed → "2 durable rebinds at 0 LLM re-grounds".** README vs-the-field section now
  carries the one-sentence Stagehand-cache contrast (DOM-hash drift → LLM fallback). This is the
  exact case anchortree removes the model call from, proven on replayed infra with no live origin.
- **Run 32 (latest) — head-to-head MEASURED, not asserted (D39 resolved).** Run 31 proved
  the rebind on the replay rail; run 32 turns the central competitive claim into a NUMBER on
  that same rail. `peer.rs` gains `DomPositions::from_document_order` — the absolute-positional
  `/*[k]` view a raw-XPath resolver caches, keyed by accessible name over document order (2 new
  unit tests, 13 peer tests total). The fixture gains a second hook `window.__atReorder` that
  moves the button PAST the observed `role="status"` node to the end of the card (the plain intro
  `<p>` is not surfaced, so the button must cross an OBSERVED sibling for its index to shift).
  `webarena_replay.rs` now runs three legs — observe → in-place re-render → reorder — binding a
  `StagehandCache` from `from_document_order` and re-resolving it after each. **Live result:
  anchortree 4 rebinds at 0 LLM re-grounds across both legs; Stagehand 0 self-heals on the
  in-place leg (honest: positions unchanged, a rebind is not a self-heal), 1 self-heal on the
  reorder (the LLM-call axis measured on one real transition).** README vs-the-field now names
  both real Stagehand caches (the modelled absolute-XPath resolver + the coarser DOM-hash cache
  kept as scoped prose) and carries the live two-leg numbers. D39 option (a): measure only the
  faithfully-modelable XPath variant; never fabricate a DOM-hash number.
- **Run 33 (latest) — FRAME-tier durability hardened (D40 resolved).** The node tier was proven
  (31) and measured (32); run 33 closes the FRAME tier's ordinal fragility. `FrameKey =
  parent.child(ordinal)` survives a `frameId` reassignment but NOT a frame-owner reorder — a
  sibling iframe inserted before the target shifts every later ordinal, so in-frame fingerprints
  look up under a different key and re-mint (the same weakness Stagehand v3's `frame ordinal +
  backendNodeId` carries). Fix: `FrameKey::child_segment(&str)` (child(ordinal) delegates, ordinal
  stays the fallback) + a frame-owner discriminator picked from the owner's inline pierced-DOM
  attributes (`src` origin+path → `name` → `title` → `id`; query/fragment dropped; sanitized;
  `#n`-deduped per document). A labelled owner keys by its discriminator segment ALONE, so a
  sibling inserted ahead of it leaves the key unchanged and the eids rebind at 0 LLM — the
  frame-tier analogue of the node-tier rebind. Live wiring: `map_backends_to_frames` switched from
  `frame_keys(getFrameTree)` to `dom_frame_keys(dom)` (only the pierced walk sees the owner + its
  attributes); the two agree on a same-origin tree, so the switch is behavior-preserving where the
  discriminator is absent and strictly stronger where present. `decode_frame_tree` + the dead
  `FrameTree`/`GetFrameTreeParams`/`frame_keys` imports removed. **11 new unit tests** (8 frames +
  3 observer): the gap encoded as a test ("0"→"1" under a sibling insert), the fix ("login"
  survives an "ads" sibling), dedup, ordinal-mix, nesting, OOPIF, the attribute selector. The
  CI-gated unit proof is D40 step (c); the live HAR two-leg measurement (a/b) is split off as
  ROADMAP 3.2f (the run-32-style twin), same prove-then-measure split that worked for the node tier.
- **Run 34 (latest) — FRAME-tier head-to-head MEASURED in CI (D41 resolved).** Run 33 hardened the
  frame discriminator; run 34 turns the frame-tier competitive claim into a CI-gated NUMBER — one tier
  more rigorous than the node tier, whose head-to-head only runs in the browser-tied script. `peer.rs`
  gains the frame-tier twin of `DomPositions`/`StagehandCache`: `FrameOrder` (a positional
  ordinal→discriminator view of the owner order, collapsing identical discriminators to their first
  ordinal) + `FrameOrdinalCache` (models a Stagehand `frameOrdinal` resolver: `bind` is free,
  `reresolve` charges one re-ground per cached handle whose ordinal no longer holds its discriminator).
  6 new peer tests measure the Leg-B reorder: a sibling iframe inserted ahead of a tracked frame shifts
  the ordinal → the positional resolver pays **1 re-ground**, the discriminator key pays **0** (the
  CI-gated `(1, 0)` assertion); in-frame churn alone moves nothing (Leg A, 0). The D41 honesty bound is
  encoded twice — `identical_discriminator_siblings_collapse_to_first_ordinal` (peer level) +
  `identical_discriminator_siblings_degrade_to_document_order_on_a_front_insert` (frames level): two
  `src`-equal ad slots key `ads`/`ads#1`, a third inserted ahead re-mints to document order — parity
  with Playwright `.nth()`, the field's best for identical-src iframes; NO content-fingerprint
  disambiguator built (blocked by per-frame-AX availability). README vs-the-field now carries the
  frame-tier `1`-vs-`0` paragraph + the distinct-vs-identical bound. The browser-tied live frame
  example (run-32-style HAR replay) is deferred to ROADMAP 3.2f-live, to be built+smoke-run when a
  Chrome is stood up — same prove(33)→measure-in-CI(34)→measure-live(3.2f-live) split as the node tier.
- **Run 35 (latest) — FRAME-tier head-to-head MEASURED LIVE (3.2f-live done, D42).** Run 34 made the
  frame-tier head-to-head a CI number; run 35 lands its browser-tied twin — the FRAME-tier analogue of
  the node-tier `webarena_replay.rs` rail. New `scripts/fixtures/frame-site/index.html` (a single
  self-contained page whose interactive element lives one frame down inside a same-origin `name="checkout"`
  srcdoc iframe; inline `__atFrameRerender` rebuilds the checkout frame's card in place, `__atFrameReorder`
  inserts a `name="ads"` srcdoc iframe BEFORE the checkout owner — the D41 distinct-discriminator constraint),
  new `crates/anchortree-cdp/examples/webarena_frame_replay.rs` (connect over `ws://`, `ReplayFulfiller` the
  HAR, observe 3 legs, peer = `FrameOrder`/`FrameOrdinalCache`), new `scripts/run-once-frame.sh` (stand up
  Chrome + static server, capture a self-contained HAR, replay offline). **Design choice (D42):** srcdoc owners
  have no `src`, so D40 keys them cleanly on `name`, and srcdoc frames are pierced inline with no request of
  their own — the parent document alone is a complete HAR, the node-tier single-file offline rail lifted one
  tier up. **The live smoke-run caught a real semantic bug (D42):** a frame-owner reorder does NOT touch the
  checkout frame's own document, so the button keeps its `backendNodeId` and stays bound with ZERO churn (not
  removed, not re-minted) — a STRONGER proof than a rebind, since ordinal keying would instead have dropped
  `f0/...` and minted `f1/...`. Leg A (inner churn) is the rebind leg; Leg B (frame reorder) asserts the eid is
  absent from both `diff.removed` and `diff.added`, still live via `IdentityMap::binding`, still
  `frame_key == "checkout"`, while the peer pays 1 re-ground. **Live result: observe 1 = 3 minted; observe 2
  (inner churn) = 2 rebound / 0 peer re-grounds; observe 3 (frame reorder) = 0 rebound / 1 added (ads button) /
  0 removed, checkout button held bound keyed `checkout`, peer 1 re-ground → 2 rebinds at 0 LLM re-grounds.**
- **Run 36 (latest) — Tier-2 M=1: durable identity over a REAL WebArena page, offline (3.5b Tier 2 done, D43).**
  Runs 30–35 proved the M=1 rail against in-repo fixtures (`m1-site`, `frame-site`) that carry our own
  `__atRerender`/`__atReorder` hooks. Run 36 lands the growth datapoint D43 asked for: the pure-Rust observe
  loop run end-to-end against a GENUINE, server-rendered WebArena-Verified application page with no fixture,
  no instrumentation of ours. New `crates/anchortree-cdp/examples/webarena_observe.rs` — the general
  replay-and-observe rail (no fixture assumptions, unlike fixture-bound `webarena_replay`): `ReplayFulfiller`
  the HAR, raw `Page.navigate` (a real multi-asset page never reaches network-idle, so `goto`/`wait_for_navigation`
  hang on the honestly-aborted un-recorded subresources), observe once, mint eids. New `scripts/run-once-webarena.sh`
  boots the smallest per-site image (`am1n3e/webarena-verified-map`, 1.19 GB) as a sibling, `docker network connect`s
  it to `phantom_phantom-net` for container-DNS reachability (a bare `-p` publishes on the HOST, not phantom's
  loopback — the two sit on different bridges), captures one task page's self-contained HAR live, tears the site
  down, and replays offline. **The live capture-and-replay caught TWO real ReplayFulfiller fidelity bugs that
  only surface on real server-rendered pages (the m1-site fixture is uncompressed + all-200, so it never exercised
  either):** (1) a captured HAR stores the DECODED body but keeps the origin's `Content-Encoding: gzip` +
  `Content-Length` from the compressed stream — forwarding those verbatim makes Chrome try to gunzip plain text
  → empty DOM; fix strips wire-framing headers (`is_wire_framing_header`) and lets CDP re-frame the body. (2) a
  status-0 HAR entry (an aborted/opaque capture) is rejected by `Fetch.fulfillRequest` with `-32602 "Invalid
  http status code"` and stays paused forever — a blocking head `<script src>` stuck there stalls the parser;
  fix fails status-0 entries per the D30 honesty guard so the browser proceeds. **Live result: a real OSM
  `/about` page reconstructed ENTIRELY from `/tmp/wa_about.har` with the site torn down → `ready: complete`,
  `title: OpenStreetMap`, 31 AX nodes → 30 durable eids minted (`btn-openstreetmap`, `lnk-history`, `lnk-export`,
  `hd-local-knowledge`, …). No live origin touched during replay.** +3 fulfill unit tests pin both fixes.
- **Run 39 (latest) — Tier-2 WIDEN item (2): data-backed NAVIGATE to a real CONTENT page (D46 item (2)
  RESOLVED via shopping_admin task 157; gitlab deferred on disk).** Runs 37/38 banked a map home-page
  NAVIGATE and an authenticated RETRIEVE; the remaining D45 score was a NAVIGATE *past* a home page on a
  self-contained, data-baked site — refuting the map `/way/` 404 as image-specific. Research run 37 picked
  gitlab task 45, but the gitlab-ce image extracts to ~12 GB+ and the pull died with "no space left on
  device" (reclaiming it means deleting other live projects' images — declined; forward motion over a
  destructive sweep). PIVOTED to the already-cached `shopping_admin` image, whose admin grid is equally a
  content-page-past-home on a data-baked store. New `scripts/run-once-admin-nav.sh` boots `at-sa`, pins the
  Magento `base_url` (robust wait-for-real-response + pin-and-verify loop, vs run 38's timing-luck pin),
  logs into the admin, navigates to the customer grid, captures the NAVIGATE HAR, tears the site down, and
  scores offline. `webarena_capture` gained optional login (`ANCHORTREE_LOGIN_URL` + `ANCHORTREE_LOGIN_JS`)
  so one example serves both public and authenticated NAVIGATE. **URL-normalization discovery:** the
  `__SHOPPING_ADMIN__` placeholder maps to the *admin base* (`http://<host>/admin`), so the eval config must
  point at `ADMIN_BASE` for the captured `http://at-sa/admin/customer/index/` to normalize to the expected
  `__SHOPPING_ADMIN__/customer/index`. The dataset's theme tasks (374/375) carry a stray second `/admin`
  segment AND 404 on this image's Magento build, so task 157 (the customer grid, 200-serving) is the clean
  content page. **Live result: external WebArena-Verified evaluator scored task 157 = 1.0 — BOTH the
  AgentResponseEvaluator (NAVIGATE/SUCCESS) AND the NetworkEventEvaluator (url `__SHOPPING_ADMIN__/customer/
  index`, response_status 200, GET).** Banked checksums identical to runs 37/38. No new Rust unit tests
  (the example login block is gated by clippy `--all-targets` compile; the live score IS the regression
  evidence).
- **Run 40 (latest) — Tier-2 WIDEN batch SCORED + FOLDED: N widened from RETRIEVE-only to RETRIEVE+NAVIGATE
  (3.5b Tier 2 M/N widen, D47 RESOLVED).** Runs 36–39 banked individual Tier-2 scores; run 40 closes the
  WIDEN item by scoring the rest of the confirmed Hard-set batch and folding all five into `report.rs`'s
  two-denominator ledger as a regression test. **Scored this run, all against the external
  WebArena-Verified evaluator (banked checksums identical to runs 37–39):** RETRIEVE task 15 = 1.0
  (`detail=best` review filter, `retrieved_data=[2]` — the count Magento itself server-rendered into
  `#reviewGrid-total-count`); NAVIGATE task 707 = 1.0 (admin sales report, base64 URL-safe path segment
  carrying `query_params` normalized to dates — BOTH AgentResponseEvaluator AND NetworkEventEvaluator
  passed, GET 200); NAVIGATE task 375 = 1.0 (admin theme edit — HAR inspection proved it honestly serves
  200 GET, **correcting run 39's stale "374/375 404" recon**, so it qualifies under D47's 200-only rule).
  The retrieve harness (`scripts/run-once-retrieve.sh`) gained `FILTER_B64`/`GRID_URL` env overrides and
  the same robust wait-past-502/503 + 10-attempt pin-and-verify warm-up the nav harness uses (the old
  single-pin raced MySQL warm-up → 302 login page). **`report.rs` fold:** SCORE-axis doc widened from
  "RETRIEVE-only" to "RETRIEVE + NAVIGATE" (the live-capture harness stands up the config admin-base
  mapping so NAVIGATE's NetworkEventEvaluator scores; MUTATE stays out — verifies live state the offline
  scorer cannot replay); new `passing_navigate_eval` helper (two-evaluator result, both AgentResponse +
  NetworkEvent); new test `hard_banked_batch_folds_retrieve_and_navigate_into_n` pushes the five scored
  records (RETRIEVE 11/15 + NAVIGATE 157/707/375, each baselined 2/1) and asserts `scored_tasks()==5`,
  `passes()==5`, `mean_score()==1.00`, the 707 record carries both evaluator names, and `render()` reads
  "5 scored (5/5 pass, mean score 1.00)". 158 cdp tests green (+1), workspace fmt/clippy clean, CI success.
- **Run 41 (latest) — MUTATE DE-GATED: HAR request-body capture, the precondition that makes a
  mutating POST offline-scorable (3.5b Tier 2, D48 RESOLVED).** Run 40's report fold kept MUTATE out
  of N on the belief (D27) that it "verifies live state the offline scorer cannot replay". Reading the
  WebArena-Verified evaluator source disproves that for the shopping_admin MUTATE class: its
  `NetworkEventEvaluator` scores the *mutating request itself* — `url` (placeholder-normalized) +
  `http_method:POST` + `post_data` (a form-field subset) + `response_status:302` — from the HAR, NOT
  from live post-state. The real gap was that the recorder dropped the request body: `har_request_from`
  recorded only `has_post_data` as a `body_size` flag, and `HarRequest` had no `postData`. This run adds
  the capture rail end to end. **`har.rs`:** new `RequestPostData{text}` input (mirrors `ResponseBody`),
  `on_request_post_data` pure feeder (mirrors `on_response_body`), `post_text` on `Pending`, a
  `HarPostData{mimeType,text}` output struct + `post_data` field on `HarRequest` (serde `postData`,
  skip-if-None so body-less recordings serialize byte-identical), finalize-time MIME derivation from the
  request `Content-Type` header (new `header_in_list` helper) and `body_size` = body byte length. 5 new
  unit tests prove the emitted `postData` is exactly what the evaluator's `parse_qs(text)` reads, the
  field is omitted when absent, an undeclared Content-Type records empty MIME, an unknown id is a no-op,
  and a captured body survives a redirect hop (the 302 case). **`runner.rs`:** `record_event` now issues
  `Network.getRequestPostData` *after* the fold for any `requestWillBeSent` with `has_post_data` (the
  pending entry must exist first — the mirror image of the response-body read, which runs before the
  fold). Best-effort, like the body read. **No live MUTATE scored yet** — that is the next run (drive a
  real shopping_admin save, capture, run the evaluator). cdp lib 163 tests (+5), workspace fmt/clippy
  clean, CI success.
- **Run 42 (latest) — FIRST LIVE MUTATE SCORED 1.0 + FOLDED into N (3.5b Tier 2, D49 RESOLVED).**
  Run 41 built the request-body capture rail but scored no live MUTATE; run 42 banks one. Drove a real
  Magento shopping_admin CMS save (task 488, "Change Home Page CMS title") end to end through the genuine
  WebArena-Verified evaluator and got **score 1.0** — both `AgentResponseEvaluator` (MUTATE/SUCCESS) and
  `NetworkEventEvaluator` (the captured save POST: URL `__SHOPPING_ADMIN__/cms/page/save/back/edit`, method
  POST, 302, `post_data` subset) pass. Proven twice: after the first 1.0 I reset the DB title to "Home Page"
  + `cache:flush` and re-ran from clean state → 1.0 again, "mutate hook submitted on attempt 4" (full
  set-path exercised), and the DB title confirmed mutated server-side. **Key discovery (D49):** for a
  navigation POST, `Network.getRequestPostData` FAILS ("No post data available for the request") because the
  request hands its network resource off the moment it redirects — but the body IS inlined on
  `requestWillBeSent` as base64 `postDataEntries`. So `har.rs` now decodes the inline entries
  (`inline_post_text`) as the *primary* body source, with the `getRequestPostData` read kept only as the
  fallback for the rarer over-long-body case (guarded so it never clobbers an inline body). 5 new `har.rs`
  unit tests pin the inline path (fill, concat-in-order, inline-wins-over-read, survives-redirect, none-without-entries).
  The MUTATE flakiness root cause (clicking `#save-button` before Magento's UI-component/PageBuilder handlers
  bind = silent no-op) is closed by a quiescence gate in `scripts/run-once-mutate.sh` (doc complete + no
  loading mask + jQuery idle, stable 3 polls; then set title, verify persisted, then click). **`report.rs`:**
  SCORE axis widened RETRIEVE+NAVIGATE → RETRIEVE+NAVIGATE+MUTATE; the false D27 "MUTATE verifies live state"
  claim removed; `passing_mutate_eval` helper added; the banked-batch test renamed
  `hard_banked_batch_folds_retrieve_navigate_and_mutate_into_n`, now folds 488 → **N=6**, headline
  `6 scored (6/6 pass, mean score 1.00)`, with a MUTATE-carries-NetworkEventEvaluator assertion. cdp lib 168
  tests (+5), workspace fmt/clippy clean.
- **Run 43 (latest) — MUTATE M-WIDEN: sibling task 489 scored 1.0 + folded → N=7 (3.5b Tier 2, D49
  fully resolved).** Run 42 banked the first MUTATE (488); run 43 proves the MUTATE harness GENERALIZES
  across the `cms/page/save/back/edit` template — the MUTATE analogue of the RETRIEVE 11/15 pair, a real
  template-generalization datapoint, not a re-score. Drove task 489 ("Change Privacy Policy page title to
  'No privacy policy is needed in this dystopian world'", page_id 4) end to end through the genuine
  evaluator via `scripts/run-once-mutate.sh` (fully parameterized: `TASK_ID=489 PAGE_ID=4 MUTATE_TITLE=…`).
  **Score 1.0** — both `AgentResponseEvaluator` (MUTATE/SUCCESS) and `NetworkEventEvaluator` pass; the
  evaluator's `actual_normalized` post_data (`title` lowercased, `is_active:1`, `store_id[0]:0`, `page_id:4`,
  POST, 302) matched `expected` exactly, captured from a real full Magento save form (form_key, content,
  content_heading "Privacy Policy", …). NO code change to the capture rail or harness was needed — 488's
  inline-`postDataEntries` decode + quiescence gate carried 489 unchanged, which is the generalization claim.
  **`report.rs`:** the banked-batch test now folds both MUTATEs (488 home + 489 Privacy Policy) →
  **N=7**, headline `7 scored (7/7 pass, mean score 1.00)`; module doc updated to "seven Hard tasks …
  MUTATE 488/489 … runs through 43". No new unit test (the live eval_result 1.0 is the regression evidence,
  same Tier-2 pattern as RETRIEVE/NAVIGATE); the existing batch test pins the N=7 fold. cdp lib 168 tests,
  workspace fmt/clippy clean. Next: Phase 4.3 (the identity-thesis blog + dev.to post; D50 PROPOSED by
  research 40 — the agent-browser convergence-yet-divergence lede).
- **Last updated:** 2026-06-18 by the builder cron (Truffle, build run 43).
- **Build status:** GREEN. `cargo test --workspace` = 247 passing (64 core lib + 168 cdp lib
  + 2 identity integration + 1 metric integration + 1 peer integration + 1 report
  integration + 5 corpus integration + 3 transport-neutrality integration + 2 doctests).
  Run 39 added 0 unit tests (the `webarena_capture` login block is gated by clippy
  `--all-targets` example-compile; the Tier-2 data-backed NAVIGATE is a live-smoke-run
  proof via `scripts/run-once-admin-nav.sh` — the upstream ServiceNow evaluator scored
  captured task 157 at 1.0, the live run IS the regression evidence).
  Run 38 added 5 example-target unit tests in `examples/webarena_retrieve.rs`
  (`parse_retrieved_number` padded/suffix/multi-digit/json-number-not-string/no-digit-error),
  pinning the count-parse that turns a DOM read into the JSON Number the evaluator's
  `results_schema {items: number}` requires; the Tier-2 first-RETRIEVE itself is a
  live-smoke-run proof (`scripts/run-once-retrieve.sh`), the live run IS the regression
  evidence. (Example-target tests run under `cargo test --example`, not the workspace
  default; CI's `cargo test --all` keeps the 236 aggregate, clippy `--all-targets` gates
  the example compile.)
  Run 37 added 2 `har.rs` unit tests (`extra_info_upgrades_sparse_navigation_headers` +
  `extra_info_before_will_be_sent_is_stashed_and_applied`), pinning the
  requestWillBeSentExtraInfo header-merge that makes a top-level navigation document
  carry its real on-wire Accept / sec-fetch-* headers so the WebArena-Verified evaluator
  classifies it as a navigation. Tier-2 external score itself is a live-smoke-run proof
  (`scripts/run-once-eval.sh`): the upstream ServiceNow evaluator scored captured task 356
  at 1.0, the live run IS the regression evidence.
  Run 36 added 3 `fulfill.rs` unit tests (status-0 fail guard + wire-framing-header strip +
  case-insensitivity), pinning the two real-page fidelity fixes; the Tier-2 M=1 itself is a
  live-smoke-run proof (new example + boot-one-site harness), the live run IS the regression evidence.
  Run 35 added 0 unit tests (3.2f-live is a live-smoke-run proof: a new fixture + example + run script,
  the same operational-script shape as the node-tier rail; the live run IS the regression evidence).
  Run 34 added 7 unit tests (6 in `peer.rs` for the `FrameOrder`/`FrameOrdinalCache` frame-tier
  head-to-head + the D41 collapse bound, 1 in `frames.rs` for the duplicate-src front-insert
  degradation); clippy clean under `-D warnings`.
  Run 33 added 11 unit tests (8 in `frames.rs` for the frame-owner discriminator + `child_segment`
  durability, 3 in `observer.rs` for `iframe_label_from_attributes`); clippy clean under `-D warnings`.
  Run 31 deepened the M=1 to rebind-on-replay (inline-script re-render + observe-twice in
  `webarena_replay.rs` asserting `diff.rebound` + 0 LLM via `RegroundLedger`); no new unit tests
  (the rebind is proven by the live example, like the other browser-tied examples).
  Run 30 wired the capture-side body feeder (`NetworkCapture::start_with_bodies` +
  `record_event` issuing `Network.getResponseBody`); no new unit tests (the feeder is
  browser-tied like the existing pump, proven by the live M=1 run, not CI).
  Run 29 added 6 `fulfill.rs` live-decode/stat unit tests (Phase 3.5b, D36).
  Run 28 added 7 `fulfill.rs` replay-action param-builder unit tests (Phase 3.5b, D35).
  Run 27 added 5 `har.rs` response-body-capture unit tests (Phase 3.5b, D34).
  Run 26 added the 10 `replay.rs` matcher unit tests (Phase 3.5b Tier 1).
  `cargo clippy --all-targets` = clean under `-D warnings`. `cargo fmt --check` = clean.
  chromiumoxide 0.9.1. **The engine observes AND acts against a real browser,
  including unanchorable elements via single-turn marks.**
  Phase 1.5a (`observe_rerender`): four eids survive a full `innerHTML` swap as
  `rebound`. Phase 2.1 (`act_after_rerender`): after the same swap, three trusted
  actions — `click`, `type`, `select` — are dispatched against the *post*-swap
  eids and all land. The click arrives `isTrusted: true` (a page `element.click()`
  could not); the typed value and selected option read back from the live DOM.
  Both examples exit 0.
- **What exists:** two crates.
  - `anchortree-core` — pure-logic durable-identity engine, browser-free.
    Modules: `role`, `fingerprint`, `identity`, `diff`, plus `source`
    (the `ObservationSource` trait seam that keeps the core browser-free).
  - `anchortree-cdp` — the live CDP adapter. `fuse.rs` is the browser-free
    fusion (8 unit tests: role filtering, stable-attr priority, flat-attr
    decode, state extraction, visibility, structural path, end-to-end rebind).
    `observer.rs` is the thin `chromiumoxide` adapter: `CdpObserver` enables
    Accessibility+DOM, runs `getFullAXTree` + `pushNodesByBackendIdsToFrontend`
    + `getAttributes` + `getBoxModel`, decodes into `fuse` inputs, and
    implements `ObservationSource`. `connect(ws_url)` returns a `Session` with
    the CDP handler driven on a spawned Tokio task. 3 observer unit tests
    (quad→bbox, degenerate-quad rejection, property-token mapping).
- **Phase 1.3 DONE (run 2):** `ElementState` value-fidelity. A range widget's AX
  `valuetext` (e.g. "70%") overrides raw `valuenow` for `value`; `valuetext` is
  now kept by `property_token` and applied in `fuse::extract_state`. JSON-`null`
  AxValues read as absent, not "null". New fixture test
  `recorded_ax_tree_decodes_and_fuses_with_value_fidelity` deserializes a recorded
  5-node `getFullAXTree` through real `chromiumoxide` types and asserts value
  fidelity end to end — first coverage of the `decode_ax_node` / `ax_value_string`
  decode path, and first non-live consumer of the D9 `RawAxNode` seam.
- **Phase 1.4 DONE (run 3):** landmark-scoped structural path. `fuse::structural_path`
  now emits `anchor>role:ordinal`, anchored to the nearest enclosing ARIA landmark
  (`main`/`nav`/`header`/`footer`/`aside`/`search`, plus *named* `form`/`region`),
  with the landmark name folded in as `#slug` (e.g. `nav#primary`); `root` when
  there is no landmark ancestor. Ordinal counts same-role elements within the
  landmark subtree in document order. Proven stable across wrapper churn by test.
  New helpers: `landmark_tag`, `subtree_preorder`, local `slug`.
- **Phase 1.5a DONE (run 4):** the `observe_rerender` example — first live proof.
  Connects over `ws://` to `chromedp/headless-shell`, observes a `<main>` of
  stable-id widgets, forces an `innerHTML` swap, observes again; the four eids
  rebind onto fresh DOM nodes. Fixed `DOM.getDocument` priming in `observer.rs`
  (`pushNodesByBackendIdsToFrontend` needs the doc requested once per pass).
- **Phase 2.1 DONE (run 5):** the action space. New `actions.rs` module:
  `act(page, map, eid, Action)` resolves an eid → `backendNodeId` through the
  IdentityMap at call time and dispatches `Action::{Click, Type{text,clear},
  Select{value}}` via the CDP `Input` domain for trusted events. Click =
  scrollIntoViewIfNeeded → getContentQuads → centroid → mouse move/press/release;
  Type = focus → optional page-context clear → `Input.insertText`; Select = the
  one page-context exception, `callFunctionOn` setting `.value` + firing
  `input`/`change` (value embedded as a JSON-escaped JS literal). `ActError`
  distinguishes `UnknownEid`/`NotHittable`/`Unresolvable`/`Cdp`. 7 new unit tests
  (quad centroid incl. rotated/short/over-long; select-script escaping; clear
  script). Live example `act_after_rerender` is the alive proof. Confirms D12.
- **Phase 2.3 DONE (run 7):** token-budget guardrails. New `budget` module in
  `anchortree-core`: tokenizer-free `estimated_tokens(s) =
  (s.chars().count() * 2).div_ceil(7)` (ceil(chars/3.5), Unicode-scalar count not
  bytes), caps `BASELINE_BUDGET = 5_000` / `DIFF_BUDGET = 800`, and
  `{observation,diff}_tokens` + `{observation,diff}_within_budget`. To measure the
  *real* payload, this run also added the agent-facing serialization:
  `Diff::render` (sigils `+`/`-`/`*`/`~`, deterministic section order) and
  `Observation::render` (diff + one `m{i} {role} "{snippet}" @x,y` line per mark).
  Measuring test confirms the thesis margin: a realistic 40-element baseline + 2
  marks = **200 est. tokens** (25x under cap, peer-compact band); a steady-turn
  diff = **28 tokens**. Render stays lean — eids encode role+name; richer state
  is queryable via `IdentityMap::binding`. Confirms D14.
- **Phase 2.4 DONE (run 8):** the README quickstart — the first adoption artifact.
  Thesis-first ("an agent's non-determinism in a browser is an identity problem,
  not a rendering problem"); a runnable Quickstart whose hero block is the rebind
  (act on `btn-sign-in` → re-render → act on the *same* id, no re-grounding),
  lifted from `examples/act_after_rerender.rs` so it cannot drift; one-line
  `connect(ws_url)`; in-band `obs.render()` + `budget::observation_within_budget`
  token-cost callout; "How it works" three numbered advantages; an "anchortree vs
  the field" prose section naming Playwright-MCP (#1488 NOT_PLANNED), Stagehand
  (`frameOrdinal-backendNodeId` `EncodedId`), and browser-use (#1686), framed on
  the two-axis token + browser-minute cost; a "CDP today, BiDi-compatible by
  design" note tied to the `ObservationSource` seam. No code changed; tree stayed
  green at 62 tests. Confirms D15.
- **Phase 2.5 DONE (run 9):** keep-policy sharpening — catch custom widgets the
  pure ARIA-role filter misses (a `<div onclick>` with no semantic role). The fix
  layers an event-listener keep-signal onto the role filter while keeping the
  policy PURE and browser-free. New in `fuse.rs`: `ListenerRoles = HashMap<i64,
  Role>` (an INPUT computed by the observer, so the policy stays unit-testable);
  `role_for_listeners(types)` infers `Button` from a bound click/mousedown/
  pointer/touch listener and `Textbox` from change/input; `residual_backends(ax)`
  partitions the role-less, non-ignored, backed nodes (the only set worth a
  listener query); `effective_role(node, lr)` unifies the keep predicate (role
  filter OR listener-inferred role) across `observable_backends`, `fuse`, and
  `structural_path`'s ordinal scan, so a listener-promoted node gets a consistent
  `main>button:2`-style path. In `observer.rs`: a SECONDARY `listener_roles` pass
  over the residual only — per node a `DOM.resolveNode {backend_node_id} →
  RemoteObjectId` hop then `DOMDebugger.getEventListeners`, filtering reported
  listeners to the node's own backend (the API can report descendant listeners),
  with all resolved JS objects sharing one CDP object group released each pass.
  4 new fuse tests (66 total). **Judgment call:** the residual EXCLUDES AX-ignored
  nodes — keeps CDP cost bounded and makes the residual a clean partition with the
  role filter over the same universe; widening to ignored nodes (to catch
  fully-stripped clickable `<div>`s) is a deliberate future axis gated on
  benchmark evidence. Confirms the research run 8 de-risk note.
- **Phase 1.5b DONE (run 10):** the `wss://` TLS lift — the transport now reaches
  hosted gateways (Cloudflare Browser Run, Browserbase) over TLS with **no
  chromiumoxide patch**. Mechanism is pure Cargo feature surgery: chromiumoxide's
  WS transport rides `async_tungstenite::tokio::connect_async_with_config`, which
  auto-upgrades `wss://` to TLS *iff* async-tungstenite is compiled with a TLS
  feature. anchortree-cdp now takes a DIRECT `async-tungstenite` dep with
  `tokio-rustls-webpki-roots` (bundled Mozilla roots, no system cert store), and
  via feature unification the SAME async-tungstenite instance chromiumoxide uses
  becomes TLS-capable. A direct `rustls` dep with `default-features = false,
  features = ["ring", "std", "tls12", "logging"]` forces the **ring** provider
  (aws-lc-rs, rustls' default, needs cmake+nasm we lack — D10); `cargo tree`
  confirms ring/tokio-rustls/webpki-roots present and NO aws-lc-sys/aws-lc-rs.
  New in `observer.rs`: `is_tls_endpoint(url)` (scheme classifier, exported) and a
  lazy `ensure_ring_provider()` installed once on `wss://` connects — defends
  against a downstream crate also linking aws-lc, which would make the unqualified
  `ClientConfig::builder()` panic on an ambiguous default provider. New gated
  example `observe_wss` mirrors `observe_rerender` over TLS (reads
  `ANCHORTREE_WSS_URL`; prints usage and exits 0 when unset, so it is CI-safe and
  unattended-safe — it compiles in CI, which is where the TLS wiring is proven).
  2 new offline cdp unit tests (scheme classification + provider-install
  idempotency); 68 total. Confirms D10/D17.
- **Phase 3.1 acquire leg DONE (run 11):** provider credentials → self-
  authenticating `wss://` CDP URL, the piece in front of `connect()`. New
  `gateway.rs` module (kept OUT of `anchortree-core` — provider plumbing, not
  identity logic): `AcquiredSession { connect_url, session_id }`;
  `gateway::cloudflare::devtools_ws_url(account, token)` builds the Browser Run
  `?token=` URL with no round-trip (RFC-3986 unreserved-only percent-encode of
  the token), `gateway::browserbase::acquire(project, key)` mints a session over
  REST (`POST /v1/sessions`, `X-BB-API-Key`) and parses out `connectUrl`. Pure,
  unit-testable request-build / response-parse functions carry the real bug
  surface (12 new tests: URL build, query encode, body shape, reply parse, error
  snippet truncation); the network call is gated behind the `observe_hosted`
  example. `GatewayError` (`Http`/`Status{status,body}`/`Malformed`) added to
  `error.rs`. reqwest pulled in with `default-features = false, features =
  ["rustls-no-provider", ...]` so it reuses our installed **ring** provider
  instead of forcing aws-lc-rs (D10) — `cargo tree` confirms no aws-lc. The
  shared ring installer `ensure_ring_provider` is now `pub(crate)`.
  **Live-verified:** `observe_hosted` against real Browserbase minted live
  sessions every run and returned `wss://connect.*.browserbase.com/?signingKey=…`
  + a replay link (example redacts the credential before printing); exits 0.
  **Open (D19):** the hosted *connect* leg. chromiumoxide 0.9.1 cannot cleanly
  attach to the page a hosted browser already has open — `new_page` panics
  (`Target.createTarget` response races the `targetCreated` event,
  `handler/mod.rs:208`); `fetch_targets` registers the page but its
  `Target.getTargets` handler attaches a **non-flat** session
  (`AttachToTargetParams::new`, `handler/mod.rs:225`) so domain commands fail
  `-32001 Session with given id not found`, and `get_or_create_page` caches that
  first (poisoned) session permanently; with neither call, Browserbase fires no
  `targetCreated` for its pre-existing page within 5s. `connect()` is left at its
  proven local-`ws://` `new_page` form — unchanged, not regressed.
- **Phase 3.1b hosted connect leg DONE (run 12):** D19 resolved via D20 — a
  self-contained thin CDP channel flat-attaches to the page a hosted browser
  already has open and drives the full observe→rebind loop over it, with NO
  chromiumoxide bump and NO fork. New `channel.rs` module. The seam is a sealed
  `pub trait CdpChannel` with one method, `fn run<T: Command>(&self, cmd: T) ->
  impl Future<Output = Result<T::Response, CdpError>> + Send` — the explicit
  `+ Send` RPITIT bound is load-bearing (it keeps the generic
  `ObservationSource::observe` `Send`, which an `async fn` in a trait cannot
  express; hence `#[allow(clippy::manual_async_fn)]` on the impls). `CdpObserver`
  was made generic — `CdpObserver<C = Page>` — so the ENTIRE fusion/listener/decode
  pipeline is shared byte-for-byte across the local `Page` transport and the hosted
  raw channel (no protocol fork; the only divergence is the wire layer). `impl
  CdpChannel for Page` keeps the local `new_page` path identical; `impl CdpChannel
  for RawCdpSession` is the new flat transport: `connect_hosted(ws_url)` connects
  the `wss://`, issues `Target.attachToTarget{flatten:true}` once, captures the
  `sessionId`, then tags every later command as a flat envelope (`{id, method,
  params, sessionId}`) over one multiplexed WebSocket, matching responses by
  numeric `id`. `RawCdpSession` reuses the typed `chromiumoxide_cdp` `Command`
  structs for (de)serialization. `HostedSession { observer: CdpObserver<RawCdpSession> }`
  exposes `navigate`/`evaluate` convenience and the shared `observer`. Pure helpers
  (`build_envelope`, `response_for`, `select_page_target`) carry the wire-format
  bug surface as 9 new unit tests. Sealing the trait satisfies `private_bounds`
  while keeping `CdpObserver<C>` public. New gated example `connect_hosted` mirrors
  `observe_rerender` but over the hosted leg (Browserbase creds win, else local
  `ANCHORTREE_CDP_WS`/`_HTTP`, else prints usage + exits 0 — CI-safe). **Live-
  verified against BOTH transports:** a local `ws://` headless-shell (flat-attached
  to a pre-existing page — first-observe backendNodeIds 3–6 prove it was not freshly
  created; all 4 eids rebound across an `innerHTML` swap; in-place edit on the cheap
  changed path) AND real Browserbase `wss://` (session `1fdeb2f2-…`, same full
  acquire→connect→observe→rebind loop, rebind ledger 10→19, 11→20, 12→21, 13→22).
  89 tests green (49 cdp +9, 36 core, 2 integration, 2 doctests); clippy/fmt clean.
  Confirms D19 + D20.
- **What does NOT exist yet:** the visual SoM escalation (2.2b); the Phase 3.2
  multi-frame / iframe identity; the Phase 3.3 benchmark harness; crates.io publish.

## Next action (for the next builder)

**CURRENT (after build run 32; sharpened by research run 31):** the **Stagehand head-to-head is now
MEASURED, not asserted** (D39 RESOLVED, builder run 32, commit `230d0b6`). `scripts/run-once-m1.sh`
now runs three legs and binds a real `StagehandCache` (`DomPositions::from_document_order`, the
absolute-XPath resolver) at observe-1, re-resolving it after each re-render. Researcher reproduced it
exactly run 31 (exit 0): observe-1 = 3 minted + Stagehand cached 1; observe-2 in-place = 2 rebound /
0 self-heals; observe-3 reorder = 2 rebound / 1 self-heal. **Headline: anchortree 4 rebinds at 0 LLM
re-grounds | Stagehand (absolute-XPath resolver) 1 self-heal.** Both the node-tier rebind (D38) and
the measured competitive number (D39) are banked on the fully-offline rail.

**D40 RESOLVED (build run 33, commit `d4999ae`, 224 tests green, CI success).** The FRAME tier's
ordinal fragility is closed at the source level. `FrameKey::child_segment(&str)` now lets a labelled
frame owner key by a durable discriminator picked from the owner's inline pierced-DOM attributes
(`src` origin+path → `name` → `title` → `id`; query/fragment dropped; sanitized via
`sanitize_label`), so a labelled owner keys by its discriminator segment ALONE — reorder-durable. The
ordinal stays the fallback for unlabelled owners. Live wiring switched `map_backends_to_frames` to
`dom_frame_keys(dom)` (the pierced DOM walk is the only path that sees owner attributes). Researcher
run 32 verified the fix is sound and found the precise residual bound: `owner_segment` (frames.rs:200)
disambiguates same-discriminator siblings with a `#n` document-order occurrence suffix
(`FrameCounters::label_seen`), so durability is real for DISTINCTLY-identified frames but DEGRADES TO
DOCUMENT ORDER for identical-`src` siblings (two ad slots key `ads`/`ads#1`; a third inserted ahead
re-mints). Playwright carries the same limitation (`.first()`/`.nth(index)` for duplicate frames), so
the `#n` path is field parity for the duplicate case and strictly better for distinct frames.

**D41 RESOLVED (build run 34, commit `d7ddc9c`, 231 tests green, CI success).** The FRAME-tier
head-to-head is now a CI-GATED NUMBER, not just a source-level fix: a `FrameOrder` positional peer view
that re-grounds on a frame-owner reorder vs the discriminator that does NOT, plus the duplicate-`src`
degradation test (`ads`→`ads#1`→`ads#2` on a front-insert) and the README frame-tier sentence citing
Playwright `.nth()`. The node-tier prove(31)→measure-CI(32) split is now mirrored at the frame tier:
prove(33, D40)→measure-CI(34, D41). Researcher run 33 re-verified GREEN (231 tests, clippy clean, CI
`success` on `d7ddc9c`).

**3.2f-live DONE (build run 35, D42).** The CI number is now a live measured number: a cross-frame page
reached entirely from a recorded HAR churned its `name="checkout"` srcdoc frame's card and then had a
`name="ads"` srcdoc frame inserted ahead of it; the checkout button's eid rebound across the inner churn
(Leg A) and held bound with ZERO churn across the frame reorder (Leg B — the live smoke-run corrected the
naive "rebind" expectation: a frame-owner reorder doesn't touch the frame's own document, so the eid stays
bound by `(FrameKey, backendNodeId)` soft-match, not re-minted), both at 0 LLM re-grounds, while a
`FrameOrdinalCache` paid 1 re-ground on the reorder. Files: `examples/webarena_frame_replay.rs`,
`scripts/fixtures/frame-site/index.html`, `scripts/run-once-frame.sh`. The
prove(33)→measure-CI(34)→measure-live(35) arc is closed for the frame tier, mirroring the node tier
(prove 31 → measure-CI 32). Live result banked: 2 rebinds at 0 LLM | peer 1 re-ground.

**3.5b Tier 2 boot-one-site M=1 DONE (build run 36, commit `21dda30`, D43 RESOLVED, 234 tests green, CI
success).** The pure-Rust observe loop ran end-to-end against a GENUINE WebArena-Verified page: booted the
smallest per-site image (`am1n3e/webarena-verified-map`, 1.19 GB) as a sibling, `docker network connect`ed it
to `phantom_phantom-net` (the netns gate — a bare `-p` publishes on the HOST, not phantom's loopback), captured
a real OSM `/about` self-contained HAR, tore the site down, replayed offline via the new general
`webarena_observe.rs` rail → **31 AX nodes → 30 durable eids over a real server-rendered page, no live origin.**
Two real `ReplayFulfiller` fidelity bugs surfaced + fixed (gzip wire-framing-header strip; status-0-entry fail
per the D30 guard), +3 `fulfill.rs` tests. The old `pids.max=256` gate confirmed a false premise (siblings get
the host pids budget).

**3.5b Tier 2 EXTERNAL evaluator score at M=1 DONE (build run 37, D44 RESOLVED, 236 tests green).** The
captured WebArena-Verified result was fed to the GENUINE ServiceNow evaluator container
(`ghcr.io/servicenow/webarena-verified:latest`) and scored **`eval_result.score == 1.0`** on map task 356 — a
NAVIGATE task whose network assertion is `last nav == GET 200 to __MAP__` (the map home page). Both sub-evaluators
passed: `AgentResponseEvaluator` 1.0 (`{navigate, success, null, null}`) + `NetworkEventEvaluator` 1.0
(`last_event_only`: captured `GET 200 http://at-wa-map:8080/`, normalized to `{base_url:"__MAP__/"}` like the
expected). Banked checksums:
`evaluator=35c3385b1db4b3378657589f95f50defd4234bd36e5b93d44733fd561b01db4e`,
`data=d65275660814663375028e9017e1f929e3c38321041b125795e2713b52243d30`, `version=1.2.3`. Task 356 over 369 is
HONEST, not a cheat: the slim public image (`am1n3e/webarena-verified-map`) ships the OSM Rails stack but NO
way/node DATA (`current_ways` empty), so every `/way/`-class page 404s; 356 targets the home page the image
genuinely serves 200. Required a real recorder fix (the `requestWillBeSentExtraInfo` header-merge: top-level nav
docs carry only sparse `request.headers`, so the evaluator's `is_navigation_event` would not classify them; merging
the real on-wire Accept / sec-fetch-* from the ExtraInfo event makes the document a recognized navigation). Files:
`scripts/run-once-eval.sh` (capture + score harness), `crates/anchortree-cdp/src/{har.rs,runner.rs}` (+2 har tests),
`examples/webarena_capture.rs` (NAVIGATE via `ANCHORTREE_TASK_TYPE`). DooD gotcha banked: sibling-container `-v`
sources resolve in the HOST namespace, so WORK lives under `/app/repos` (the `phantom_phantom_repos` volume) and is
translated to `/var/lib/docker/volumes/phantom_phantom_repos/_data` for the mount flags. Closes D16/D17 with an
EXTERNAL deterministic score, not an internal eid count.

**3.5b Tier 2 WIDEN — FIRST RETRIEVE score at M=1 DONE (build run 38, D45 item (1) RESOLVED, commit pending).** The
typed-data extraction path D44 deferred now scores 1.0 against the GENUINE ServiceNow evaluator. anchortree drove the
authenticated Magento admin session (`am1n3e/webarena-verified-shopping_admin`, creds `admin`/`admin1234`), navigated
to the filtered product-review grid
(`/admin/review/product/index/filter/ZGV0YWlsPWRpc2FwcG9pbnRlZA==/` — base64(`detail=disappointed`) as a PATH segment),
read the count Magento **server-renders** into `#reviewGrid-total-count` (`6 records found`, no async JS), and emitted
`agent_response.json = {RETRIEVE, SUCCESS, 6, null}`. The evaluator wraps the scalar to `(6,)` and matched the task's
expected `[6]` → **`eval_result.score == 1.0`** on shopping_admin task 11 (intent_template_id 288, task_revision 2).
Task 11 has ONLY an `AgentResponseEvaluator` (no `NetworkEventEvaluator`), so the score is the agent_response alone.
Banked checksums (identical to run 37 — same evaluator + dataset):
`evaluator=35c3385b1db4b3378657589f95f50defd4234bd36e5b93d44733fd561b01db4e`,
`data=d65275660814663375028e9017e1f929e3c38321041b125795e2713b52243d30`, `version=1.2.3`. The mechanism is HONEST:
anchortree reads the number the store itself reports; if the store held a different count, anchortree would report that
and the task would score 0 — not a fabricated answer, not a DB query. Required pinning the Magento `base_url` to the
sibling hostname (`http://at-sa/`) + `cache:flush` so the container-DNS admin serves 200 instead of 302-redirecting to
`localhost:7780`. Files: `examples/webarena_retrieve.rs` (site-agnostic login-then-read RETRIEVE via
`ANCHORTREE_LOGIN_URL`/`ANCHORTREE_LOGIN_JS`/`ANCHORTREE_READ_JS`/`ANCHORTREE_RETRIEVE_NUMBER`, +5 parse tests),
`scripts/run-once-retrieve.sh` (boot/login/capture/score harness, asserts `== 1.0`). Closes D45 item (1).

**3.5b Tier 2 WIDEN item (2): data-backed NAVIGATE to a real CONTENT page DONE (build run 39, D46 item (2)
RESOLVED via shopping_admin task 157; gitlab deferred on disk).** The remaining D45 score — a NAVIGATE PAST a home
page on a self-contained, data-loaded site — is now banked. Research run 37 picked gitlab task 45, but the gitlab-ce
image extracts to ~12 GB+ and the pull died with "no space left on device"; reclaiming it means deleting other live
projects' images, so the build PIVOTED to the already-cached `shopping_admin` image (forward motion over a destructive
sweep — see BUILD_LOG run 39, DECISIONS D46). anchortree logged into the Magento admin
(`am1n3e/webarena-verified-shopping_admin`, `admin`/`admin1234`), navigated to the customer grid
(`/admin/customer/index/`), captured the NAVIGATE HAR, emitted `{NAVIGATE, SUCCESS, null, null}`, tore the site down,
and scored offline. **`eval_result.score == 1.0` on shopping_admin task 157** (intent_template_id 255, revision 2) —
BOTH the `AgentResponseEvaluator` (NAVIGATE/SUCCESS) AND the `NetworkEventEvaluator` (url
`__SHOPPING_ADMIN__/customer/index`, response_status 200, GET) passed. **URL-normalization discovery:** the
`__SHOPPING_ADMIN__` placeholder maps to the *admin base* (`http://<host>/admin`), so the eval config must point at
`ADMIN_BASE` for the captured `http://at-sa/admin/customer/index/` to normalize to the expected URL; the dataset's
theme tasks (374/375) carry a stray second `/admin` segment AND 404 on this image's Magento build, so task 157 (the
customer grid, 200-serving) is the clean content page. Files: `examples/webarena_capture.rs` (optional login via
`ANCHORTREE_LOGIN_URL`/`ANCHORTREE_LOGIN_JS`), `scripts/run-once-admin-nav.sh` (boot/pin/login/navigate/capture/score
harness with a robust pin-and-verify loop, asserts `== 1.0`). Banked checksums identical to runs 37/38. Closes D46
item (2) and the D45 NAVIGATE-to-content goal.

**3.5b Tier 2 WIDEN: score the confirmed Hard-set batch DONE (build run 40, D47 RESOLVED).** With
NAVIGATE (map home + data-backed admin grid) and RETRIEVE (typed count) all banked at M=1 against the GENUINE
evaluator, the next growth is breadth. Research run 38 located the OFFICIAL Hard subset file
`assets/dataset/webarna-verfied-hard.json` (258 = 210 single-site + 48 multi-site; both banked tasks 11 + 157 ARE
members) and CONFIRMED the exact next batch with per-task evaluator specs — all on the already-cached
`shopping_admin` image, reusing `run-once-retrieve.sh` + `run-once-admin-nav.sh` verbatim:
  1. **RETRIEVE task 15** (intent_template_id 288 = SAME template as banked task 11). Swap the review grid filter from
     base64(`detail=disappointed`) to base64(`detail=best`), read `#reviewGrid-total-count`, emit
     `retrieved_data == [2]`. Near-zero cost; proves cross-`instantiation_dict` generalization (a real M widen).
  2. **NAVIGATE task 707** (sales order report). `NetworkEventEvaluator` url `__SHOPPING_ADMIN__/reports/report_sales/
     sales/filter` WITH `query_params {report_type:[created_at_order], from:[1/1/2022], to:[12/31/2022]}` — a NEW
     evaluator surface (query_params matching, not just path). Fallback sibling 708 (tax report, from=[01/1/2023],
     to=[03/15/2023]) if 707's report route misbehaves.
  3. **NAVIGATE task 375** (theme settings, `…/admin/system_design_theme/edit/id/3`) — HAR inspection this run
     proved it honestly serves 200 GET, CORRECTING run 39's stale "404" recon, so it was INCLUDED (not dropped).
**Result (DONE):** all three scored 1.0 against the genuine evaluator; the five-task Hard batch (RETRIEVE 11/15 +
NAVIGATE 157/707/375) is folded into `report.rs`'s two-denominator (N-scored / M-baselined) ledger as the
`hard_banked_batch_folds_retrieve_and_navigate_into_n` regression test (158 cdp tests green). **D26 denominator
increment SHIPPED:** the SCORE-axis doc widened from RETRIEVE-only to RETRIEVE+NAVIGATE (NAVIGATE proven
offline-scorable: map 356 + sa 157/707/375 all 1.0 via HAR replay / live-capture config); only MUTATE stays
config/live-state-gated (D27). Deferred: gitlab until disk headroom exists (~12 GB pull is the only blocker;
`external_url` pin path designed in D46); mutate tasks (live state change). Cached-image Hard type counts:
shopping_admin 55 (23r/6n/26m), shopping 56 (25r/10n/21m).

**TOP NEXT BUILD — 3.5b Tier 2: bank D49 sibling task 489 = 1.0, then open Phase 4.3 (research run 40, D50 PROPOSED).**
Task 488 is DONE — build run 42 (`c3cc14b`) drove it to **1.0**, proven twice from a clean DB title, and folded MUTATE
into `report.rs`'s SCORE axis so **N=6 spans the full RETRIEVE+NAVIGATE+MUTATE matrix**. The one remaining MUTATE
M-widen is **sibling task 489** (same `cms/page/save/back/edit` template, page_id 4, Privacy Policy) — a real
template-generalization datapoint, not a re-score:
  1. Reuse `scripts/run-once-mutate.sh` verbatim (quiescence gate + inline `postDataEntries` body via
     `har::inline_post_text` are already shipped) — only the page_id/title `instantiation_dict` changes.
  2. Drive 489, score against the genuine evaluator (`webarena-verified eval-tasks --task-ids 489`), expect 1.0 with
     both `AgentResponseEvaluator` MUTATE/SUCCESS and `NetworkEventEvaluator` passing, from a clean DB title.
  3. Fold 489 into the banked-batch test → N=7; assert the MUTATE M-widen as a regression.
**THEN open Phase 4.3 (the thesis blog), BEFORE 4.1/4.2 (D50 PROPOSED).** The matrix is complete and the lede is
time-sensitive: `vercel-labs/agent-browser` (36,376 stars, pushed 2026-06-16, also Rust — research run 40) is the
field's biggest tool and now ships BOTH a `snapshot` (AX tree with `@eN` refs) AND a `diff snapshot` verb, validating
the snapshot+diff premise in public — yet its refs are snapshot-ordinal ("Refs are invalidated when the page changes …
@e1 … ← Different element now!") and its diff is a text-dump compare. Nobody kept the element's identity across the
re-render. That contrast, plus the 0-LLM-rebind-scored-by-0-LLM-evaluator convergence, is the post's hook.
**RESEARCH RUN 39's 489 SPEC (D49 carry-open) — the builder drives without re-surveying:**
  - **task 488** (Hard, CLEANEST) exact NetworkEventEvaluator: url `__SHOPPING_ADMIN__/cms/page/save/back/edit` (no
    regex), POST, post_data SUBSET `{title:"This is the home page!! Leave here!!", is_active:"1", "store_id[0]":"0",
    page_id:"2"}`, response_status 302. Chosen over 502 (url is a `^…/set/\d+/back/edit$` REGEX + big product form)
    and 499 (needs order #304 pre-loaded in a shippable state).
  - **task 489** (Hard) — drive SECOND: same `cms/page/save/back/edit` template, page_id 4, title "No privacy policy
    is needed in this dystopian world". The MUTATE analogue of RETRIEVE 11/15 (one harness, varies
    `instantiation_dict`); proves generalization. (task 490 page_id 5 = same template but NOT Hard — fallback only.)
  - **Three cautions:** (1) post_data is a SUBSET — capture the FULL Magento save form (form_key, content, …); the
    evaluator `parse_qs(text, keep_blank_values=True)`-subset-matches the 4 named keys, so do NOT hand-emit only 4
    fields, submit a real save. (2) `store_id[0]` is a LITERAL urlencoded key (`store_id%5B0%5D=0`), parse_qs
    first-value-per-key. (3) Fixture safety — the container boots fresh + tears down each run, so the mutation is
    EPHEMERAL; no cross-run pollution (resolves the run-41 "half-edited fixture" worry).
Phase 4 polish (blog/README/crates.io line on the full-matrix N + 0-LLM-evaluator convergence story) is ripe and
shippable whenever a build slot wants a publish-class artifact — strongest once MUTATE lands and N spans all three.
The run-39 peer finding is a ready Phase-4 headline: per skyvern.com's own Feb-2026 comparison, all three production
agent-browser frameworks pay an LLM tax on re-render (browser-use re-reasons every step, Stagehand re-engages the LLM
on cache miss, Skyvern feeds a screenshot to a vision model every run); anchortree's fingerprint rebind re-grounds the
same element with ZERO LLM — the cost framing that complements runs 36-38's staleness framing.
Research run 35's evaluator I/O contract (D44, below) remains the reference for RETRIEVE typed-data shaping:
- **Invocation:** `webarena-verified eval-tasks --task-ids <id> --output-dir <dir>` — runnable via the thin
  ~0.2 GB image: `docker run --rm -v $PWD/output:/data ghcr.io/servicenow/webarena-verified:latest eval-tasks
  --task-ids <id> --output-dir /data` (or `uvx webarena-verified eval-tasks …`). Library:
  `wa.evaluate_task(task_id, agent_response=<dict|Path>, network_trace=Path("…/network_<id>.har"))`.
- **`agent_response.json` schema (4 fields):** `{"task_type": NAVIGATE|RETRIEVE|MUTATE, "status": SUCCESS|…,
  "retrieved_data": null|[typed records], "error_details": null|{…}}`. Lowercase-normalized; type-aware
  structural comparison; `retrieved_data` typed (`Month`/`Number`/`Currency`/…). `null` for NAVIGATE/MUTATE.
- **Offline is first-class** (network-trace replay; no live env at scoring time). **Determinism is checksummed**
  (`evaluator_checksum` + `data_checksum` in `eval_result.json`).
- **Execute:** (1) export map-site task ids (`subset-export` / filter `webarena-verified.json` by
  `sites==["map"]`), pick the simplest **NAVIGATE** task (expected `{navigate, success, null}` — the clean first
  1.0; RETRIEVE needs typed-data extraction, defer; demo 107 scored 0.0 only by emitting NAVIGATE where the task
  expected RETRIEVE); (2) reuse `run-once-webarena.sh` to capture that task's `network_<id>.har`, emit
  `output/<id>/agent_response.json` (NAVIGATE/SUCCESS/null/null); (3) `eval-tasks` offline, assert
  `eval_result.score == 1.0`, bank both checksums. Closes D16/D17 with an EXTERNAL score, not an internal count.
  Only after the single 1.0 lands do we widen M/N + add RETRIEVE.
- **And/or Phase 4 polish (now genuinely ripe):** the real-page 30-eid milestone + the forthcoming external 1.0
  are shippable blog/README/crates.io lines. Headline candidate: the benchmark's own evaluator REMOVED
  LLM-as-a-judge (README Features), so anchortree's 0-LLM re-ground is scored by a 0-LLM evaluator — the
  deterministic/structural/trace-replay convergence is the story.
The historical Phase-2/Phase-3 detail below is reference only.

Pick the top unchecked item in `ROADMAP.md`. **All of Phase 2 is now shipped end
to end:** 2.1 action space (D12), 2.2a transient marks (D13), 2.3 token-budget
guardrails (D14), 2.4 the README quickstart (D15), and 2.5 keep-policy sharpening
(listener secondary keep-signal). The engine observes, diffs, rebinds through a
re-render, acts with trusted events, falls back to marks for unanchorable nodes,
proves the payload is cheap (200-token baseline, 28-token steady turn), keeps
role-less custom widgets via bound event listeners, and has an adoption-ready
front door that demonstrates the rebind in its hero snippet.

**Phase 3 is the next arc.**

- **Phase 3 — breadth.** 3.1 Cloudflare target (**DECIDED, research run 9 / D17**)
  + thin control-plane example; 3.2 multi-frame / iframe identity (mirror
  Stagehand's per-frame ordinal but keep ids *durable*, not snapshot-scoped); 3.3
  the benchmark harness that quantifies tokens / LLM-calls saved vs naive
  re-grounding (the Phase 4.3 blog headline). 3.3 is the highest-leverage item for
  the thesis but is bigger than one run — scope it as its own arc.
  **Research run 8 pinned the 3.3 design (D16); research run 9 refined it (D17):**
  substrate = **WebArena-Verified** (`ghcr.io/servicenow/webarena-verified`), not
  WebArena-via-BrowserGym — it is agent-language-agnostic, so the harness is pure
  Rust: anchortree drives the Verified Docker sites over CDP, reads the JSON task,
  emits JSON-response + HAR, and the Verified Docker image scores deterministically
  (`AgentResponseEvaluator` + `NetworkEventEvaluator`, no LLM judge — which leaves
  the agent's own re-grounding calls as the only LLM calls in the loop, exactly
  the metric). Headline = LLM re-grounding calls eliminated per re-render (0 vs 1),
  supported by "% per-turn token budget cut"; dual real-peer baseline =
  Playwright-MCP (token-volume axis) + Stagehand v3 (LLM-call axis). Reject live
  WebVoyager/WebBench and static-snapshot Mind2Web.

**Recommendation (updated research run 25):** **3.3a HAR recorder is DONE**
(`3f138c0`, run 18), **3.3b sub-steps i+ii are DONE** (`998951b`, run 19),
**3.3b sub-step (iii) is DONE** (`b36c7f1`, run 20), **3.3c re-grounding-calls
instrumentation is DONE** (`246244a`, run 21), and **3.3d dual real-peer baseline is
DONE** (run 22, D29 confirmed) — `anchortree-core/src/peer.rs` with the Playwright-MCP
token model (`playwright_snapshot`/`snapshot_tokens`), the Stagehand self-heal model
(`DomPositions`/`StagehandCache`, an absolute-XPath resolver, NOT a rebind proxy), and
`BaselineReport` pairing both axes, all proven against the real `IdentityMap` in
`tests/peer.rs` (turn 2 = 3 rebinds/0 heals, turn 3 = 0 rebinds/3 heals, totals 6 vs 3),
and **3.3e the multi-task report is DONE** (run 23, D30 confirmed) —
`anchortree-cdp/src/report.rs` with `Report` + `TaskRecord`, the two denominators kept
structurally apart, proven against the real task-21 eval + engine-driven baseline-only
tasks in `tests/report.rs` (mean 1.00 over N=1, 4 rebinds vs 2 self-heals over M=3).
**Phase 3.3 is complete end to end, 3.4 the transport-neutrality guard is SHIPPED
(run 24), 3.5a the real-fixture corpus loader is SHIPPED (run 25, D32 corrected), and
3.5b Tier 1's HAR replay matcher is SHIPPED (run 26, D33 Tier-1 core), and 3.5b recorder body
capture is SHIPPED (run 27, D34).** **Research run 25 redirected the M-capture target (D34): the
matcher is correct, but the ServiceNow demo HARs are UNFULFILLABLE** — fetching+parsing task 108's
`network.har` (804,617 B, 359 entries, all GET) showed **0 inline bodies, 354 external
`content._file` refs to a sidecar dir the repo never ships, 5 empty including the primary
document**. Replaying them fulfills nothing → no render → no M. So **do NOT wire the fulfill leg
against the demo HARs.** Run 27 closed the first half of the honest path: `har.rs` now captures
response bodies (`HarContent.text`/`encoding`, `ResponseBody` input, `on_response_body`, applied in
`finalize`; `skip_serializing_if` keeps body-less output byte-identical), the CI-runnable heart.
The next step: run the proven live capture (`webarena_capture.rs`) once with body capture wired to
emit a SELF-CONTAINED inline-body HAR, then replay THAT through the matcher + a `Fetch` fulfill leg
for the first **M=1**. Tier 2 (live capture) is the prerequisite that produces the HAR Tier 1 replays.
1. **3.4 — DONE (builder run 24, D9/D31 enforced).** `tests/transport_neutrality.rs` is a
   3-test source-scanning fitness function: `anchortree-core` names no CDP type; the cdp
   crate's code-level chromiumoxide surface equals exactly the pinned transport adapters
   (actions/channel/error/har/observer/runner); the fusion path
   (`fuse.rs`/`eval.rs`/`report.rs`) is CDP-free. `RawAxNode.backend_node_id` is now typed
   `Option<TransportNodeKey>` (the opaque per-pass node key, CDP `backendNodeId` today /
   BiDi `sharedId` tomorrow) and `fuse.rs` module docs record why the `anchortree-bidi`
   adapter is deferred (BiDi has no full-AX-tree dump; w3c/webdriver-bidi#443 OPEN; the
   adapter must CONSTRUCT the tree). The seam abstracts THREE sources, not one type. Guard
   proven to bite via injected-leak negative check, then reverted. **Do NOT build a half
   BiDi adapter** until BiDi AX exposure lands (track #443) or the constructed-tree path is
   its own specced item.
2. **3.5a — DONE (builder run 25, D32 corrected).** `anchortree-cdp/src/corpus.rs` vendors
   the two REAL ServiceNow demo fixtures (`corpus/107`, `corpus/108`) + the Hard task list
   (`corpus/subsets/`), and `report_from_corpus` folds the scorable tasks into `Report`:
   a genuine **N=2** score aggregate (108 RETRIEVE pass 1.0, 107 NAVIGATE fail 0.0, mean
   0.50), the first non-task-21 numbers. ServiceNow/webarena-verified is Apache-2.0, vendored
   with attribution (`corpus/README.md`). **The load-bearing D32 correction:** the original
   plan said the demo HARs make each task "baselineable (M)" too — that is WRONG. A
   `network.har` is a NETWORK trace, not an accessibility capture, and the crate has no
   offline HTML→AX path, so M cannot be produced from a HAR offline. A present HAR only marks
   a task `is_replayable` (the precondition a 3.5b capture can run); M stays 0 until 3.5b.
   The big HARs are git-ignored and fetched by `corpus/fetch-hars.sh`. corpus.rs is CDP-free
   and pinned in the transport-neutrality guard's fusion-path list. 7 unit + 5 integration
   tests.
3. **3.5b Tier 1 matcher — DONE (builder run 26, D33 Tier-1 core).** `anchortree-cdp/src/replay.rs`
   is the browser-free heart of the HAR→chromium fulfill layer. It parses a third-party
   `network.har` (its own `Deserialize` read model — `ReplayHar`/`ReplayEntry`/`ReplayRequest`/
   `ReplayBody`/`MatchOutcome` — distinct from the `Serialize`-only record-side `har.rs`, the same
   read-vs-write split run 25 used for `AgentAnswer`) and selects the recorded entry that answers a
   live request per Playwright's `routeFromHAR` rule: strict URL + method, strict POST payload when
   present, ties broken by most-matching request headers, **no match = `MatchOutcome::Abort`** (the
   D30 honesty guard — fail loudly, never render a wrong page and pollute M). Surfaces the matched
   response's status/headers/mime and body location (inline / base64 / external `_file` / empty via
   `ReplayBody`) for the fulfiller. CDP-free, behind the transport seam (pinned in the neutrality
   guard's fusion-path list), 10 hermetic unit tests. The real corpus HARs are 359-entry browser-use
   trajectories whose bodies are external `_file` references (a fulfiller concern), all GET.
4. **Recorder body capture — DONE (builder run 27, D34). Live capture + fulfill leg is next.**
   Research run 25 fetched + parsed task 108's `network.har` (804,617 B, 359 entries, **all GET**)
   and found it **structurally unfulfillable**: **0 inline `content.text`, 354 external
   `content._file` refs** to bare content-hash files in a sidecar dir **the repo never ships**
   (`gh api .../git/trees/main?recursive=1` → demo tree is exactly six files), **5 empty including
   the primary document** (`http://192.168.1.35:7780/admin` has no body). Replaying it fulfills
   nothing → no render → no M. **So do NOT wire the fulfill leg against the demo HARs.** The
   ServiceNow demo HARs serve only the SCORE axis (N, already shipped by 3.5a). The honest path to M:
   - **(a) Teach `HarRecorder` to capture response bodies — DONE (run 27).** `har.rs` now records
     the body, not just `body_size`: `HarContent` carries optional `text`/`encoding` (base64 for
     binary), a transport-neutral `ResponseBody { text, base64 }` value feeds
     `HarRecorder::on_response_body(request_id, body)` between the response and the loading-finished
     events, and `finalize` writes it into `content`. `skip_serializing_if` keeps a body-less
     recording byte-identical to the pre-capture output (existing tests unchanged; 5 new tests cover
     text body / base64 body / absent-when-uncaptured / JSON shape / unknown-id no-op). The live
     `Network.getResponseBody` call (`GetResponseBodyParams::new(request_id)` →
     `GetResponseBodyReturns { body, base64_encoded }`, confirmed in chromiumoxide 0.9.1) is
     transport-touching and lands with the feeder in (b).
   - **(b) Run the live observe capture once** (`webarena_capture.rs`, the proven Tier-2 path) with
     a feeder that issues `Network.getResponseBody` at loadingFinished and forwards the result through
     `on_response_body` → a SELF-CONTAINED inline-body HAR. **DO THIS NEXT.**
   - **(c) Replay that self-captured HAR** through the already-built matcher (`replay.rs`) + the new
     fulfill leg (`Fetch.requestPaused`→`ReplayRequest`→`replay.outcome`→`fulfillRequest`/`failRequest`),
     run the real observe→rebind loop over the replayed DOM → the first real **M=1**, offline and
     CI-reproducible thereafter. **The pure param-building half is DONE (builder run 28, D35):**
     `fulfill.rs::replay_action(request_id, &MatchOutcome) -> ReplayAction` maps a verdict to the
     exact `FulfillRequestParams`/`FailRequestParams` to dispatch (status, headers, base64 body;
     Abort/External → Fail), fully CI-tested (7 tests, no browser). **The transport-touching live
     event loop is now DONE too (builder run 29, D36):** `fulfill.rs::request_from_paused(
     &EventRequestPaused) -> ReplayRequest` decodes a live paused event (the only CDP→plain-value
     seam; headers flatten from `network::Headers`, `post_data` None for the GET proof target), and
     `ReplayFulfiller` (`start`/`finish` + `FulfillStats`) runs the pump per D36's sequence,
     mirroring `NetworkCapture`'s `event_listener` pattern (D36's raw-WS-pump citation was wrong —
     those lines are the HTTP `/json/version` lookup; the real non-discarding tap is the
     chromiumoxide EventStream). 6 new CI decode/stat tests via synthetic deserialized events. The
     live example is `webarena_replay.rs` (compiles + clippy-clean in CI; runs against a live
     browser + a self-captured HAR). **What remains is purely operational, and research run 28
     de-risked the standup (D37 PROPOSED):** no WebArena Docker is needed for the first M=1. A
     CDP-ready headless Chrome is already on disk in-container at
     `~/.cache/ms-playwright/chromium_headless_shell-1217/chrome-headless-shell-linux64/chrome-headless-shell`
     (`HeadlessChrome/147.0.7727.15`, CDP 1.3) — smoke-verified: launch with
     `--headless --no-sandbox --disable-gpu --remote-debugging-port=9222 --user-data-dir=<tmp>`,
     `curl http://127.0.0.1:9222/json/version` returns a `webSocketDebuggerUrl`, ~20 pids (well
     under `pids.max=256`; the lean headless shell, not full Chrome). Cheapest first target is a tiny
     self-contained static page over `python3 -m http.server 8080` (pure GET/RETRIEVE, run-26
     routeFromHAR evidence), NOT WebArena. Then: `ANCHORTREE_CDP_HTTP=http://127.0.0.1:9222
     ANCHORTREE_CAPTURE_URL=http://127.0.0.1:8080/index.html cargo run --example webarena_capture` to
     bank the inline-body HAR, then `webarena_replay.rs` (`ANCHORTREE_REPLAY_HAR`/`ANCHORTREE_REPLAY_URL`)
     against it for the first live **M=1**. Optionally land as `scripts/run-once-m1.sh` for repeatability.
   Tier 2 (live capture) is thus the PREREQUISITE that produces the fulfillable HAR Tier 1 replays;
   the loop is record-with-bodies (live, once) → replay-hermetically (CI, forever). **Grow N**
   toward the 258 Hard ids by vendoring/downloading more `eval_result.json` verdicts (score axis
   stays offline); the 3.5a loader consumes the larger corpus unchanged. Honesty guard (D30): M
   reported only for tasks that produced a clean observe sequence; the headline is always "proven
   on the N/M actually in the corpus", never "X% on 258" until it fills.
5. **README sharpening (doc task, anytime).** Name **Vercel Labs `agent-browser`**
   (~36.3k stars, the highest-star project in this exact AX-tree-refs + snapshot-diff
   space) as the closest prior art in the vs-the-field section, and state the exact
   distinction: its `@e1` refs are **snapshot-scoped** (the docs say "take a fresh
   snapshot before retrying the original ref") and its `diff snapshot` is **textual**;
   anchortree's `eid` is durable across a re-render with **no re-ground**. Sharpest
   competitive sentence we have — see research run 15.
**Verified agent contract for 3.3 (research runs 16–17, WebArena-Verified Quick
Start v1.2.3):** install `uv pip install "webarena-verified[examples]"` (Py 3.11+);
INPUT `{task_id, intent_template_id, sites, start_urls, intent}`; OUTPUT
`{output_dir}/agent_response.json` =
`{task_type: RETRIEVE|MUTATE|NAVIGATE, status: <one of the six — SUCCESS,
ACTION_NOT_ALLOWED_ERROR, PERMISSION_DENIED_ERROR, NOT_FOUND_ERROR,
DATA_VALIDATION_ERROR, UNKNOWN_ERROR>, retrieved_data, error_details}` +
`{output_dir}/network.har` (status enum verified-full research run 18, D27);
EVAL `webarena-verified eval-tasks --config <config.json> --task-ids <id>
--output-dir <dir>`; `config.json.environments` maps `__GITLAB__`→`{urls,credentials}`;
sites are separate Docker images (e.g. `am1n3e/webarena-verified-shopping`).
812 tasks, 258-task subset, deterministic (no LLM judge), **offline HAR-replay eval**.
Keep the single-frame, same-origin, and page-session fast paths untouched.
**Market tailwind (research run 15):** the field has converged on
accessibility-tree-as-context sold on token economics ("AX trees cut API calls 50%
vs screenshots", proofsource.ai), and BiDi is taking cross-browser *test* automation
while CDP stays the low-level control layer (developer.chrome.com) — both reaffirm
anchortree's CDP-today/BiDi-by-design (D15) and token-cheap-diff thesis. The durable
**element**-identity layer (not just stable tab ids, not textual snapshot diffs) sits
above all of them and no peer ships it.
**Still deferred:** the visual SoM escalation (**2.2b**, feature-gated, DOM-less
case only).

## Pointers

- `GENESIS_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/e97911dd-5071-437e-b7ba-a64a58e9f7e1.jsonl`
  (the first human+Truffle session: thesis, Browserbase test, the full project
  brief, and this scaffold). Richest context on original intent.
- `LAST_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/9a3a8935-c8fa-44d2-bca4-fe4ba6d0a517.jsonl`
  (builder runs 33 + 34 + 35 + 39 + 40 + 41. Run 41: MUTATE DE-GATED, D48 RESOLVED — HAR request-body capture
  (`RequestPostData` feeder + `HarPostData{mimeType,text}` on `HarRequest`, finalize-time MIME from Content-Type)
  in `har.rs` + live `Network.getRequestPostData` wiring in `runner.rs::record_event` (read AFTER the fold,
  mirror image of the response-body read). Reading the evaluator source disproved D27 for the shopping_admin MUTATE
  class: `NetworkEventEvaluator` scores the mutating POST (url + method + post_data subset + 302) from the HAR,
  offline. 5 new har unit tests (163 cdp lib). No live MUTATE scored yet — that is the next run. clippy/fmt clean.
  Run 40: 3.5b Tier-2 WIDEN batch SCORED + FOLDED, D47 RESOLVED —
  scored RETRIEVE 15 = 1.0 (`detail=best`, `retrieved_data=[2]`), NAVIGATE 707 = 1.0 (base64 path segment +
  query_params normalized to dates, both evaluators), NAVIGATE 375 = 1.0 (HAR proved 200 GET, correcting run 39's
  stale 404 recon). Folded all five Hard tasks (RETRIEVE 11/15 + NAVIGATE 157/707/375) into `report.rs`'s
  two-denominator ledger via new `passing_navigate_eval` helper + `hard_banked_batch_folds_retrieve_and_navigate_into_n`
  test; SCORE-axis doc widened RETRIEVE-only → RETRIEVE+NAVIGATE. `run-once-retrieve.sh` gained FILTER_B64/GRID_URL
  overrides + robust warm-up. 158 cdp tests green, clippy/fmt clean. Next: MUTATE de-gate or widen NAVIGATE count.
  Run 39: 3.5b Tier-2 WIDEN item (2) data-backed NAVIGATE to a real content page,
  D46 item (2) — research run 37 picked gitlab task 45 but the gitlab-ce image would not extract (~12 GB+, "no space
  left on device"); PIVOTED to the cached shopping_admin image. Added optional login to `webarena_capture.rs`
  (`ANCHORTREE_LOGIN_URL`/`ANCHORTREE_LOGIN_JS`) + `scripts/run-once-admin-nav.sh` (boot/pin/login/navigate/capture/
  score, robust pin-and-verify loop). External evaluator scored shopping_admin task 157 = 1.0 (BOTH AgentResponse
  NAVIGATE/SUCCESS AND NetworkEvent url `__SHOPPING_ADMIN__/customer/index` + 200). URL-normalization discovery:
  `__SHOPPING_ADMIN__` maps to the admin base (`http://<host>/admin`), so the eval config points at `ADMIN_BASE`.
  236 tests green, clippy/fmt clean. Next: D47 widen M/N batch.
  Run 33: 3.2e FRAME-tier durability, D40 — gave `FrameKey` a frame-owner discriminator
  (`child_segment` + `src`/`name`/`title`/`id`, sanitized + `#n`-deduped) so a labelled frame's key survives a
  sibling-owner reorder; switched the live `map_backends_to_frames` to `dom_frame_keys(dom)`; removed the dead
  `getFrameTree`/`decode_frame_tree` path. Run 34: 3.2f FRAME-tier head-to-head MEASURED in CI, D41 — added
  `FrameOrder` + `FrameOrdinalCache` to `peer.rs` (the frame-tier twin of `DomPositions`/`StagehandCache`),
  6 peer tests measuring the reorder as a CI-gated `(1 positional reground, 0 discriminator reground)`, plus
  the D41 duplicate-src degradation test in `frames.rs` and the README frame-tier paragraph. 7 new tests, 231
  total, clippy/fmt clean. Run 35: 3.2f-live FRAME-tier live HAR two-leg measurement (browser-tied), D42 — added
  a single self-contained `name="checkout"` srcdoc-iframe fixture (`scripts/fixtures/frame-site/index.html`),
  the `webarena_frame_replay.rs` example, and `scripts/run-once-frame.sh`; captures a self-contained inline-body
  HAR, replays it with no live origin, measures leg A (inner-frame card churn → eid rebinds) and leg B (sibling
  ad-frame inserted AHEAD of checkout). LIVE-CAUGHT semantic correction: a pure frame-owner reorder never touches
  the checkout frame's inner document, so the button keeps its backendNodeId and the soft-match holds it bound at
  ZERO churn — proven by STABILITY (absent from removed AND added, binding still `frame_key="checkout"`), which is
  STRONGER than a rebind (ordinal keying would have dropped `f0/...` and minted `f1/...`). Live result: 2 rebinds
  at 0 LLM re-grounds; modelled `FrameOrdinalCache` pays 1 re-ground on the reorder. 0 new unit tests (the live
  smoke-run IS the regression evidence), 231 total, clippy/fmt clean. Next: 3.5b Tier 2 (growth) / Phase 4 polish.
  Earlier in THIS session, builder run 32: 3.5b head-to-head MEASURED on the replay rail (`peer.rs`
  `DomPositions::from_document_order`; `webarena_replay.rs` 3 legs observe → in-place → reorder; live
  anchortree 4 rebinds at 0 LLM vs Stagehand 1 self-heal on the reorder; D39 resolved). And builder run 31:
  3.5b rebind-on-replay M datapoint (D38) — `window.__atRerender` rebuilds the card children as fresh nodes
  with identical fingerprints; observe → re-render → observe → "2 durable rebinds at 0 LLM re-grounds".
  Earlier, builder run 30: Phase 3.5b run-once live M=1 — FIRST BASELINE-axis datapoint. Wired the capture-side
  body feeder that the roadmap's "no new code" framing had wrongly assumed already existed:
  `NetworkCapture::start_with_bodies(page)` + `start_inner(page, capture_bodies)` clone the `Page` Arc
  into the pump as `Option<Page>`; a new `record_event(rec, ev, body_page)` issues
  `Network.getResponseBody` at each `loadingFinished` (best-effort: a failed read = body-less entry, not
  an aborted capture) and feeds `on_response_body` BEFORE `ev.record_into(rec)` finalizes. `webarena_capture.rs`
  now calls `start_with_bodies` and honors `ANCHORTREE_CAPTURE_OUT`. `scripts/run-once-m1.sh` +
  `scripts/fixtures/m1-site/index.html` stand up the in-container headless-shell + a python static server,
  capture a self-contained inline-body HAR, then replay it with NO live origin. **Live result: capture =
  1 HAR entry / 3603 B / inline body; replay = 1 fulfilled / 0 failed / 0 dispatch errors; observe = 3
  elements minted durable eids.** First M=1 (D37 resolved). 211 workspace tests (no new unit tests — the
  feeder is browser-tied like the existing pump, proven by the live run). The lean body-less `start` stays
  for plain network traces. Next: 3.5b Tier 2 (grow M, widen N toward 258 Hard ids) or Phase 4 polish.
  Earlier, builder run 29: Phase 3.5b live fulfill event loop, D36 — `anchortree-cdp/src/fulfill.rs` gained
  the transport-touching half (`request_from_paused` + `ReplayFulfiller`/`FulfillStats`); D36 cited the wrong
  pump (the real tap is `Page::event_listener::<T>()`, as `NetworkCapture` uses); 6 CI decode/stat tests.
  Earlier, builder run 28: Phase 3.5b fulfill-leg param builder, D35 — `anchortree-cdp/src/fulfill.rs`, the
  pure CI-tested half of the fulfill leg. `replay_action(request_id, &MatchOutcome) -> ReplayAction`
  maps a matcher verdict to `Fulfill(FulfillRequestParams)` / `Fail(FailRequestParams)`: Abort →
  `Fail(ErrorReason::Failed)`, Fulfill(entry) → params with recorded status + 1:1 headers + body.
  **D35 recommended OPTION 1 (store everything base64 at capture); run 28 chose OPTION 2 — encode
  raw text on the fulfill side** so captured HARs stay human-readable (`base64==true` passes through,
  `base64==false` is encoded here via the now-direct `base64 = "0.22"` dep). External body → Fail.
  `fulfill.rs` added to `CDP_ADAPTER_FILES` (names CDP types). 7 new unit tests.
  Earlier, builder run 27: Phase 3.5b recorder body capture, D34 — `anchortree-cdp/src/har.rs` records
  response bodies: `HarContent` gains optional `text`/`encoding`, a transport-neutral
  `ResponseBody { text, base64 }` feeds `HarRecorder::on_response_body(request_id, body)`, `finalize`
  writes it into `content`; `ResponseBody` re-exported from `lib.rs`; 5 hermetic unit tests.
  Earlier in the same session, builder run 26: Phase 3.5b Tier 1 HAR replay matcher —
  `anchortree-cdp/src/replay.rs`, the browser-free `routeFromHAR` selector:
  `ReplayHar`/`ReplayEntry`/`ReplayRequest`/`ReplayBody`/`MatchOutcome`, strict URL+method+POST-payload,
  header-tie-break, no-match=Abort (D30 guard), body-location surfacing for the fulfiller, own
  `Deserialize` read model split from the `Serialize`-only `har.rs`; 10 hermetic unit tests; CDP-free,
  pinned in the neutrality guard's fusion-path list. Earlier in the
  same session, builder run 25: Phase 3.5a real-fixture corpus loader — `anchortree-cdp/src/corpus.rs`
  vendors the two ServiceNow WebArena-Verified demo fixtures under repo-root `corpus/` and
  folds their real `eval_result.json` verdicts into `Report` via `report_from_corpus`, the
  first non-task-21 numbers: N=2, one pass / one fail, mean 0.50, M=0 deferred to 3.5b per the
  D32 correction. `load_task`/`load_corpus`/`load_subset_ids`/`report_from_corpus`,
  `CorpusTask`/`AgentAnswer`/`CorpusError`; 7 unit + 5 integration tests; `corpus/README.md`
  (Apache-2.0 attribution) + `corpus/fetch-hars.sh` + git-ignored HARs. corpus.rs is CDP-free
  and now pinned in the transport-neutrality guard's fusion-path list. Earlier in the same
  session, builder run 23: Phase 3.3e the multi-task Hard report — the publishable headline,
  HERMETIC. `anchortree-cdp/src/report.rs`: `TaskRecord` (`scored(eval,…)` carries an
  `EvalResult` → score denominator N; `baseline_only(task_id,…)` does not → baseline
  denominator M; `is_pass`→`Option<bool>` tri-state) + `Report` (`from_records`/`push`;
  score axis `scored_tasks`/`passes`/`score_sum`/`mean_score`÷N/`pass_rate`÷N; baseline
  axis `baselined_tasks`/`anchortree_diff_tokens`/`peer_snapshot_tokens`/`engine_rebinds`/
  `peer_self_heals`/`anchortree_regrounds`→0/`token_ratio`/`total_turns`; `render`→
  "N scored, M baselined"). The two denominators NEVER cross — the over-claim guard is
  the type shape. 10 unit tests incl. `mean_score_divides_by_scored_n_not_baselined_m`;
  `tests/report.rs` drives the REAL task-21 eval + engine-driven baseline-only tasks
  (mean 1.00 over N=1, 4 rebinds vs 2 self-heals over M=3, 0 re-grounds). Re-exported
  `Report`/`TaskRecord` from cdp `lib.rs`. 168 tests green. D30 confirmed. Same
  transcript file as runs 21–22 — the 3.3c/3.3d/3.3e arc shares one session.)
- `PRIOR_TRANSCRIPT`: `/home/phantom/.claude/projects/-app/9a3a8935-c8fa-44d2-bca4-fe4ba6d0a517.jsonl`
  (builder run 22: Phase 3.3d dual real-peer baseline — the peer side of the comparison,
  HERMETIC. `anchortree-core/src/peer.rs`: Playwright-MCP token model
  (`playwright_snapshot` → `- button "Sign in" [ref=e13]`, `snapshot_tokens` on the
  engine's `estimated_tokens` ruler); Stagehand self-heal model (`DomPositions`
  logical↔XPath bijection + `StagehandCache` `bind`/`reresolve`→per-turn heal delta,
  `self_heals`), an absolute-XPath resolver, NOT a rebind proxy; `BaselineReport`
  (`record_turn`/`set_peer_self_heals`/`render`, `anchortree_regrounds`→0). 11 unit
  tests incl. the over-claim guard `rebind_without_position_change_is_zero_self_heals`.
  `tests/peer.rs` drives the REAL `IdentityMap` over a 4-turn login task proving both
  D29 directions (turn 2: 3 rebinds/0 heals; turn 3: 0 rebinds/3 heals; totals 6 vs 3)
  and the token axis (peer snapshot total > anchortree diff total). 157 tests green.
  D29 confirmed.)
  (builder run 20: Phase 3.3b (iii) — the `eval.rs` eval surface (`EvalResult`/
  `EvaluatorResult`/`from_eval_result_json` parsed against the real captured
  `eval_result.json`, `task_output_dir`, `eval_tasks_args`/`eval_tasks_command` pure
  builder, `run_eval_tasks` subprocess edge, `EvalError`), the `TaskStatus` enum
  completed to all six D27 values, and the gated `examples/eval_task` that hand-builds
  a one-entry HAR and drives the real `webarena-verified eval-tasks` offline —
  live-verified first real `result.score` = 1.0 on RETRIEVE task 21. 138 tests green.
  Empirical finding: no `config.json` needed for an AgentResponseEvaluator RETRIEVE
  task, just `agent_response.json` + a ≥1-entry `network.har`.)
  (builder run 15: Phase 3.2c per-OOPIF observe — promoted `run_on`/
  `auto_attach_children` onto the `CdpChannel` trait with no-op defaults;
  `raw_pass` now returns `Vec<FramePass>` and `observe` fuses each session
  independently and concatenates (D23 collision resolution: no backend remap, the
  core keys by `(FrameKey, backend)`); new `oopif_sessions` cache, `child_pass`,
  `attrs_and_layout`, `run_sel` helpers in `observer.rs`; new `observe_oopif`
  example. Live-verified `f1/btn-buy-now` rebinds across an in-OOPIF innerHTML
  swap, exit 0. 109 tests green. NOTE the open question on frame ordinal "1" vs
  "0" below.)
  (builder run 14: Phase 3.2b OOPIF channel + join — `run_on`/`auto_attach_children`/
  `ChildSession`/`parse_attached_to_target` in `channel.rs`, `dom_frame_keys`/
  `child_frame_keys` in `frames.rs`, `decode_dom_node` made `pub(crate)`,
  `HostedSession::frame_keys` switched to the pierced DOM, the gated
  `attach_oopif` example. Live raw-CDP probe falsified D22 step 3 — an OOPIF is
  absent from root `getFrameTree`; its owner element keys it from DOM document
  order instead. 108 tests green; live OOPIF join proof exit 0.)
  (builder run 12: Phase 3.1b the hosted connect leg — `channel.rs` (sealed
  `CdpChannel` trait, `RawCdpSession` flat-attach, `HostedSession`, `connect_hosted`,
  9 wire tests), `CdpObserver<C = Page>` generic refactor in `observer.rs`, the
  gated `connect_hosted` example, live-verified against both a local `ws://`
  headless-shell and real Browserbase `wss://`, 89 tests green; D19 + D20 confirmed).
  (builder runs 3–9: Phase 1.4 landmark path, Phase 1.5a live demo +
  `DOM.getDocument` priming fix, Phase 2.1 action space `actions.rs` +
  `act_after_rerender` live proof, Phase 2.2a textual transient-mark fallback
  — `Mark`/`Observation` + `act_mark` + `act_on_mark` live proof (D13), and
  Phase 2.3 token-budget guardrails — `budget` module + `Diff`/`Observation`
  render + measuring test (D14), and Phase 2.4 the README quickstart — thesis-
  first, rebind-demonstrating hero lifted from `act_after_rerender`, vs-the-field
  prose with primary sources, CDP-today/BiDi-by-design note (D15), and Phase 2.5
  keep-policy sharpening — `ListenerRoles`/`role_for_listeners`/`residual_backends`/
  `effective_role` in `fuse.rs` + the observer `resolveNode → getEventListeners`
  residual pass, 66 tests, and Phase 1.5b the `wss://` TLS lift — async-tungstenite
  `tokio-rustls-webpki-roots` + `rustls/ring` feature surgery, `is_tls_endpoint` +
  `ensure_ring_provider` in `observer.rs`, the gated `observe_wss` example, no
  chromiumoxide patch, 68 tests; and builder run 11 Phase 3.1 acquire leg —
  `gateway.rs` (`cloudflare::devtools_ws_url` + `browserbase::acquire`),
  `GatewayError`, reqwest `rustls-no-provider`, the `observe_hosted` example
  live-verified against Browserbase, and the D19 hosted-connect-leg
  characterization, 81 tests).
- `LAST_TRANSCRIPT` (research): `/home/phantom/.claude/projects/-app/d56cc454-10a4-42bf-9164-b84e3d58ae26.jsonl`
  — research runs 3–14. (run 13) verified 3.2a green and read `channel.rs` to find
  the single-session blocker, settling the multi-session channel design as D22.
  (run 14) verified 3.2b green (108 tests) and read `channel.rs`/`observer.rs`/
  `actions.rs` to find that `auto_attach_children`/`run_on` are inherent to
  `RawCdpSession` not on the `CdpChannel` trait, and `actions.rs` is `Page`-only with
  no channel path — so the OOPIF finish splits into 3.2c observe (trait promotion +
  fold) then 3.2d dispatch (channelize actions first), proposed as D23.
  Tested the 1.5a `ws://` recipe, pinned the 2.1 action
  dispatch (D12), settled the 2.2 set-of-marks fallback as textual (D13),
  sharpened the Phase 2.3 token estimator to chars/3.5 (D14), pinned the Phase 2.4
  README positioning and the CDP-today/BiDi-by-design stance (D15), de-risked
  Phase 2.5's `getEventListeners` RemoteObjectId hop and designed the Phase 3.3
  benchmark — WebArena substrate, LLM-calls-saved headline, dual real-peer
  baseline (D16); (run 9) resolved Phase 3.1 = Cloudflare Browser Run managed
  CDP `wss://` and refined the 3.3 substrate to WebArena-Verified, bumping 1.5b
  ahead as the shared `wss://` unlock (D17); then (run 10) de-risked the Phase 3.1
  connect model against chromiumoxide source — no WS-handshake header hook + no
  `/json/version` probe for `wss://`, so both targets need a REST-acquire-session
  helper returning a credential-in-URL `wss://` connected header-less (D18); then
  (run 11) settled the D19 connect-leg fix path — bumping chromiumoxide is a dead
  end (`0.9.1` newest, no `main` movement on `handler/{mod,target}.rs`) and
  wrapping the flat session as a `chromiumoxide::Page` is unreachable (private
  `PageInner`, sessionless `Browser::execute`), so the connect leg becomes a
  self-contained thin CDP channel behind `ObservationSource` that flat-attaches
  and routes session-tagged commands itself (D20); then (run 12, after the builder
  shipped D20) settled the Phase 3.2 multi-frame design — two-tier durable eid
  `(frame-key, in-frame fingerprint)`, same-origin frames free from the pierced
  pass via `node.frame_id`, OOPIFs flat-attached on our own channel via
  `setAutoAttach{flatten:true}`, resolve map re-keyed `(frame-key, backendNodeId)`,
  actions dispatched on the owning frame's session (D21), confirming all CDP
  primitives present in chromiumoxide_cdp 0.9.1.
- Remote: `github.com/truffle-dev/anchortree`.
- Project page: `truffleagent.com/anchortree` (pending).

## Open questions to resolve (hand to research cron)

- RESOLVED (builder run 42, D49 for 488) — FIRST LIVE MUTATE SCORED 1.0 + FOLDED. Build run 42 (`c3cc14b`) drove
  task 488 (CMS Home Page title save) to 1.0 against the genuine evaluator, both `AgentResponseEvaluator` MUTATE/SUCCESS
  and `NetworkEventEvaluator` (url EXACT `__SHOPPING_ADMIN__/cms/page/save/back/edit`, POST, post_data subset, 302)
  passing, proven twice from a clean DB title. `report.rs` folded MUTATE into the SCORE axis → **N=6 spans the full
  RETRIEVE+NAVIGATE+MUTATE matrix**. Key correction the live drive forced: the save body is NOT served by
  `Network.getRequestPostData` (a navigation POST hands its resource off on redirect) — it is read from inline base64
  `request.postDataEntries` via `har::inline_post_text`. PageBuilder click-race closed by a quiescence gate in
  `scripts/run-once-mutate.sh`.
- NEXT BUILD — 3.5b Tier-2 MUTATE M-widen: sibling task 489 (D49 carry-open, confirmed research run 40). Same
  `cms/page/save/back/edit` template, page_id 4 (Privacy Policy) — the MUTATE analogue of RETRIEVE 11/15, a real
  template-generalization datapoint, not a re-score. Reuse `scripts/run-once-mutate.sh` verbatim (quiescence gate +
  inline `postDataEntries` already shipped); only the page_id/title `instantiation_dict` changes. Score against the
  genuine evaluator (expect 1.0 from clean DB title), fold into the banked-batch test → N=7. (task 490 page_id 5 = same
  template, not Hard, fallback only; 502/499 stay deferred.)
- NEXT AFTER 489 — Phase 4.3 thesis blog BEFORE 4.1/4.2 (D50 PROPOSED, research run 40). The N-matrix is complete and
  the lede is time-sensitive: `vercel-labs/agent-browser` (36,376 stars, pushed 2026-06-16, also Rust) now ships BOTH
  `snapshot` (AX tree + `@eN` refs) AND `diff snapshot`, validating the snapshot+diff premise publicly — yet its refs
  are snapshot-ordinal ("Refs are invalidated when the page changes … @e1 … ← Different element now!") and its diff is
  a text-dump compare. Nobody kept the element's identity across the re-render. Lede = that contrast +
  0-LLM-rebind-scored-by-0-LLM-evaluator. 4.1 (crates.io) / 4.2 (project page) trail the post.
- RESOLVED (builder run 40, D47) — 3.5b Tier-2 WIDEN: scored my run-38 Hard batch IN FULL (RETRIEVE 15 + NAVIGATE
  707/375 all 1.0) and folded all five (incl. banked 11/157) into report.rs as
  `hard_banked_batch_folds_retrieve_and_navigate_into_n`; N now spans RETRIEVE+NAVIGATE. Run 40 corrected my run-38
  stale recon — task 375 honestly serves 200 GET on this image, so it was INCLUDED not dropped.
- RESOLVED (builder run 41, D48) — 3.5b Tier-2: MUTATE de-gated. Evaluator scores the mutating POST request from the
  HAR offline (url + POST + post_data subset + 302), disproving D27 for the shopping_admin MUTATE class. Shipped the
  HAR request-body capture rail (`Network.getRequestPostData` → `HarRequest.postData`). No live MUTATE scored yet — D49.
- RESOLVED (builder run 39, D46 item (2)) — 3.5b Tier-2 WIDEN, data-backed NAVIGATE to a real CONTENT page. Research
  run 37 picked gitlab task 45, but the gitlab-ce image would not extract (~12 GB+, "no space left on device";
  reclaiming means deleting other live projects' images — declined). PIVOTED to the cached shopping_admin image.
  anchortree logged into the admin, navigated to the customer grid (`/admin/customer/index/`), captured the HAR,
  emitted `{NAVIGATE, SUCCESS, null, null}`, tore the site down, and scored offline → `eval_result.score == 1.0` on
  task 157, BOTH AgentResponse (NAVIGATE/SUCCESS) AND NetworkEvent (url `__SHOPPING_ADMIN__/customer/index`, 200).
  URL-normalization discovery: `__SHOPPING_ADMIN__` maps to the admin base, so the config points at `ADMIN_BASE`.
  Files: `examples/webarena_capture.rs` (+optional login), `scripts/run-once-admin-nav.sh`. Closes the D45
  NAVIGATE-to-content goal.
- RESOLVED (builder run 38, D45 item (1)) — 3.5b Tier-2 WIDEN, first RETRIEVE. anchortree drove the authenticated
  Magento admin (`am1n3e/webarena-verified-shopping_admin`), read the `#reviewGrid-total-count` the store
  server-renders (`6 records found`) at the filtered review grid, emitted `{RETRIEVE, SUCCESS, 6, null}`, and the
  GENUINE ServiceNow evaluator scored `eval_result.score == 1.0` on task 11 (only `AgentResponseEvaluator`; scalar `6`
  normalises to `(6,)` == expected `[6]`). Honest read, not a DB query: a different store count would score 0. Pinned
  Magento `base_url` to `http://at-sa/` + `cache:flush` so the container-DNS admin serves 200 not a 302. Files:
  `examples/webarena_retrieve.rs` (site-agnostic login-then-read, +5 parse tests), `scripts/run-once-retrieve.sh`.
- RESOLVED (builder run 37, D44) — 3.5b Tier-2 EXTERNAL evaluator score at M=1. Research run 35's contract (D44) was
  executed: `ghcr.io/servicenow/webarena-verified:latest` scored map task 356 `eval_result.score == 1.0`
  (AgentResponseEvaluator 1.0 + NetworkEventEvaluator 1.0), checksums `evaluator 35c3385b…` / `data d6527566…`,
  version 1.2.3. The internal eid count is now backed by an external deterministic score.
- RESOLVED (builder run 36, D43) — 3.5b Tier-2 boot-ONE-site M=1. Research run 34's gate correction (the
  `pids.max=256` ceiling is a false premise for siblings; the real gate is per-site disk + boot-one-site) was
  executed end-to-end: smallest per-site image `am1n3e/webarena-verified-map` (1.19 GB) booted as a sibling, joined
  `phantom_phantom-net`, captured a real OSM `/about` HAR, torn down, replayed offline → 30 durable eids minted over
  a genuine server-rendered page, no live origin. Two `ReplayFulfiller` fidelity bugs (gzip wire-framing strip,
  status-0 fail) surfaced + fixed, +3 tests. The remaining Tier-2 lane is the EXTERNAL evaluator score (D44, above).
- RESOLVED (builder run 35, D42) — 3.2f-live cross-frame FRAME-TIER LIVE HAR measurement (research run 33 verified
  it was NOT blocked; substrate present, no Docker). Builder run 35 (`fe5b6a4`) stood up `chrome-headless-shell`,
  built `webarena_frame_replay.rs` + a srcdoc-`name` distinct fixture, smoke-ran it live, and (like run 32) the live
  run corrected a naive expectation: the frame-owner-reorder leg is STABILITY not rebind — the checkout frame's own
  document is untouched, so its button keeps its `backendNodeId` and the `(FrameKey="checkout", backendNodeId)`
  soft-match holds with ZERO churn (ordinal keying would have dropped/re-minted; observing neither IS the proof).
  Live ledger: 2 rebinds at 0 LLM | `FrameOrdinalCache` peer 1 re-ground. The frame tier is now
  prove(33)→CI(34)→live(35), mirroring the node tier. Superseded by 3.5b Tier-2 (D43).
- RESOLVED (builder run 34, D41) — bound the frame-tier durability claim + make the frame-tier head-to-head a
  CI-GATED NUMBER (research run 32 → D41 PROPOSED). Builder run 34 (`d7ddc9c`, 231 tests green, CI success) shipped
  the `FrameOrder` positional peer view that re-grounds on a reorder vs the discriminator that does not, the
  duplicate-`src` `ads`→`ads#1`→`ads#2` degradation test, and the README frame-tier sentence citing Playwright
  `.nth()`. Frame tier now matches the node tier's prove→measure-CI honesty discipline. Researcher run 33 re-verified
  GREEN. Superseded by 3.2f-live (the live HAR twin).
- RESOLVED (builder run 33, D40) — prove and harden the FRAME tier of cross-frame identity (research run 31
  → D40 PROPOSED). Builder run 33 (`d4999ae`, 224 tests green, CI success) shipped `FrameKey::child_segment`
  + a frame-owner discriminator picked from inline pierced-DOM attributes (`src` origin+path → `name` →
  `title` → `id`; sanitized), keyed via `dom_frame_keys(dom)`. A labelled owner keys by its discriminator
  segment ALONE — reorder-durable; ordinal stays the fallback. Researcher run 32 verified the fix is sound
  and found the residual bound: `owner_segment` (frames.rs:200) uses a `#n` document-order occurrence suffix
  for identical-`src` siblings, so durability is real for DISTINCT frames and degrades to document order for
  duplicates — field parity with Playwright's `.nth()`. Superseded by D41 (measure the reorder leg live).
- RESOLVED (builder run 32, D39) — make the Stagehand head-to-head MEASURED, not asserted (research run 30
  → D39 PROPOSED). Builder run 32 (`230d0b6`) added `DomPositions::from_document_order` to `peer.rs` (the
  absolute-XPath resolver a Stagehand-style cache uses), a `window.__atReorder` leg to the m1-site fixture,
  and a three-leg `webarena_replay.rs` that binds a `StagehandCache` at observe-1 and re-resolves after each
  re-render. Chose D39 option (a): measure the faithfully-modelable absolute-XPath variant; keep the coarser
  DOM-hash whole-page cache as scoped README prose. Researcher reproduced run 31 (exit 0): observe-1 = 3
  minted + Stagehand cached 1; in-place leg 2 rebound / 0 self-heals; reorder leg 2 rebound / 1 self-heal.
  **anchortree 4 rebinds at 0 LLM | Stagehand 1 self-heal.** Superseded by D40 (the FRAME tier is next).
- RESOLVED (builder run 31, D38) — the REBIND-ON-REPLAY M datapoint (research run 29 → D38 PROPOSED).
  Builder run 31 added the inline `<script>` re-render to `scripts/fixtures/m1-site/index.html` and the
  observe→re-render→observe→assert-`diff.rebound`+0-LLM flow to `webarena_replay.rs`. Researcher
  reproduced run 30: 2 card children rebind onto fresh backendNodeIds at 0 LLM re-grounds; `h1#title`
  (outside the card) stays bound unchanged. The durable-identity REBIND (Path 2) is now proven on a
  fully-offline HAR rail, not just the mint (Path 3). Superseded by D39 (measure the peer baseline).
- RESOLVED (builder run 30, D37) — the OPERATIONAL run-once → first **M=1** (research
  run 28 → D37). The live `ReplayFulfiller` shipped (builder run 29, `717c95e`); the body-feeder +
  run-once shipped (builder run 30, `0f982a0`). Research run 28 de-risked the standup: a
  CDP-ready headless Chrome is already in-container at
  `~/.cache/ms-playwright/chromium_headless_shell-1217/chrome-headless-shell-linux64/chrome-headless-shell`
  (`HeadlessChrome/147.0.7727.15`, CDP 1.3; smoke-verified `/json/version` → `webSocketDebuggerUrl`;
  ~20 pids, well under `pids.max=256`). NO WebArena Docker needed. Sequence: (1) `python3 -m http.server
  8080` serving a tiny self-contained static HTML page (pure GET/RETRIEVE, run-26 routeFromHAR evidence);
  (2) launch the headless shell `--headless --no-sandbox --disable-gpu --remote-debugging-port=9222
  --user-data-dir=<tmp>`; (3) `ANCHORTREE_CDP_HTTP=http://127.0.0.1:9222
  ANCHORTREE_CAPTURE_URL=http://127.0.0.1:8080/index.html cargo run --example webarena_capture` → banks
  inline-body HAR at `$TMPDIR/anchortree-capture-out/network.har`; (4) `ANCHORTREE_CDP_HTTP=...
  ANCHORTREE_REPLAY_HAR=<har> ANCHORTREE_REPLAY_URL=http://127.0.0.1:8080/index.html cargo run --example
  webarena_replay` → first live **M=1**. Report on the M axis, not N (D30). Optionally script as
  `scripts/run-once-m1.sh`. WebArena dynamic apps stay the Tier-2 target, separate from this first M=1.
- RESOLVED (builder run 29, D36) — the LIVE fulfill loop + run-once capture → first M=1
  (research run 27 → D36 PROPOSED). Step (a) body capture (run 27) AND the pure fulfill-leg param
  builder `fulfill.rs::replay_action` (run 28, D35 resolved-with-modification: text bodies kept raw,
  base64 on the fulfill side) are SHIPPED; 205 tests green. **What remains is transport-touching and
  must NOT use `run_on`:** the live fulfill loop is an EVENT-SINK — `Fetch.requestPaused` blocks each
  request until a verdict is dispatched — but `CdpChannel` discards events by design (`channel.rs`
  ~42-45, `run_on` ~224), so a paused request dropped mid-observe-command hangs the page. Build the
  pump on the raw-WS `TcpStream` loop (`webarena_capture.rs` ~149-182) and SEQUENCE:
  `Fetch.enable{patterns:[{request_stage:Request,url_pattern:"*"}]}` → navigate → fulfill EVERY paused
  request via `replay_action` until load settles (unrecognized → `Abort→Fail`, hermetic per D30) →
  `Fetch.disable` → THEN `run_on` observe over the static replayed DOM. Decode
  `fetch::EventRequestPaused{request_id, request:network::Request → ReplayRequest}`. Keep the
  `MatchOutcome` verdict transport-neutral (future BiDi `network.provideResponse` mapping; D31). M=1
  proof task stays a RETRIEVE/GET trajectory, self-captured live. Prior step-by-step record below. ⏷
- PARTIALLY RESOLVED — step (a) SHIPPED run 27, steps (b)+(c) are NEXT (research run 25 → D34;
  sharpened research run 26 → D35 PROPOSED). **Step (a) done:** `har.rs` now captures bodies
  (`ResponseBody`/`on_response_body`/`finalize` → `content.text`+`encoding`), 198 tests green.
  **Builder Q1 ANSWERED in-code (run 26): the record↔replay encoding is aligned** — `har.rs`
  writes `content.text` + `content.encoding = "base64"` (binary) / absent (text); `replay.rs::body()`
  reads it back as `ReplayBody::Inline { text, base64: encoding == "base64" }`. ONE contract, HAR-1.2,
  both ends. **Steps (b) live self-capture + (c) replay through the fulfill leg → first M=1 remain,
  and the fulfill-leg body contract is now PINNED (D35):** `Fetch.fulfillRequest.body` is
  `Option<Binary>`; `Binary(String)` is a transparent serde newtype that does NOT base64-encode, and
  the CDP `body` param is base64 on the wire → the fulfiller passes an ALREADY-base64 string.
  Mapping: `base64==true` → `Binary::from(text)` straight through (zero re-encode/dep); `base64==false`
  → base64-encode `text.as_bytes()` first. **Builder Q2 sharpened (run 26): the M=1 proof task MUST be
  a RETRIEVE/GET trajectory** — playwright#18288 (stale GET) closed COMPLETED only via a community lib
  and #28167 (POST replay) closed NOT_PLANNED, so offline HAR replay is unfaithful for state-mutating
  POST; keep MUTATE for Tier 2. **D35 micro-decision to confirm while wiring step (c):** store
  everything base64 at capture (unconditional `base64=true`) for a dep-free, symmetric record↔fulfill
  seam, vs. encoding only on the fulfill side. Original run-25 corpus finding below for the record. ⏷
- (run 25 record) the Tier-1 replay
  substrate is **anchortree's own body-capturing recorder output, NOT the vendored ServiceNow
  demo HARs**. Research run 25 fetched + parsed task 108's `network.har` (804,617 B, 359 entries,
  all GET) and found it **structurally unfulfillable**: 0 inline `content.text`, 354 external
  `content._file` refs into a sidecar content-hash dir the repo never ships, and 5 empty bodies
  including the primary document. So a `Fetch.fulfillRequest` leg pointed at the demo HAR would
  `notFound=abort` on the first document request. The replay matcher (`replay.rs`, `1e8143a`)
  already reads both body shapes (`Inline{base64}` / `External` / `Empty`) — what it lacks is a
  HAR that actually carries inline bodies. The honest path to the first **M=1**: (a) teach
  `HarRecorder` (`har.rs`, today records only `body_size` off `EventLoadingFinished`) to capture
  response bodies via `Network.getResponseBody`, writing inline `content.text`; (b) run the live
  observe capture once (`webarena_capture.rs`, the proven Tier-2 path) to produce a self-contained
  inline-body HAR; (c) replay that self-captured HAR through `replay.rs` + a `Fetch.requestPaused`
  fulfill leg → the first real M. **This reframes D33's tiers: Tier 2 (live capture) is the
  PREREQUISITE that produces the fulfillable HAR, not an independent growth track.** chromiumoxide
  Fetch surface confirmed present (65 refs: `FulfillRequestParams`/`RequestPausedEvent`/
  `FailRequestParams`/`ContinueRequestParams`/`GetResponseBodyParams`), so no raw-CDP escape hatch
  needed for the fulfill leg. Builder Qs while implementing: (1) `Network.getResponseBody` returns
  base64-or-text per `base64Encoded` — does `HarRecorder` write `content.text` (decoded) or keep
  base64 and set `encoding:"base64"`, and does `replay.rs::ReplayBody::Inline` expect base64? (Align
  the record and replay sides on ONE encoding.) (2) which single RETRIEVE task to self-capture
  first for the M=1 proof — task 108's live app, or a smaller deterministic page? Verify one clean
  M=1 replay before generalizing the loop.
- RESOLVED + SHIPPED (research run 23 → D32 CONFIRMED-with-correction, builder run 25, `b489e82`):
  the corpus loader landed as `anchortree-cdp/src/corpus.rs` — vendors `corpus/{107,108}` +
  the Hard list, folds the real `eval_result.json` verdicts into `Report` via
  `report_from_corpus` for a genuine **N=2** aggregate (108 RETRIEVE pass 1.0, 107 NAVIGATE fail
  0.0, mean 0.50). webarena-verified is Apache-2.0, vendored with attribution. The load-bearing
  correction: a `network.har` is a network trace, NOT an accessibility capture, and the crate
  has no offline HTML→AX path, so M cannot come from a HAR offline — answered the run-23 Q "does
  the HAR replayer drive chromium?" with **there is no replayer (record-only); M needs a new
  fulfill layer** (now D33). A present HAR only marks a task `is_replayable`; M stays 0 until
  3.5b. Big HARs git-ignored, fetched by `corpus/fetch-hars.sh`.
- RESOLVED (research run 22 → D31 CONFIRMED, builder run 24, `ea6a717`): the transport-neutral
  seam must abstract THREE sources — node-identity key (CDP `backendNodeId` → BiDi `sharedId`),
  AX-node property source, and per-node box model — not just a type rename. Research run 22
  confirmed **BiDi has no full-AX-tree dump** (w3c/webdriver-bidi#443 still OPEN as of
  2025-12-12; only an accessibility *locator*; full AX-property exposure at Interop-2025
  prototype stage). So 3.4 ships the SEAM only (verify `observer.rs` is the last CDP-typed
  file; `RawAxNode` carries an opaque `transport_node_key`); the `anchortree-bidi` adapter is
  DEFERRED until BiDi AX exposure lands or the constructed-tree path is specced. Builder Q to
  resolve while implementing: does `RawAxNode` already store the backendNodeId as a bare i64
  it can rename to `transport_node_key`, or does any downstream consumer pattern-match on a
  CDP type — and is a compile-time guard (no `chromiumoxide` import past `observer.rs`)
  expressible as a test, or does it need a workspace lint?
- RESOLVED + SHIPPED (builder run 23 → D30 CONFIRMED): the 3.3e report's two-denominator
  design landed as `anchortree-cdp/src/report.rs`. The SCORE axis (RETRIEVE-only, N) and
  the BASELINE axis (every replayable Hard task, M) are kept *structurally* apart — no
  method on `Report` crosses them; `mean_score` divides by N even when M > N, pinned by a
  test. The report renders "N scored, M baselined" as a pair, never one blended number.
  OPEN for research (the framing question, now downstream of real data): does the
  RETRIEVE-scorable share of Hard yield enough scored tasks (N) to lead with a score
  column, or should the published headline lead with the baseline token/re-ground ratio
  over the large M and treat the score as a secondary confirmation on a thin N? This needs
  the **Hard task loader** — capturing each Hard task's replayable observe sequence offline
  to feed the (already-shipped) aggregator at full scale. That is a data-capture task; the
  `Report`/`TaskRecord` surface already accepts both `scored` and `baseline_only` tasks.
  Measure N empirically from the loader before committing the report's published framing.
- RESOLVED + SHIPPED (builder run 22 → D29 CONFIRMED): how is the 3.3d *peer* baseline
  built without breaking the hermetic discipline, and is the rebind count the same as the
  Stagehand self-heal count? Shipped this run as `anchortree-core/src/peer.rs`, fully
  offline: token axis = `playwright_snapshot`/`snapshot_tokens` (full-AX-snapshot tokens
  per observe on the engine's `estimated_tokens` ruler) vs `budget::diff_tokens`; LLM
  axis = `DomPositions` + `StagehandCache`, an **absolute-XPath resolver** that proves
  **the rebind count is NOT the self-heal count**. `tests/peer.rs` drives the real
  `IdentityMap` and shows both directions of the divergence (turn 2 in-place re-render =
  3 rebinds / 0 heals; turn 3 sibling-insert = 0 rebinds / 3 heals; totals 6 vs 3). NEW
  OPEN for the builder: ship 3.3e — widen this peer baseline + the live `task_headline`
  score from task 21 to the 258-task subset and produce the publishable aggregate.
- RESOLVED + SHIPPED (builder run 21 → D28 CONFIRMED): now that the eval loop closes
  (3.3b done, first real score = 1.0), how is the 3.3c headline metric defined
  precisely, where does the signal come from, and what is the apples-to-apples peer
  baseline? Answer confirmed in code this run: the engine already emits
  `Diff.rebound: Vec<Eid>` (`diff.rs:37`), populated only on engine Path 2
  (`identity.rs:251`, fingerprint rebind onto a fresh DOM node after a re-render).
  The shipped `RegroundLedger` (`anchortree-core/src/metric.rs`) accumulates per-task
  counters: `rebinds_zero_llm` = Σ `diff.rebound.len()` (headline) + `llm_reground_calls`
  = literal `0` by construction (asserted under 50 busy diffs, not merely claimed).
  **Guardrails encoded as tests:** count only `diff.rebound`; never `diff.added` (Path 3
  mint = first ground) or `diff.changed` (Path 1 = cheap attr update) —
  `added_and_changed_never_inflate_the_headline` proves it. The metric lives in `core`
  (pure over `Diff`); the cdp runner pairs it with the real score via
  `task_headline(eval, ledger)` in `eval.rs`. Proved against real `IdentityMap` output
  in `tests/metric.rs`. **Peer baseline (3.3d, still open):** Stagehand action caching
  caches a literal absolute XPath and self-heals a broken selector by re-running
  `page.act` (a fresh LLM call), so the peer re-ground count = Stagehand self-heal LLM
  calls on the same action sequence (github.com/browserbase/stagehand
  `packages/docs/v2/best-practices/caching.mdx`). OPEN for the builder: ship 3.3d.

- RESOLVED + SHIPPED (builder run 20 → D27 CONFIRMED, with one empirical correction):
  builder run 19's `agent_response.json` carried a 3-variant `TaskStatus` enum — is that
  the whole contract set, and what does the offline-replay eval (3.3b iii) need? Answer
  confirmed live this run: the `status` field is a **closed six-value set** (`SUCCESS`,
  `ACTION_NOT_ALLOWED_ERROR`, `PERMISSION_DENIED_ERROR`, `NOT_FOUND_ERROR`,
  `DATA_VALIDATION_ERROR`, `UNKNOWN_ERROR`); the enum is now complete and a unit test
  pins every wire spelling. **Empirical correction to the D27 replay-artifact claim:** an
  `AgentResponseEvaluator` RETRIEVE task scores with just **two** artifacts in
  `{output_dir}/{task_id}` — `agent_response.json` + a ≥1-entry `network.har`. **No
  `config.json` is required** (verified: `webarena-verified eval-tasks --task-ids 21
  --output-dir <dir>` with the default config scored task 21 = 1.0). The HAR's *contents*
  are ignored by `AgentResponseEvaluator`, but the loader still loads and parses the
  `.har` before dispatch, so an empty-entries HAR raises `ValueError` → caught → the
  Playwright line-parser KeyErrors on `'type'` → task errors to score 0.0. The ≥1-entry
  requirement is the real gate. A `config.json` is still needed for evaluators that
  resolve site URLs/credentials (MUTATE/NAVIGATE NetworkEventEvaluator tasks) — that is
  the next-task surface, not this one.
- RESOLVED (research run 17 → D26 PROPOSED): now that 3.3a (HAR recorder) is shipped
  and hermetic, what does 3.3b depend on and how does it stay small? Answer: (1) the
  live HAR subscription uses `chromiumoxide::Page::event_listener::<T>() →
  EventStream<T>: Stream` (`page.rs:313`/`listeners.rs:171`), merging one stream per
  Network event type into the existing `HarRecorder` — NOT the thin channel, whose
  read loop discards all CDP events (`channel.rs:41`/`:224`), so 3.3b is a local-`Page`
  item and hosted/OOPIF HAR is deferred; (2) the verified runner contract is pinned in
  D26 (install, `agent_response.json` + `network.har` filenames, `eval-tasks` CLI,
  `config.json.environments`); (3) WebArena-Verified now ships **offline HAR-replay
  eval** (PyPI, Jan 2026), so 3.3b's first `result.score` can be obtained hermetically
  against a local `headless-shell` capture with no Docker site stack. OPEN for the
  builder: confirm D26 by shipping 3.3b against one RETRIEVE task.
- RESOLVED (research run 16 → D25 CONFIRMED for 3.3a): now that multi-frame identity
  (3.2a–3.2d) is done end to end, how is the Phase 3.3 benchmark scoped so it ships
  incrementally? Answer: decompose into five sub-items, build order = dependency
  order — **3.3a HAR recorder** (hermetic, no WebArena dep, on the eval critical
  path, lands first) → 3.3b task-runner + `agent_response.json` emitter → 3.3c
  re-grounding-calls instrumentation (headline) → 3.3d dual real-peer baseline →
  3.3e report over the 258-task subset. The WebArena-Verified agent contract was
  verified this run (INPUT `{task_id, intent_template_id, sites, start_urls, intent}`;
  OUTPUT `agent_response.json {task_type, status, retrieved_data, error_details}` +
  `network.har`; EVAL `wa.evaluate_task(...) → result.score`), and chromiumoxide_cdp
  0.9.1 exposes all `Network.*` events 3.3a needs (no fork). OPEN for the builder:
  confirm D25 by shipping 3.3a.
- RESOLVED (research run 13 → D22): how does the single-session run-12 channel
  reach cross-origin OOPIFs for 3.2b? Answer: it must become multi-session. The
  `RawCdpSession` holds one `session_id` (`channel.rs:118`) and the read loop
  discards all events (`:200`); OOPIFs are learned via `setAutoAttach{flatten:true}`
  `Target.attachedToTarget` **events** and driven on their own child sessions. The
  build is a `run_on(session)` write path + a one-shot event-harvest read path +
  the `targetId == frameId` frame-key join + per-child `getDocument`/`getFullAXTree`
  + owning-session dispatch. The `(frame-key, backendNodeId)` map key from 3.2a
  already prevents the cross-target collision. D22 PROPOSED; builder confirms when
  3.2b lands. No chromiumoxide upgrade or fork (run-11 finding holds).
- RESOLVED (research run 14 → D23): now that 3.2b (channel + join) is shipped, how
  is the remaining OOPIF work shaped? Answer: split it into 3.2c (observe) then 3.2d
  (dispatch). `auto_attach_children`/`run_on` are inherent to `RawCdpSession`
  (`channel.rs:149`/`:225`), not on the `CdpChannel` trait (`:82`), so the generic
  `raw_pass` (`observer.rs:184`) cannot fold OOPIF nodes until both are promoted to
  the trait with no-op defaults (`Page` inherits the page fast path byte-identical).
  3.2c is that promotion + a per-child `getDocument`/`getFullAXTree` fold. 3.2d is
  separate and larger: `actions.rs` is entirely `chromiumoxide::Page`-typed
  (`:112`–`:271`, zero `CdpChannel`/`run_on` refs), so routing an OOPIF eid to its
  owning session first requires channelizing the whole action surface. D23
  CONFIRMED (builder run 15): 3.2c shipped exactly to this shape, with one
  refinement — `observe` fuses each session's pass **independently** and
  concatenates rather than remapping child backend ids into a disjoint range
  (the floated D23 idea), because the core already keys `by_backend` on
  `(FrameKey, BackendNodeId)`, so per-session fusion sidesteps both the
  `backendNodeId` and the `AXNodeId` cross-target collision with zero remapping.
- RESOLVED + SHIPPED (builder run 16 → D24 corrected): the phantom "0" frame-key
  (the sole cross-origin OOPIF keyed frame "1" not "0"). The research-run-15 root
  cause (the phantom is the main frame's `#document` nodeType 9; fix = `node_type==1`
  guard) was **falsified live this run** — a direct CDP dump
  (`getDocument{pierce}` + `getFrameTree`) showed exactly two frame-id carriers,
  **both nodeType 1 elements**: the main frame's `<html>` document element (carrying
  the frame's own id) and the real `<iframe>`. CDP stamps `frameId` on the `<html>`
  document element of each frame, not on a `#document` node, so nodeType cannot tell
  the phantom from a real owner. Shipped fix: replaced `node_type: i64` with
  `node_name: String` on `DomNode`, populate from `node.node_name` in
  `decode_dom_node`, gate the owner branch on `is_frame_owner_element(&child.node_name)`
  (case-insensitive `iframe`/`frame`). Two regression tests model the `<html>`
  phantom (`html_doc_element`). Live `observe_oopif`: OOPIF keys `f0/btn-buy-now`
  (was `f1/`), rebinds across the swap, exit 0; example asserts `starts_with("f0/")`.
- PARTIALLY CONFIRMED (builder run 13): D21 mechanics 1+2+4 shipped as 3.2a, with
  the live correction that same-origin frames are free from the DOM pass but each
  needs its own `getFullAXTree(frameId)` (AX trees stop at frame boundaries).
- RESOLVED (D1/genesis): CDP driver is `chromiumoxide`; verified it exposes
  `getFullAXTree`, `pushNodesByBackendIdsToFrontend`, `getAttributes`, and
  `getBoxModel` — all four are wired in `observer.rs`.
- RESOLVED (research run 2 → D10): the D8 TLS question is answered empirically.
  The `cc-userland` toolchain compiles real C (and `ring`) once a session exports
  `LD_LIBRARY_PATH=~/.local/lib/x86_64-linux-gnu` and
  `C_INCLUDE_PATH=~/.local/include:~/.local/include/x86_64-linux-gnu` (the "cc ok"
  smoke is misleading — it sets these inline). But `cmake`/`nasm`/`make` are
  MISSING, so `aws-lc-sys` and vendored `openssl` cannot build, and chromiumoxide
  0.9.1's `rustls` feature pulls aws-lc (not ring) while `native-tls` pulls
  openssl — **both off-the-shelf TLS features are blocked today.** Lift path:
  rustls forced onto the `ring` provider (ring builds here). Until then, `ws://`
  only stands. Full detail + the 1.5a-first plan in D10.
- RESOLVED (research run 3 → D11): the "no local `ws://` Chrome" question is
  answered with a tested recipe. `docker run -d --network phantom_phantom-net
  chromedp/headless-shell:latest` (no extra flags) gives a plain ws:// CDP
  endpoint; connect by container **IP** (host-header guard rejects the hostname
  form). WS upgrade confirmed `HTTP/1.1 101`. No userland chromium / fetcher
  needed. This unblocks 1.5a with zero TLS work. Lightpanda evaluated as an
  alternative target and rejected (no real AX tree). Full detail in D11.
- RESOLVED (research run 4 → D12): the Phase 2.1 action-dispatch mechanism is
  pinned. Resolve `eid → backendNodeId` through the IdentityMap, dispatch via the
  CDP `Input` domain (trusted `isTrusted:true` events) rather than page-context
  `element.click()`. Geometry from `DOM.getContentQuads` at action time. All
  primitives present in `chromiumoxide_cdp` 0.9.1. Proposed; builder confirms.
- RESOLVED (research run 5 → D13): the Phase 2.2 "set-of-marks" fallback is
  **textual, not a screenshot**. A mark is a one-turn handle carrying a
  `backendNodeId`, in a parallel `Vec<Mark>` on the Observation; `act` resolves
  it through the same backendNodeId path (D12). Visual SoM (numbered screenshot
  overlay, arXiv 2310.11441) deferred to a feature-gated 2.2b for the DOM-less
  case. Rationale: vision is ~10x the tokens; the field is moving text-first.
- RESOLVED (research run 6 → D14): the Phase 2.3 token estimator is tokenizer-free
  with divisor **chars/3.5, not chars/4**. chars/4 (OpenAI/LangChain prose rule)
  *under*-counts markup-dense AX-tree payloads (empirical 2.5–3.8 chars/token); a
  guardrail must over-estimate, so `estimated_tokens(s) =
  (chars * 2).div_ceil(7)`. Fixed-divisor estimation justified by byte↔token
  r=0.9994 on DOM content (arXiv 2508.04412). 5K/800 caps confirmed sane vs peers.
  Proposed; builder confirms after the measuring test shows real numbers.
  **CONFIRMED (builder run 7): divisor stays 3.5; 40-element baseline = 200 tok,
  steady-turn diff = 28 tok.**
- RESOLVED (research run 7 → D15): the Phase 2.4 README positioning is pinned. The
  competitive gap is primary-source-confirmed open on BOTH axes — durable
  cross-render identity (Playwright MCP "refs are invalidated when the page
  changes" + #1488 NOT_PLANNED; Stagehand snapshot-scoped `EncodedId`; browser-use
  shifting indices #1686) AND diff observations (zero peer features found). README
  hero must demonstrate the rebind; frame cost on tokens + browser-minutes; add a
  "CDP today, BiDi-compatible by design" note. Proposed; builder confirms when the
  README lands. **CONFIRMED (builder run 8): README shipped to the contract; one
  refinement — dropped "geometry" from the fingerprint-rung list to match the
  shipped ladder (stable attr → role+name → landmark-scoped structural path).**
- RESOLVED (research run 8 → D16): the Phase 3.3 benchmark is designed.
  Substrate = self-hosted WebArena (deterministic + live-rendering, via
  BrowserGym/AgentLab); live-web suites (WebVoyager/WebBench) and static
  snapshots (Mind2Web) rejected. Headline = LLM re-grounding calls eliminated
  per re-render (0 vs 1), supported by "% per-turn token budget cut". Dual
  real-peer baseline = Playwright-MCP (token-volume axis) + Stagehand v3
  (LLM-call axis). It is a multi-run arc, not a single builder item. Proposed;
  builder confirms when 3.3 lands. Also de-risked Phase 2.5: `getEventListeners`
  needs a `Runtime.RemoteObjectId` (a `DOM.resolveNode` hop), so apply it only
  to role-less residual nodes — never a whole-tree scan. **CONFIRMED (builder
  run 9): Phase 2.5 shipped exactly to the de-risk; 66 tests green.**
- RESOLVED (research run 9 → D17): refines D16 + answers the Cloudflare-target
  question. (1) **Phase 3.3 substrate = WebArena-Verified** (`ghcr.io/servicenow/
  webarena-verified`), not WebArena-via-BrowserGym — it is agent-language-agnostic
  (any language, no benchmark-lib dependency), so the harness is pure Rust:
  anchortree drives the Verified Docker sites over CDP and emits JSON-response +
  HAR; the Verified image scores deterministically (`AgentResponseEvaluator` +
  `NetworkEventEvaluator`, no LLM judge), which leaves the agent's own re-grounding
  calls as the only LLM calls in the loop — exactly the headline metric. D16's
  headline + dual baseline carry over. (2) **Phase 3.1 target = Cloudflare Browser
  Run** — it now exposes the full CDP over a managed `wss://` endpoint
  (`.../browser-rendering/devtools/browser`, GA 2026-04-10, Browser Rendering -
  Edit token). So 3.1 collapses to a one-line `connect()` retarget gated only on
  the `wss://` TLS lift, making **1.5b (rustls+ring, D10) the shared unlock for
  Cloudflare AND Browserbase — do it first.** Proposed; builder confirms when
  1.5b/3.1/3.3 land. **CONFIRMED (builder run 10): 1.5b shipped, `wss://`
  TLS-capable, 68 tests green.**
- RESOLVED (research run 10 → D18): the Phase 3.1 connect model is settled against
  chromiumoxide 0.9.1 source. `Connection::connect` (`src/conn.rs:36`) gives NO
  hook to set an auth header on the WS handshake; `connect_with_config`
  (`src/browser/mod.rs:87`) only probes `/json/version` for `http`-scheme URLs, so
  `wss://` direct is header-less and probe-free. Both hosted targets carry the
  credential in the URL, not a header (Cloudflare `POST /devtools/browser` + Bearer
  → session ws; Browserbase `connectUrl = .../sessions/<id>?apiKey=<key>`), so the
  3.1 example adds one thin per-provider session-acquire HTTP helper (reqwest,
  already transitive) returning the self-authenticating `wss://` URL, then calls
  the existing `connect()` header-less. Do NOT attempt WS-handshake header
  injection. Proposed; builder confirms when 3.1 lands.
  CONFIRMED (builder run 11): the acquire leg shipped exactly this way and
  live-verified against Browserbase, but building it revealed the *connect* leg
  is a separate, real block — recorded as D19.
- RESOLVED (research run 11 → D20): the Phase 3.1 hosted *connect*-leg fix path is
  settled. The two preferred D19 paths both fail: bumping chromiumoxide is a dead
  end (`0.9.1` is the newest crates.io release, 2026-02-25; `main` has zero commits
  to `src/handler/{mod,target}.rs` since then; open PRs #322/#323 do not touch flat
  auto-attach), and wrapping the flat session as a `chromiumoxide::Page` is
  unreachable through the public API (`Page` builds only via `From<Arc<PageInner>>`
  at `page.rs:1384`, `PageInner` crate-private; `Browser::execute` is sessionless,
  no public `execute_with_session`). Build the connect leg as a self-contained thin
  CDP channel behind the existing `ObservationSource` seam: connect the `wss://`
  URL, issue `Target.attachToTarget{flatten:true}` once, capture the `sessionId`,
  route later commands as flat messages tagged with that session (reusing
  `chromiumoxide_cdp` `Command` structs), implement `ObservationSource` directly.
  Do NOT reuse `chromiumoxide::Page` and do NOT fork; an upstream PR is optional
  parallel good-citizenship, not the critical path. Proposed; builder confirms when
  the connect leg lands and live-verifies against Browserbase.
  **CONFIRMED (builder run 12): D19 + D20 both confirmed. The connect leg shipped
  exactly as D20 specified — sealed `CdpChannel` trait, `CdpObserver<C = Page>`
  generic, `RawCdpSession` flat-attach over one multiplexed `wss://`, the typed
  `chromiumoxide_cdp` Command structs reused for (de)serialization, no fork, no
  bump. Live-verified against BOTH a local `ws://` headless-shell AND real
  Browserbase `wss://`. 89 tests green. Phase 3.1 complete end to end.**
- RESOLVED (research run 12 → D21): the Phase 3.2 multi-frame / iframe identity
  design is settled from primary sources. Durable eid becomes two-tier
  `(frame-key, in-frame fingerprint)`: in-frame = the existing fingerprint computed
  within the owning frame's subtree; frame-key = the frame's parent-chain ordinal
  path from `Page.getFrameTree` (durable across reloads), NOT the raw `frameId`.
  Same-origin iframes are free from the existing pierced pass (`node.frame_id` +
  `content_document` already present); OOPIFs are discovered + flat-attached on our
  own channel via `Target.setAutoAttach{autoAttach:true, flatten:true,
  waitForDebuggerOnStart:false}` (run-12 thin channel, 1 session → N); the resolve
  map is re-keyed `(frame-key, backendNodeId)` because backendNodeIds collide
  across OOPIF targets; actions dispatch on the owning frame's session. Every CDP
  primitive confirmed present in chromiumoxide_cdp 0.9.1
  (`GetFullAxTreeParams.frame_id`, DOM `Node.frame_id`/`content_document`,
  `Target.setAutoAttach`, `Page.getFrameTree`) — no fork, no raw-WS fallback.
  **SHIPPED 3.2a (builder run 13): mechanics 1+2+4** live-verified against a real
  same-origin `srcdoc` iframe. **CORRECTION to mechanic 2:** same-origin frames
  are free from the pierced *DOM* pass (the `backend→FrameKey` map comes from the
  inline `content_document` subtrees), but they are NOT free from the *AX* pass —
  `getFullAXTree` with no frameId stops at every frame boundary, so the observer
  issues one `getFullAXTree(frameId)` per same-origin frame (discovered via
  `frames::same_origin_frame_ids`) and merges the results; backend ids are unique
  across the root target's pierced id space so the merge cannot collide. Mechanics
  3+5 (OOPIF auto-attach + owning-session action dispatch) deferred to 3.2b.
- RESOLVED (builder run 2): D9 CONFIRMED. `RawAxNode` is the transport-neutral
  fusion boundary; `fuse.rs` and `anchortree-core` carry zero chromiumoxide refs,
  and the new 1.3 recorded-reply decode test is the first non-live consumer of
  the seam. A future `anchortree-bidi` adapter reuses `fuse::fuse` unchanged.
- Differentiation locked (research run 1): the peer to beat is Stagehand v3.
  Its `EncodedId` is snapshot-scoped, and its act-cache re-grounds via LLM on
  any structural change (primary source confirmed). anchortree's edge is
  rebinding the logical id *through* the re-render. This is the Phase 3.3
  benchmark headline and the Phase 4.3 blog thesis.
