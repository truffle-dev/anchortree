#!/usr/bin/env bash
#
# run-once-admin-nav.sh — the data-backed NAVIGATE datapoint (DECISIONS D45
# item 2). Where run-once-eval.sh scored a NAVIGATE to the map HOME page (no data
# dependency) and run-once-retrieve.sh scored a RETRIEVE off an authenticated
# admin grid, this script proves the NAVIGATE contract reaches a real CONTENT
# page PAST a home page on a self-contained, data-baked site — refuting the map
# /way/ 404 as image-specific (the slim map image ships no OSM data; the
# shopping_admin image ships its Magento store, including its baked-in themes).
#
# The research pick (D46) was gitlab task 45. The gitlab-ce image extracts to
# ~12GB+ and would not fit the available overlay headroom (extract failed with
# "no space left on device"), and reclaiming that space means deleting other
# projects' images. The shopping_admin image is already cached and proven to boot
# (build run 38). Its admin theme pages are equally a content-page-past-home on a
# data-baked site, so they refute the same image-specific-404 claim without the
# multi-GB gitlab pull. See BUILD_LOG run 39 / DECISIONS D46 for the trade.
#
# ## The task (task 157): a shopping_admin NAVIGATE task. Intent "View the details
# ## of all customers", start_urls = ['__SHOPPING_ADMIN__']. Two evaluators:
#   1. AgentResponseEvaluator expects {NAVIGATE, SUCCESS, null} (results_schema
#      null — a pure reach-a-URL contract, no retrieved data).
#   2. NetworkEventEvaluator expects the navigation URL
#      __SHOPPING_ADMIN__/customer/index/ with response_status 200 (exact, no
#      regex, no product/selection reasoning — a pure navigation proof).
#
# ## The __SHOPPING_ADMIN__ base includes /admin. The placeholder maps to the
# admin base (http://<host>/admin), so __SHOPPING_ADMIN__/customer/index/ renders
# to http://at-sa/admin/customer/index/ — the admin customer grid, which serves
# 200 once authenticated. (The dataset's theme tasks carry a stray second /admin
# segment and 404 on this image's Magento build, so the customer grid is the
# clean, 200-serving content page to prove the NAVIGATE-to-content contract.)
#
# anchortree logs into the admin, navigates Chrome to the customer grid on the
# live store, captures the navigation document HAR, emits
# AgentResponse::completed(NAVIGATE), tears the site down, and scores offline. The
# evaluator normalises the captured real URL (http://at-sa/admin/...) back to
# __SHOPPING_ADMIN__/... via the config environment, so the rendered expected URL
# matches what we navigated to. The grid is behind the admin login, so the
# (login-capable) webarena_capture example authenticates first.
#
# ## The Magento base_url pin (as in run-once-retrieve.sh): the shopping_admin
# image ships base_url=localhost:7780, which 302-redirects every container-DNS
# request. We point base_url at the sibling hostname and flush cache so
# http://at-sa/ serves 200 instead of bouncing to localhost.
#
# This is an OPERATIONAL script, not a CI gate: it needs Docker, a real browser,
# and the multi-GB shopping_admin image. CI never runs it.
#
# ## Usage
#
#   bash scripts/run-once-admin-nav.sh
#
# ## Environment overrides (all optional; defaults are task 157 on shopping_admin)
#
#   EVAL_IMAGE   evaluator image   (default ghcr.io/servicenow/webarena-verified:latest)
#   SITE_IMAGE   per-site image    (default am1n3e/webarena-verified-shopping_admin)
#   SITE_NAME    sibling name      (default at-sa)
#   TASK_ID      WebArena task id  (default 157)
#   TASK_PATH    path to capture   (default /admin/customer/index/)
#   SITE_KEY     config site key   (default shopping_admin)
#   DOCKER_NET   phantom net       (default phantom_phantom-net)
#   CHROME_BIN   CDP-capable headless shell
#   CDP_PORT     remote debugging port (default 9222)
#   ADMIN_USER / ADMIN_PASS  admin creds (default admin / admin1234)
#   KEEP_SITE=1       leave the site container running on success
#   KEEP_ARTIFACTS=1  leave the output dir + eval_result.json in place on success
#
# Exit 0 means the external evaluator scored the captured NAVIGATE task 1.0.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

EVAL_IMAGE="${EVAL_IMAGE:-ghcr.io/servicenow/webarena-verified:latest}"
SITE_IMAGE="${SITE_IMAGE:-am1n3e/webarena-verified-shopping_admin}"
SITE_NAME="${SITE_NAME:-at-sa}"
TASK_ID="${TASK_ID:-157}"
TASK_PATH="${TASK_PATH:-/admin/customer/index/}"
SITE_KEY="${SITE_KEY:-shopping_admin}"
DOCKER_NET="${DOCKER_NET:-phantom_phantom-net}"
CHROME_BIN="${CHROME_BIN:-$HOME/.cache/ms-playwright/chromium_headless_shell-1217/chrome-headless-shell-linux64/chrome-headless-shell}"
CDP_PORT="${CDP_PORT:-9222}"
ADMIN_USER="${ADMIN_USER:-admin}"
ADMIN_PASS="${ADMIN_PASS:-admin1234}"

# Magento admin serves on port 80 inside the image; we reach it by container DNS.
SITE_BASE="http://${SITE_NAME}"
LOGIN_URL="${SITE_BASE}/admin"
TASK_URL="${SITE_BASE}${TASK_PATH}"
# The __SHOPPING_ADMIN__ placeholder maps to the admin base (host + /admin), so
# the evaluator normalises http://at-sa/admin/customer/index/ back to
# __SHOPPING_ADMIN__/customer/index/ to match the task's expected URL exactly.
ADMIN_BASE="${SITE_BASE}/admin"

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

# The NetworkEventEvaluator renders the task's __SHOPPING_ADMIN__ placeholder
# against this config. The placeholder maps to the admin base (host + /admin),
# so we point the config at ADMIN_BASE: the captured http://at-sa/admin/customer/
# index/ then normalises back to __SHOPPING_ADMIN__/customer/index/, matching the
# task's expected URL exactly.
cat >"$CONFIG_FILE" <<JSON
{
  "environments": {
    "$SITE_KEY": {
      "urls": ["$ADMIN_BASE"]
    }
  }
}
JSON

# Pre-build the example BEFORE launching the browser: the phantom container's
# pids cgroup (pids.max=256, threads included) cannot fit a headless Chrome and
# a concurrent rustc/ld at once. Build down, run up.
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

# Wait for a REAL Magento response before pinning base_url. A 502/503 is nginx
# unable to reach php-fpm (still warming); a 200 or 302 is Magento itself
# answering (php-fpm + MySQL up). Pinning before MySQL is ready silently no-ops
# the base_url UPDATE, leaving the 302-redirect loop. So wait past the gateway
# errors first.
echo "==> waiting for a real Magento response (past 502/503; php-fpm + db warm-up)"
for _ in $(seq 1 240); do
  CODE="$(curl -s -o /dev/null -w '%{http_code}' "$LOGIN_URL" || true)"
  [[ "$CODE" == "200" || "$CODE" == "302" ]] && break
  sleep 1
done
echo "    $LOGIN_URL first real response HTTP $CODE"

# The shopping_admin image ships base_url=localhost:7780, which 302-redirects
# every container-DNS request. Point it at the sibling hostname and flush cache
# so /admin serves 200 under http://at-sa/. cache:flush can lag, and the DB may
# still be settling, so pin-and-verify in a loop until /admin answers 200.
echo "==> pinning Magento base_url to $SITE_BASE/ (pin-and-verify loop)"
HTTP_CODE=""
for attempt in $(seq 1 10); do
  docker exec "$SITE_NAME" bash -lc "
    mysql -u magentouser -pMyPassword magentodb -e \"
      UPDATE core_config_data SET value='${SITE_BASE}/'
      WHERE path IN ('web/unsecure/base_url','web/secure/base_url',
                     'web/unsecure/base_link_url','web/secure/base_link_url');\" 2>/dev/null
    php /var/www/magento2/bin/magento cache:flush >/dev/null 2>&1 || true
  " || echo "    (base_url pin attempt $attempt hit an error; continuing)"
  for _ in $(seq 1 15); do
    HTTP_CODE="$(curl -s -o /dev/null -w '%{http_code}' "$LOGIN_URL" || true)"
    [[ "$HTTP_CODE" == "200" ]] && break
    sleep 1
  done
  [[ "$HTTP_CODE" == "200" ]] && { echo "    pinned on attempt $attempt"; break; }
  echo "    attempt $attempt: $LOGIN_URL -> HTTP $HTTP_CODE; retrying pin"
done
echo "    $LOGIN_URL -> HTTP $HTTP_CODE"
[[ "$HTTP_CODE" == "200" ]] \
  || { echo "error: admin login page did not serve 200 (got $HTTP_CODE)" >&2; exit 1; }

# Login JS: fill the admin form and submit it. Magento admin login fields are
# id="username" (login[username]) and id="login" (login[password]).
LOGIN_JS="(function(){var u=document.getElementById('username');var p=document.getElementById('login');if(!u||!p){return 'no-form';}u.value=${ADMIN_USER@Q};p.value=${ADMIN_PASS@Q};u.form.submit();return 'submitted';})()"

echo "==> capturing the authenticated NAVIGATE (login + customer grid) into $TASK_OUT"
ANCHORTREE_CDP_HTTP="http://127.0.0.1:${CDP_PORT}" \
ANCHORTREE_LOGIN_URL="$LOGIN_URL" \
ANCHORTREE_LOGIN_JS="$LOGIN_JS" \
ANCHORTREE_CAPTURE_URL="$TASK_URL" \
ANCHORTREE_TASK_TYPE="navigate" \
ANCHORTREE_CAPTURE_OUT="$TASK_OUT" \
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
echo "    anchortree authenticated, navigated PAST the dashboard home to a real"
echo "    content page (the admin customer grid) on a self-contained data-baked"
echo "    site, captured the navigation, and the upstream scorer agreed — both the"
echo "    AgentResponseEvaluator (NAVIGATE/SUCCESS) and the NetworkEventEvaluator"
echo "    (exact URL + 200) — refuting the map 404 as image-specific."
