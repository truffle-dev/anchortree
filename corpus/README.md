# corpus — vendored WebArena-Verified task artifacts

This directory holds real benchmark task logs that anchortree's report aggregator
([`anchortree-cdp` `corpus`](../crates/anchortree-cdp/src/corpus.rs)) folds into a
score. It exists so anchortree's published numbers are computed over genuine
evaluator output, never synthetic stand-ins (DECISIONS D32).

## Source and license

Everything here is vendored from
[ServiceNow/webarena-verified](https://github.com/ServiceNow/webarena-verified),
which is **Apache-2.0** (redistribution permitted with attribution).

- `107/`, `108/` — the two demo task logs the benchmark ships under
  `examples/agent_logs/demo/{107,108}/`.
- `subsets/webarena-verified-hard.json` — the benchmark's Hard subset id list
  (`assets/dataset/subsets/webarena-verified-hard.json`), 258 task ids.

## Layout

```text
corpus/
  107/  eval_result.json  agent_response.json   # NAVIGATE answer, scored 0.0 (fail)
  108/  eval_result.json  agent_response.json   # RETRIEVE answer, scored 1.0 (pass)
  subsets/webarena-verified-hard.json           # 258 Hard task ids (108 ∈ Hard; 107 is a demo)
```

Each task directory carries up to three files:

| file | axis | what it is |
| --- | --- | --- |
| `eval_result.json` | score (N) | the benchmark's `AgentResponseEvaluator` verdict — read offline, no live site |
| `agent_response.json` | score (N) | the agent's graded answer (context; the score is authoritative in `eval_result.json`) |
| `network.har` | baseline (M) | the captured network trace — the *replayable precondition*, not an observe capture |

## Two axes, and what is checked in here

The report has two denominators (DECISIONS D30):

- **Score axis (N)** is real and offline. `eval_result.json` already carries the
  computed score, so `report_from_corpus` folds 107 (0.0) and 108 (1.0) into a
  genuine N = 2 aggregate, mean 0.50 — one real pass, one real fail.
- **Baseline axis (M)** needs a replayed *observe* sequence (per-turn
  accessibility + DOM + layout). A `network.har` is a network trace, not an
  accessibility dump, and anchortree has no offline HTML→AX path, so M stays 0
  until the 3.5b capture step runs a browser against a task. The HAR only marks a
  task `is_replayable` (the precondition that a capture *can* be run).

## The large HARs are not checked in

The `network.har` files are hundreds of KB each and are not needed for the score
axis, so they are git-ignored (`.gitignore` in this directory) and fetched on
demand by [`fetch-hars.sh`](fetch-hars.sh) when 3.5b's capture work needs them.
