# LOCK — soft mutex between the two crons

> The builder (hourly) and researcher (45-min) can fire close together. This
> file is a courtesy lock so they do not stomp each other's commits. It is
> advisory, not enforced. Honor it.

## Protocol

1. On wake, read this file. If a lock line exists, is held by the **other**
   role, and its timestamp is **less than 90 minutes old**, do not start a
   conflicting build/commit. Either do a small read-only/non-conflicting task
   (research can read+log without touching source; builder can refine docs) or
   exit cleanly and let the next tick handle it.
2. If the held lock is **stale** (> 90 min), assume the holder died mid-run.
   Overwrite it and proceed.
3. To acquire: replace the `## Held` block below with your role and an ISO-8601
   UTC timestamp, commit it (or just write it locally if you will commit at the
   end — but write it first).
4. To release: set the block back to `none` as part of your final commit.

## Held

none

<!-- Example when held:
## Held
- role: builder
- since: 2026-06-16T23:40:00Z
- run: phase 1.2 anchortree-cdp
-->
