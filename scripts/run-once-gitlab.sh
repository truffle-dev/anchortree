#!/usr/bin/env bash
#
# run-once-gitlab.sh — the data-backed NAVIGATE datapoint (DECISIONS D45 item 2,
# D46). Where run-once-eval.sh scored a NAVIGATE to the map HOME page (no data
# dependency) and run-once-retrieve.sh scored a RETRIEVE off an authenticated
# admin grid, this script proves the NAVIGATE contract reaches a real CONTENT
# page PAST a home page on a self-contained, data-baked site — refuting the map
# /way/ 404 as image-specific (the slim map image ships no OSM data; the gitlab
# image ships its repositories baked in).
#
# ## The task (task 45): a gitlab NAVIGATE task. Intent "Open the issues page for
# ## the current project filtered to the most recent open issues",
# ## start_urls = ['__GITLAB__/a11yproject/a11yproject.com']. Two evaluators:
#   1. AgentResponseEvaluator expects {NAVIGATE, SUCCESS, null, null}.
#   2. NetworkEventEvaluator expects the navigation URL
#      __GITLAB__/a11yproject/a11yproject.com/-/issues (exact, no regex, no
#      product/selection reasoning — a pure navigation proof).
#
# anchortree navigates Chrome to the project's issues page on the live gitlab,
# captures the navigation document HAR, emits AgentResponse::completed(NAVIGATE),
# tears the site down, and scores offline. The evaluator normalises the captured
# real URL (http://at-gl/...) to __GITLAB__/... via the config environment, so
# the rendered expected URL matches what we navigated to.
#
# ## The gitlab host-pin (the gitlab analogue of build run 38's Magento base_url):
# gitlab-ce reads external_url from /etc/gitlab/gitlab.rb and 302-redirects any
# request whose Host header does not match it. We pin external_url to the sibling
# hostname and `gitlab-ctl reconfigure` so http://at-gl/ serves 200 instead of
# bouncing to localhost. This is slow (~1-3 min) but only runs once per boot.
#
# This is an OPERATIONAL script, not a CI gate: it needs Docker, a real browser,
# and the multi-GB gitlab image. CI never runs it.
#
# ## STATUS (build run 39): NOT YET EXECUTED — disk-deferred. The gitlab-ce image
# extracts to ~12 GB+ and the pull failed with "no space left on device" on this
# VM (no dangling images/exited containers to reclaim; the rest belongs to live
# projects). D46 item (2) was instead scored on shopping_admin task 157 via
# run-once-admin-nav.sh. This script is the designed rail for when disk headroom
# exists; gitlab task 45 stays the canonical pure-nav pick. See BUILD_LOG run 39.
#
# ## Usage
#
#   bash scripts/run-once-gitlab.sh
#
# ## Environment overrides (all optional; defaults are task 45 on the gitlab site)
#
#   EVAL_IMAGE   evaluator image     (default ghcr.io/servicenow/webarena-verified:latest)
#   SITE_IMAGE   per-site image      (default am1n3e/webarena-verified-gitlab)
#   SITE_NAME    sibling name        (default at-gl)
#   SITE_PORT    in-container port   (default 80)
#   TASK_ID      WebArena task id    (default 45)
#   TASK_PATH    path to capture     (default /a11yproject/a11yproject.com/-/issues)
#   SITE_KEY     config site key     (default gitlab)
#   DOCKER_NET   phantom net         (default phantom_phantom-net)
#   CHROME_BIN   CDP-capable headless shell
#   CDP_PORT     remote debugging port (default 9222)
#   SKIP_RECONFIGURE=1  skip the external_url pin (if the image already serves on at-gl)
#   KEEP_SITE=1         leave the site container running on success
#   KEEP_ARTIFACTS=1    leave the output dir + eval_result.json in place on success
#
# Exit 0 means the external evaluator scored the captured NAVIGATE task 1.0.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

EVAL_IMAGE="${EVAL_IMAGE:-ghcr.io/servicenow/webarena-verified:latest}"
SITE_IMAGE="${SITE_IMAGE:-am1n3e/webarena-verified-gitlab}"
SITE_NAME="${SITE_NAME:-at-gl}"
SITE_PORT="${SITE_PORT:-80}"
TASK_ID="${TASK_ID:-45}"
TASK_PATH="${TASK_PATH:-/a11yproject/a11yproject.com/-/issues}"
SITE_KEY="${SITE_KEY:-gitlab}"
DOCKER_NET="${DOCKER_NET:-phantom_phantom-net}"
CHROME_BIN="${CHROME_BIN:-$HOME/.cache/ms-playwright/chromium_headless_shell-1217/chrome-headless-shell-linux64/chrome-headless-shell}"
CDP_PORT="${CDP_PORT:-9222}"

# gitlab-ce serves on port 80; SITE_BASE drops the :80 so the captured URL and
# the evaluator's rendered __GITLAB__ base match exactly (no stray :80).
if [[ "$SITE_PORT" == "80" ]]; then
  SITE_BASE="http://${SITE_NAME}"
else
  SITE_BASE="http://${SITE_NAME}:${SITE_PORT}"
fi
TASK_URL="${SITE_BASE}${TASK_PATH}"

# Sibling-container bind mounts resolve in the HOST namespace, so WORK must live
# on a host-shared path. /app/repos is the phantom_phantom_repos named volume.
REPOS_ROOT="${REPOS_ROOT:-/app/repos}"
HOST_REPOS_ROOT="${HOST_REPOS_ROOT:-/var/lib/docker/volumes/phantom_phantom_repos/_data}"
WORK="$REPOS_ROOT/anchortree/.eval-tmp/run-$$"
host_path() { echo "${1/#$REPOS_ROOT/$HOST_REPOS_ROOT}"; }
OUT_DIR="$WORK/output"
TASK_OUT="$OUT_DIR/$TASK_ID"
CONFIG_FILE="$WORK/config.json"
PROFILE_DIR="$(mktemp -d)"

rm -rf "$WORK" 2>/dev/null || true
mkdir -p "$TASK_OUT"

command -v docker >/dev/null 2>&1 || { echo "error: docker not on PATH" >&2; exit 1; }
if [[ ! -x "$CHROME_BIN" ]]; then
  echo "error: no CDP-capable Chrome at $CHROME_BIN (set CHROME_BIN)" >&2
  exit 1
fi

# The evaluator renders the task's __GITLAB__ placeholder against this config, so
# the rendered expected URL (__GITLAB__ -> SITE_BASE) matches the URL we capture.
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
# Chrome and a concurrent rustc/ld at once. Build down, run up.
echo "==> pre-building webarena_capture (browser down to keep pids headroom)"
CARGO_INCREMENTAL=0 GOMAXPROCS=1 RAYON_NUM_THREADS=1 CARGO_BUILD_JOBS=1 \
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

# Reuse an already-running site if present; otherwise boot a fresh one.
if docker ps --filter "name=^${SITE_NAME}$" --format '{{.Names}}' | grep -q "^${SITE_NAME}$"; then
  echo "==> reusing running site container '$SITE_NAME'"
else
  echo "==> booting $SITE_IMAGE as sibling container '$SITE_NAME'"
  docker rm -f "$SITE_NAME" >/dev/null 2>&1 || true
  docker run -d --name "$SITE_NAME" "$SITE_IMAGE" >/dev/null
  SITE_STARTED=1
  echo "==> joining '$SITE_NAME' to $DOCKER_NET (container-DNS reachability)"
  docker network connect "$DOCKER_NET" "$SITE_NAME" 2>/dev/null \
    || echo "    (already attached or network absent; continuing)"
fi

# Pin external_url to the sibling hostname so http://at-gl/ serves 200 instead of
# 302-redirecting to the image's baked-in host. Idempotent: safe on a reused site.
if [[ "${SKIP_RECONFIGURE:-0}" != "1" ]]; then
  echo "==> pinning gitlab external_url to $SITE_BASE and reconfiguring (slow, ~1-3 min)"
  docker exec "$SITE_NAME" bash -lc "
    if [ -f /etc/gitlab/gitlab.rb ]; then
      if grep -qE '^external_url' /etc/gitlab/gitlab.rb; then
        sed -i \"s|^external_url.*|external_url '${SITE_BASE}'|\" /etc/gitlab/gitlab.rb
      else
        echo \"external_url '${SITE_BASE}'\" >> /etc/gitlab/gitlab.rb
      fi
      gitlab-ctl reconfigure >/dev/null 2>&1 || true
      gitlab-ctl restart nginx >/dev/null 2>&1 || true
    fi
  " || echo "    (external_url pin skipped; continuing)"
fi

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

# gitlab boots slowly (puma + workhorse + nginx via runit); allow a generous warm-up.
echo "==> waiting for $TASK_URL to answer 200 (gitlab warm-up can take several min)"
HTTP_CODE=""
for _ in $(seq 1 600); do
  HTTP_CODE="$(curl -s -o /dev/null -w '%{http_code}' "$TASK_URL" || true)"
  [[ "$HTTP_CODE" == "200" ]] && break
  sleep 1
done
echo "    $TASK_URL -> HTTP $HTTP_CODE"
[[ "$HTTP_CODE" == "200" ]] \
  || { echo "error: issues page did not serve 200 (got $HTTP_CODE); the evaluator wants a 200 navigation" >&2; exit 1; }

echo "==> capturing the navigation HAR + agent contract (NAVIGATE) into $TASK_OUT"
ANCHORTREE_CDP_HTTP="http://127.0.0.1:${CDP_PORT}" \
ANCHORTREE_CAPTURE_URL="$TASK_URL" \
ANCHORTREE_CAPTURE_OUT="$TASK_OUT" \
ANCHORTREE_TASK_TYPE="navigate" \
  cargo run -q -p anchortree-cdp --example webarena_capture

[[ -f "$TASK_OUT/network.har" ]] || { echo "error: no network.har at $TASK_OUT" >&2; exit 1; }
[[ -f "$TASK_OUT/agent_response.json" ]] || { echo "error: no agent_response.json at $TASK_OUT" >&2; exit 1; }

echo "==> agent_response.json:"
cat "$TASK_OUT/agent_response.json"
echo

# Tear the site down BEFORE scoring (unless reused): the evaluator reads only the
# captured files, so a torn-down site proves the score comes from the recording.
if [[ -n "$SITE_STARTED" && "${KEEP_SITE:-0}" != "1" ]]; then
  echo "==> tearing the site down (scoring is offline, reads only the captured files)"
  docker rm -f "$SITE_NAME" >/dev/null 2>&1 || true
  SITE_STARTED=""
fi

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

SCORE="$(grep -oE '"score"[[:space:]]*:[[:space:]]*[0-9.]+' "$RESULT_FILE" | grep -oE '[0-9.]+$' | head -1)"
echo "==> parsed score = ${SCORE:-<none>}"
case "$SCORE" in
  1|1.0|1.00) : ;;
  *) echo "error: external evaluator score was '$SCORE', expected 1.0" >&2; exit 1 ;;
esac

echo
echo "OK: external WebArena-Verified evaluator scored NAVIGATE task $TASK_ID = 1.0."
echo "    anchortree navigated PAST a home page to a real content page on a"
echo "    self-contained data-baked site (gitlab issues), captured the navigation,"
echo "    and the upstream scorer agreed — refuting the map 404 as image-specific."
