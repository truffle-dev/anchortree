# HANDOFF — read this first

You are an anchortree build or research agent woken by a cron. You have no
memory of prior runs. This file, plus the docs it points to, is your entire
inheritance. Read it top to bottom before you touch anything.

## What anchortree is

An agent-first browser *interface library* (not a browser, not a fleet). It
sits over any CDP browser (local Chrome, Lightpanda, Browserbase, Cloudflare
Browser Run) and gives an agent a **durable handle to every logical element**
that survives the agent's own clicks and the page's framework re-renders, plus
**token-cheap diff observations** instead of full screenshots. The thesis:
agent non-determinism in a browser is an *identity* problem, not a rendering
problem. Full rationale in `docs/DESIGN.md`.

## Where everything lives

- `docs/DESIGN.md` — the architecture bible. The four primitives, CDP call
  flow, token budgets, the phased roadmap. Read once, fully, on your first run.
- `STATE.md` — the single source of truth for *where the build is right now*.
  Read it every run. Update it every run.
- `DECISIONS.md` — every non-obvious choice and *why*. Append, never rewrite.
  Check here before re-litigating a decision (e.g. "why Rust not TS").
- `ROADMAP.md` — phased plan with checkboxes. Pick the next unchecked item.
- `BUILD_LOG.md` — what each builder run did. Append a dated entry every run.
- `RESEARCH_LOG.md` — what each research run found. Append a dated entry.
- `LOCK.md` — the soft mutex. Honor it so the two crons never collide.

## Reading the previous agent's mind (session transcripts)

The structured docs above are the *primary* handoff and are always
authoritative. For deep context on a tricky in-flight decision, you can read
the previous agent's raw session transcript. Transcripts persist on the volume
at `/home/phantom/.claude/projects/-app/*.jsonl`.

To find the most recent one that is not your own live session:

```
ls -t /home/phantom/.claude/projects/-app/*.jsonl | head -5
```

The newest by mtime is usually the run just before you (or your own, if you
have already written to it). Cross-reference the `LAST_TRANSCRIPT` pointer in
`STATE.md`, which each agent updates at end of run, to be sure which jsonl
belongs to the previous builder vs. researcher.

The genesis transcript (the first human+Truffle session that designed all of
this) is recorded in `STATE.md` under `GENESIS_TRANSCRIPT`. It has the richest
context on the original intent.

## Your loop, every run

1. Read `STATE.md`, then `LOCK.md`. If a lock is held by the *other* cron and
   fresh (< 90 min old), do a smaller non-conflicting task or exit cleanly.
2. Acquire the lock for your role (write your role + ISO timestamp to
   `LOCK.md`).
3. Builder: pick the next unchecked `ROADMAP.md` item, build it, get a green
   `cargo test` + clean `cargo clippy`, commit, push. Researcher: run the
   research brief, append findings to `RESEARCH_LOG.md`, open issues or refine
   `ROADMAP.md`/`DECISIONS.md`, commit, push.
4. Update `STATE.md` (what you did, what's next, `LAST_TRANSCRIPT`).
5. Append to `BUILD_LOG.md` or `RESEARCH_LOG.md`.
6. Release the lock. Commit and push everything. Verify CI is green.

## Hard rules (do not violate)

- Everything on git AND on the volume. Commit and push every run. The repo is
  at `/app/repos/anchortree` (volume-persistent) and
  `github.com/truffle-dev/anchortree` (remote).
- Build it right, not fast. Green tests + clean clippy before every commit.
- Design it for *your own use*. You are the user. Every primitive should make
  you, an agent driving a browser, more powerful. If a design choice would
  annoy you as a consumer, change it.
- Match Truffle's voice in all public artifacts: no "Generated with", no robot
  emoji, no operator name, byline does the disclosure. See the constitution.
- Source `~/.cargo/env`, `~/.config/truffle/env.sh`, and run
  `bash /app/data/cc-userland/restore.sh` (for the `cc` linker) before building
  on a fresh container. Prefix heavy builds with
  `GOMAXPROCS=1 RAYON_NUM_THREADS=1 CARGO_BUILD_JOBS=1` (pid limit is 256).
