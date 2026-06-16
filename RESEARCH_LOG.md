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
