#!/usr/bin/env bash
#
# run-once-eval.sh — capture ONE real WebArena-Verified task page live, emit the
# agent contract, then score it with the genuine ServiceNow evaluator container
# and assert a deterministic score == 1.0. This is the Phase 3.5b Tier 2 external
# evaluator datapoint (DECISIONS D44): not our own assertion that the observe
# loop worked, but the upstream `webarena-verified` scorer agreeing that the
# captured `network.har` + `agent_response.json` pass a real task's eval.
#
# Where run-once-webarena.sh proves anchortree reconstructs and observes a real
# page offline (our own success criterion), this script closes the loop against
# the authority: the same evaluator the WebArena-Verified benchmark uses scores
# our recording and stamps the result with its own checksums.
#
# ## The task (M=1): 356 — a NAVIGATE map task whose network assertion is the
# ## last navigation event being GET 200 to __MAP__ (the map home page). Two evals:
#   1. AgentResponseEvaluator expects {NAVIGATE, SUCCESS, null, null}.
#   2. NetworkEventEvaluator (last_event_only) expects the last navigation event
#      to be GET 200 to __MAP__ (the evaluator normalises both __MAP__ and the
#      captured http://<site>/ to {base_url: "__MAP__/"}, so the home page satisfies it).
#
# Why 356 and not a /way/ task (e.g. 369 -> __MAP__/way/154257484/): the public
# slim map image (am1n3e/webarena-verified-map, ~4.75GB) ships the OpenStreetMap
# Rails stack and routing binaries but NO OSM way/node data — current_ways is
# empty, so every /way/, /node/, /relation/ browse page 404s. A task whose
# expected target is a data-backed page cannot honestly score 200 on this image.
# 356 targets the home page, which the image genuinely serves 200, so the
# external evaluator scores a real live capture without any fabricated response.
#
# We navigate Chrome straight to the map home page, capture the document request
# (now carrying its real on-wire Accept / sec-fetch-* headers via the recorder's
# requestWillBeSentExtraInfo merge, so the evaluator recognises it as a
# navigation), emit AgentResponse::completed(NAVIGATE), tear the site down, and
# hand the output dir to `eval-tasks`.
#
# This is an OPERATIONAL script, not a CI gate: it needs Docker, a real browser,
# and the multi-GB map image. CI never runs it.
#
# ## Usage
#
#   bash scripts/run-once-eval.sh
#
# ## Environment overrides (all optional; defaults are task 369 on the map site)
#
#   EVAL_IMAGE   evaluator image     (default ghcr.io/servicenow/webarena-verified:latest)
#   SITE_IMAGE   per-site image      (default am1n3e/webarena-verified-map)
#   SITE_NAME    sibling name        (default at-wa-map)
#   SITE_PORT    in-container port   (default 8080, apache)
#   TASK_ID      WebArena task id    (default 369)
#   TASK_PATH    path to capture     (default /way/154257484/)
#   SITE_KEY     config site key     (default map)
#   DOCKER_NET   phantom net         (default phantom_phantom-net)
#   CHROME_BIN   CDP-capable headless shell
#   CDP_PORT     remote debugging port (default 9222)
#   KEEP_SITE=1       leave the site container running on success
#   KEEP_ARTIFACTS=1  leave the output dir + eval_result.json in place on success
#
# Exit 0 means the external evaluator scored the captured task 1.0.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

EVAL_IMAGE="${EVAL_IMAGE:-ghcr.io/servicenow/webarena-verified:latest}"
SITE_IMAGE="${SITE_IMAGE:-am1n3e/webarena-verified-map}"
SITE_NAME="${SITE_NAME:-at-wa-map}"
SITE_PORT="${SITE_PORT:-8080}"
TASK_ID="${TASK_ID:-356}"
TASK_PATH="${TASK_PATH:-/}"
SITE_KEY="${SITE_KEY:-map}"
DOCKER_NET="${DOCKER_NET:-phantom_phantom-net}"
CHROME_BIN="${CHROME_BIN:-$HOME/.cache/ms-playwright/chromium_headless_shell-1217/chrome-headless-shell-linux64/chrome-headless-shell}"
CDP_PORT="${CDP_PORT:-9222}"

SITE_BASE="http://${SITE_NAME}:${SITE_PORT}"
TASK_URL="${SITE_BASE}${TASK_PATH}"

# The evaluator runs as a SIBLING container against the host Docker daemon, so a
# bind mount source is resolved in the HOST filesystem namespace, not ours. A
# plain mktemp dir under /tmp is private to this container and the daemon would
# create an empty placeholder dir for it (IsADirectoryError on the config file).
# So WORK must live under a path that is shared with the host. /app/repos is the
# `phantom_phantom_repos` named volume, whose host data dir is HOST_REPOS_ROOT;
# we place WORK there and translate it to the host path for the docker -v flags.
REPOS_ROOT="${REPOS_ROOT:-/app/repos}"
HOST_REPOS_ROOT="${HOST_REPOS_ROOT:-/var/lib/docker/volumes/phantom_phantom_repos/_data}"
WORK="$REPOS_ROOT/anchortree/.eval-tmp/run-$$"
host_path() { echo "${1/#$REPOS_ROOT/$HOST_REPOS_ROOT}"; }
OUT_DIR="$WORK/output"
TASK_OUT="$OUT_DIR/$TASK_ID"
CONFIG_FILE="$WORK/config.json"
PROFILE_DIR="$(mktemp -d)"

rm -rf "$WORK" 2>/dev/null || true
mkdir -p "$WORK"

command -v docker >/dev/null 2>&1 || { echo "error: docker not on PATH" >&2; exit 1; }
if [[ ! -x "$CHROME_BIN" ]]; then
  echo "error: no CDP-capable Chrome at $CHROME_BIN (set CHROME_BIN)" >&2
  exit 1
fi

# The evaluator renders the task's __MAP__ placeholder against this config, so
# the rendered expected URL (__MAP__ -> SITE_BASE) matches the URL we navigate
# Chrome to and capture.
mkdir -p "$TASK_OUT"
cat >"$CONFIG_FILE" <<JSON
{
  "environments": {
    "$SITE_KEY": {
      "urls": ["$SITE_BASE"]
    }
  }
}
JSON

# Pre-build the capture example BEFORE launching the browser: the phantom
# container's pids cgroup (pids.max=256, threads included) cannot fit a headless
# Chrome (~150 threads) and a concurrent rustc/ld at once. Build down, run up.
echo "==> pre-building webarena_capture (browser down to keep pids headroom)"
cargo build -q -p anchortree-cdp --example webarena_capture

CHROME_PID=""
SITE_STARTED=""
cleanup() {
  [[ -n "$CHROME_PID" ]] && kill "$CHROME_PID" 2>/dev/null || true
  if [[ -n "$SITE_STARTED" && "${KEEP_SITE:-0}" != "1" ]]; then
    docker rm -f "$SITE_NAME" >/dev/null 2>&1 || true
  fi
  if [[ "${KEEP_ARTIFACTS:-0}" != "1" ]]; then
    rm -rf "$WORK" "$PROFILE_DIR" 2>/dev/null || true
  fi
}
trap cleanup EXIT

echo "==> booting $SITE_IMAGE as sibling container '$SITE_NAME'"
docker rm -f "$SITE_NAME" >/dev/null 2>&1 || true
docker run -d --name "$SITE_NAME" "$SITE_IMAGE" >/dev/null
SITE_STARTED=1

echo "==> joining '$SITE_NAME' to $DOCKER_NET (container-DNS reachability)"
docker network connect "$DOCKER_NET" "$SITE_NAME" 2>/dev/null \
  || echo "    (already attached or network absent; continuing)"

echo "==> launching headless Chrome on :${CDP_PORT}"
"$CHROME_BIN" \
  --headless --no-sandbox --disable-gpu \
  --remote-debugging-port="${CDP_PORT}" \
  --user-data-dir="$PROFILE_DIR" \
  about:blank >/dev/null 2>&1 &
CHROME_PID=$!

echo "==> waiting for CDP endpoint"
for _ in $(seq 1 50); do
  curl -sf "http://127.0.0.1:${CDP_PORT}/json/version" >/dev/null 2>&1 && break
  sleep 0.2
done
curl -sf "http://127.0.0.1:${CDP_PORT}/json/version" >/dev/null \
  || { echo "error: CDP endpoint never came up" >&2; exit 1; }

# The site stack (supervisord -> postgres -> rails/apache) takes a while to serve.
echo "==> waiting for $TASK_URL to answer (site warm-up can take ~1-2 min)"
for _ in $(seq 1 180); do
  curl -sf "$TASK_URL" >/dev/null 2>&1 && break
  sleep 1
done
HTTP_CODE="$(curl -s -o /dev/null -w '%{http_code}' "$TASK_URL" || true)"
echo "    $TASK_URL -> HTTP $HTTP_CODE"
[[ "$HTTP_CODE" == "200" ]] \
  || { echo "error: task page did not serve 200 (got $HTTP_CODE); the evaluator wants a 200 navigation" >&2; exit 1; }

echo "==> capturing the navigation HAR + agent contract (NAVIGATE) into $TASK_OUT"
ANCHORTREE_CDP_HTTP="http://127.0.0.1:${CDP_PORT}" \
ANCHORTREE_CAPTURE_URL="$TASK_URL" \
ANCHORTREE_CAPTURE_OUT="$TASK_OUT" \
ANCHORTREE_TASK_TYPE="navigate" \
  cargo run -q -p anchortree-cdp --example webarena_capture

[[ -f "$TASK_OUT/network.har" ]] || { echo "error: no network.har at $TASK_OUT" >&2; exit 1; }
[[ -f "$TASK_OUT/agent_response.json" ]] || { echo "error: no agent_response.json at $TASK_OUT" >&2; exit 1; }

# Tear the site down BEFORE scoring: the evaluator is a pure offline scorer that
# reads only the captured files, so a torn-down site proves the score comes from
# the recording, not a live origin.
echo "==> tearing the site down (scoring is offline, reads only the captured files)"
docker rm -f "$SITE_NAME" >/dev/null 2>&1 || true
SITE_STARTED=""

echo "==> scoring with the external evaluator ($EVAL_IMAGE)"
docker run --rm \
  -v "$(host_path "$OUT_DIR"):/data" \
  -v "$(host_path "$CONFIG_FILE"):/config.json:ro" \
  "$EVAL_IMAGE" \
  eval-tasks --task-ids "$TASK_ID" --output-dir /data --config /config.json

RESULT_FILE="$TASK_OUT/eval_result.json"
[[ -f "$RESULT_FILE" ]] || { echo "error: evaluator wrote no eval_result.json at $RESULT_FILE" >&2; exit 1; }

echo
echo "==> eval_result.json:"
cat "$RESULT_FILE"
echo

# Assert the score is exactly 1.0. The score key is a float at the top level.
SCORE="$(grep -oE '"score"[[:space:]]*:[[:space:]]*[0-9.]+' "$RESULT_FILE" | grep -oE '[0-9.]+$' | head -1)"
echo "==> parsed score = ${SCORE:-<none>}"
case "$SCORE" in
  1|1.0|1.00) : ;;
  *) echo "error: external evaluator score was '$SCORE', expected 1.0" >&2; exit 1 ;;
esac

echo
echo "OK: external WebArena-Verified evaluator scored task $TASK_ID = 1.0."
echo "    A real task page was captured live, its navigation document carried the"
echo "    on-wire headers the evaluator classifies on, and the upstream scorer"
echo "    agreed — with its own evaluator + data checksums stamped in the result."
