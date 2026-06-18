#!/usr/bin/env bash
#
# run-once-retrieve.sh — the first RETRIEVE datapoint against the genuine
# WebArena-Verified evaluator (DECISIONS D45). Where run-once-eval.sh proves the
# NAVIGATE contract (task 356, reach the map home page, score 1.0), this script
# proves the typed-data extraction path D44 deferred: log into an authenticated
# Magento store admin, navigate to the filtered product-review grid, read the
# total the *site itself renders*, emit AgentResponse::retrieved(<count>), and
# have the upstream scorer agree it equals the task's expected answer.
#
# ## The task (task 11): a RETRIEVE/data-validation task whose intent is
# ## "Get the total number of reviews that our store received so far that mention
# ## term 'disappointed'" with expected retrieved_data == [6].
#
# Task 11 has ONLY an AgentResponseEvaluator (no NetworkEventEvaluator), so the
# score depends solely on agent_response.json. The evaluator wraps a scalar into
# a tuple before comparing, so emitting the JSON number 6 normalises to (6,) and
# matches the expected [6]. (results_schema is {type: array, items: number}.)
#
# ## The honest mechanism
#
# anchortree drives the authenticated admin session and reads the count Magento
# server-renders into `#reviewGrid-total-count` at the filtered grid URL. The
# filter is encoded as a base64 PATH segment: base64("detail=disappointed") =
# ZGV0YWlsPWRpc2FwcG9pbnRlZA==, so the grid page renders "6 records found" in its
# initial HTML (no async JS). anchortree reads that 6. It does not query the DB
# or assert its own answer: if the store held a different number, anchortree
# would report that and the task would score 0.
#
# This is an OPERATIONAL script, not a CI gate: it needs Docker, a real browser,
# and the multi-GB shopping_admin image. CI never runs it.
#
# ## Usage
#
#   bash scripts/run-once-retrieve.sh
#
# ## Environment overrides (all optional; defaults are task 11 on shopping_admin)
#
#   EVAL_IMAGE   evaluator image   (default ghcr.io/servicenow/webarena-verified:latest)
#   SITE_IMAGE   per-site image    (default am1n3e/webarena-verified-shopping_admin)
#   SITE_NAME    sibling name      (default at-sa)
#   TASK_ID      WebArena task id  (default 11)
#   SITE_KEY     config site key   (default shopping_admin)
#   DOCKER_NET   phantom net       (default phantom_phantom-net)
#   CHROME_BIN   CDP-capable headless shell
#   CDP_PORT     remote debugging port (default 9222)
#   ADMIN_USER / ADMIN_PASS  admin creds (default admin / admin1234)
#   KEEP_SITE=1       leave the site container running on success
#   KEEP_ARTIFACTS=1  leave the output dir + eval_result.json in place on success
#
# Exit 0 means the external evaluator scored the captured RETRIEVE task 1.0.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

EVAL_IMAGE="${EVAL_IMAGE:-ghcr.io/servicenow/webarena-verified:latest}"
SITE_IMAGE="${SITE_IMAGE:-am1n3e/webarena-verified-shopping_admin}"
SITE_NAME="${SITE_NAME:-at-sa}"
TASK_ID="${TASK_ID:-11}"
SITE_KEY="${SITE_KEY:-shopping_admin}"
DOCKER_NET="${DOCKER_NET:-phantom_phantom-net}"
CHROME_BIN="${CHROME_BIN:-$HOME/.cache/ms-playwright/chromium_headless_shell-1217/chrome-headless-shell-linux64/chrome-headless-shell}"
CDP_PORT="${CDP_PORT:-9222}"
ADMIN_USER="${ADMIN_USER:-admin}"
ADMIN_PASS="${ADMIN_PASS:-admin1234}"

# Magento admin serves on port 80 inside the image; we reach it by container DNS.
SITE_BASE="http://${SITE_NAME}"
LOGIN_URL="${SITE_BASE}/admin"
# The filtered review grid: base64("detail=<term>") as a PATH segment. Task 11
# filters on "disappointed" (default); sibling tasks in the same template family
# (e.g. task 15, intent_template_id 288, term "best") only swap FILTER_B64, so
# both are env-overridable to score the family from one harness.
FILTER_B64="${FILTER_B64:-ZGV0YWlsPWRpc2FwcG9pbnRlZA==}"
GRID_URL="${GRID_URL:-${SITE_BASE}/admin/review/product/index/filter/${FILTER_B64}/}"

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

# Config: task 11 has no network evaluator, so the rendered URL is irrelevant to
# scoring, but eval-tasks still wants the environment present.
cat >"$CONFIG_FILE" <<JSON
{
  "environments": {
    "$SITE_KEY": {
      "urls": ["$SITE_BASE"]
    }
  }
}
JSON

# Pre-build the example BEFORE launching the browser: the phantom container's
# pids cgroup (pids.max=256, threads included) cannot fit a headless Chrome and
# a concurrent rustc/ld at once. Build down, run up.
echo "==> pre-building webarena_retrieve (browser down to keep pids headroom)"
CARGO_INCREMENTAL=0 GOMAXPROCS=1 RAYON_NUM_THREADS=1 CARGO_BUILD_JOBS=1 \
  cargo build -q -p anchortree-cdp --example webarena_retrieve

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

# Reuse an already-running site if present (the de-risk loop leaves it up);
# otherwise boot a fresh one.
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
# the base_url UPDATE, leaving the 302-redirect loop. So wait past gateway errors.
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
# Read JS: the grid renders the total into #reviewGrid-total-count.
READ_JS="(function(){var e=document.getElementById('reviewGrid-total-count');return e?e.textContent.trim():'';})()"

echo "==> capturing the authenticated RETRIEVE (login + filtered grid read) into $TASK_OUT"
ANCHORTREE_CDP_HTTP="http://127.0.0.1:${CDP_PORT}" \
ANCHORTREE_LOGIN_URL="$LOGIN_URL" \
ANCHORTREE_LOGIN_JS="$LOGIN_JS" \
ANCHORTREE_CAPTURE_URL="$GRID_URL" \
ANCHORTREE_READ_JS="$READ_JS" \
ANCHORTREE_RETRIEVE_NUMBER="1" \
ANCHORTREE_CAPTURE_OUT="$TASK_OUT" \
  cargo run -q -p anchortree-cdp --example webarena_retrieve

[[ -f "$TASK_OUT/network.har" ]] || { echo "error: no network.har at $TASK_OUT" >&2; exit 1; }
[[ -f "$TASK_OUT/agent_response.json" ]] || { echo "error: no agent_response.json at $TASK_OUT" >&2; exit 1; }

echo "==> agent_response.json:"
cat "$TASK_OUT/agent_response.json"
echo

# Tear the site down BEFORE scoring (unless reused): the evaluator is a pure
# offline scorer that reads only the captured files.
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
echo "OK: external WebArena-Verified evaluator scored RETRIEVE task $TASK_ID = 1.0."
echo "    anchortree drove the authenticated admin session, read the count Magento"
echo "    itself server-rendered into #reviewGrid-total-count for the requested"
echo "    filter, emitted it as retrieved_data, and the upstream scorer agreed —"
echo "    with its own evaluator + data checksums stamped."
