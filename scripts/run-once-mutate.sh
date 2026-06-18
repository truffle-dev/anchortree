#!/usr/bin/env bash
#
# run-once-mutate.sh — the first data-backed MUTATE datapoint (DECISIONS D48/D49).
# Where run-once-admin-nav.sh scored a NAVIGATE and run-once-retrieve.sh scored a
# RETRIEVE, this script closes the third axis: it drives a real Magento admin CMS
# save, captures the mutating POST request (body included, via the run-41
# getRequestPostData wiring), tears the site down, and the upstream
# WebArena-Verified evaluator scores the MUTATE 1.0 offline.
#
# ## The task (task 488): a shopping_admin MUTATE task. Intent "Change the home
# ## page CMS title". Two evaluators:
#   1. AgentResponseEvaluator expects {MUTATE, SUCCESS, null} (no retrieved data).
#   2. NetworkEventEvaluator expects the save POST:
#      url            __SHOPPING_ADMIN__/cms/page/save/back/edit  (exact)
#      http_method    POST
#      response_status 302   (the Magento "save & continue edit" redirect)
#      post_data      a SUBSET of the submitted form fields, matched first-value-
#                     per-key after URL-decoding: title, is_active=1,
#                     store_id[0]=0, page_id=2. (The evaluator only checks this
#                     subset is PRESENT; the full Magento form — form_key, content,
#                     content_heading, ... — must still be submitted for the save to
#                     succeed and 302. So we submit the real, full form, not 4 fields.)
#
# ## How the MUTATE is driven. anchortree logs into the admin, navigates Chrome to
# the CMS page edit form for page_id 2 (the home page), then runs a fill+submit
# hook (ANCHORTREE_MUTATE_JS): it sets the title input and does a NATIVE full-form
# POST to .../admin/cms/page/save/back/edit. A native POST is what 302-redirects
# (an AJAX save would return 200 + JSON and would not be scored), and it serializes
# every form field — including form_key and the store_id[0]=0 view scope — so the
# scored post_data subset is present in a real, complete save. The save mutates the
# fixture, but the container is booted fresh and torn down each run, so the change
# is ephemeral and safe.
#
# ## The base_url pin and the config placeholder map are identical to the nav
# script: the image ships base_url=localhost:7780 (302-loops on container DNS), so
# we pin base_url to the sibling hostname and flush cache; and __SHOPPING_ADMIN__
# maps to the admin base (host + /admin) so the captured
# http://at-sa/admin/cms/page/save/back/edit normalises back to
# __SHOPPING_ADMIN__/cms/page/save/back/edit, matching the task exactly.
#
# This is an OPERATIONAL script, not a CI gate: it needs Docker, a real browser,
# and the multi-GB shopping_admin image. CI never runs it.
#
# ## Usage
#
#   bash scripts/run-once-mutate.sh
#
# ## Environment overrides (all optional; defaults are task 488 on shopping_admin)
#
#   EVAL_IMAGE    evaluator image   (default ghcr.io/servicenow/webarena-verified:latest)
#   SITE_IMAGE    per-site image    (default am1n3e/webarena-verified-shopping_admin)
#   SITE_NAME     sibling name      (default at-sa)
#   TASK_ID       WebArena task id  (default 488)
#   PAGE_ID       CMS page to edit  (default 2 — the home page)
#   MUTATE_TITLE  new title value   (default "This is the home page!! Leave here!!")
#   SITE_KEY      config site key   (default shopping_admin)
#   DOCKER_NET    phantom net       (default phantom_phantom-net)
#   CHROME_BIN    CDP-capable headless shell
#   CDP_PORT      remote debugging port (default 9222)
#   ADMIN_USER / ADMIN_PASS  admin creds (default admin / admin1234)
#   KEEP_SITE=1       leave the site container running on success
#   KEEP_ARTIFACTS=1  leave the output dir + eval_result.json in place on success
#
# Exit 0 means the external evaluator scored the captured MUTATE task 1.0.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

EVAL_IMAGE="${EVAL_IMAGE:-ghcr.io/servicenow/webarena-verified:latest}"
SITE_IMAGE="${SITE_IMAGE:-am1n3e/webarena-verified-shopping_admin}"
SITE_NAME="${SITE_NAME:-at-sa}"
TASK_ID="${TASK_ID:-488}"
PAGE_ID="${PAGE_ID:-2}"
MUTATE_TITLE="${MUTATE_TITLE:-This is the home page!! Leave here!!}"
SITE_KEY="${SITE_KEY:-shopping_admin}"
DOCKER_NET="${DOCKER_NET:-phantom_phantom-net}"
CHROME_BIN="${CHROME_BIN:-$HOME/.cache/ms-playwright/chromium_headless_shell-1217/chrome-headless-shell-linux64/chrome-headless-shell}"
CDP_PORT="${CDP_PORT:-9222}"
ADMIN_USER="${ADMIN_USER:-admin}"
ADMIN_PASS="${ADMIN_PASS:-admin1234}"

# Magento admin serves on port 80 inside the image; we reach it by container DNS.
SITE_BASE="http://${SITE_NAME}"
LOGIN_URL="${SITE_BASE}/admin"
# The CMS page edit form for the home page; the title input lives here and the
# save POST is fired from it.
TASK_URL="${SITE_BASE}/admin/cms/page/edit/page_id/${PAGE_ID}/"
# The __SHOPPING_ADMIN__ placeholder maps to the admin base (host + /admin), so
# the evaluator normalises the captured save POST
# http://at-sa/admin/cms/page/save/back/edit back to
# __SHOPPING_ADMIN__/cms/page/save/back/edit to match the task's url exactly.
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
# so we point the config at ADMIN_BASE: the captured save POST then normalises
# back to __SHOPPING_ADMIN__/cms/page/save/back/edit, matching the task exactly.
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

# Mutate JS: the Magento admin CMS page form is a UI component (knockout), not a
# native <form> the browser can submit directly; the only real <form> on the page
# is the header search box. So we drive the form the way the UI does: set the
# title input through the native value setter and dispatch input+change so the
# knockout observable behind the field updates, then click the real "Save" button
# (#save-button). That main Save is "Save & Continue Edit", which POSTs the full
# form (form_key, content, is_active, store_id[0], page_id, ...) to
# cms/page/save/back/edit and 302-redirects back to the editor — exactly the url +
# method + status + post_data subset the NetworkEventEvaluator scores.
#
# The home page (page_id 2) is a PageBuilder page, so the field and button DOM
# appear well before the UI components finish wiring their click handlers. Click
# Save too early and it is a silent no-op — no POST. So the hook gates on page
# quiescence (document complete, no visible loading mask, jQuery idle) sustained
# for several polls before it acts, then sets the title and waits one more poll to
# confirm the value persisted (knockout can re-render and revert it) before
# clicking Save. It returns a "waiting:*" sentinel until then and "submitted" once
# it has clicked Save; the example polls it until it submits.
MUTATE_JS="(function(){var st=window.__atm||(window.__atm={q:0});var t=document.querySelector('input[name=\"title\"]');if(!t){return 'waiting:title';}var b=document.getElementById('save-button')||document.querySelector('[data-ui-id=\"save-button\"]');if(!b||b.disabled){return 'waiting:savebtn';}var busy=document.readyState!=='complete';if(!busy){var ms=document.querySelectorAll('.loading-mask,[data-role=\"loader\"],.admin__form-loading-mask');for(var i=0;i<ms.length;i++){if(ms[i].offsetParent!==null){busy=true;break;}}}if(!busy&&window.jQuery&&window.jQuery.active>0){busy=true;}if(busy){st.q=0;return 'waiting:busy';}st.q++;if(st.q<3){return 'waiting:settle';}var TARGET=${MUTATE_TITLE@Q};if(t.value!==TARGET){var s=Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype,'value').set;s.call(t,TARGET);t.dispatchEvent(new Event('input',{bubbles:true}));t.dispatchEvent(new Event('change',{bubbles:true}));return 'waiting:titleset';}b.click();return 'submitted';})()"

echo "==> capturing the authenticated MUTATE (login + CMS save POST) into $TASK_OUT"
# Match the pre-build's resource/incremental env exactly so cargo reuses the
# already-built example instead of recompiling it under the live browser: a
# recompile here both competes for the pids budget and, with incremental on, has
# tripped a flaky LLVM codegen ICE. Build down, run the SAME artifact up.
CARGO_INCREMENTAL=0 GOMAXPROCS=1 RAYON_NUM_THREADS=1 CARGO_BUILD_JOBS=1 \
ANCHORTREE_CDP_HTTP="http://127.0.0.1:${CDP_PORT}" \
ANCHORTREE_LOGIN_URL="$LOGIN_URL" \
ANCHORTREE_LOGIN_JS="$LOGIN_JS" \
ANCHORTREE_CAPTURE_URL="$TASK_URL" \
ANCHORTREE_TASK_TYPE="mutate" \
ANCHORTREE_MUTATE_JS="$MUTATE_JS" \
ANCHORTREE_CAPTURE_OUT="$TASK_OUT" \
  cargo run -q -p anchortree-cdp --example webarena_capture

[[ -f "$TASK_OUT/network.har" ]] || { echo "error: no network.har at $TASK_OUT" >&2; exit 1; }
[[ -f "$TASK_OUT/agent_response.json" ]] || { echo "error: no agent_response.json at $TASK_OUT" >&2; exit 1; }

# Free the pid budget before any docker step. A live headless Chrome spawns many
# threads, and the phantom container's pids.max=256 is too tight for Chrome plus
# the Go-based docker CLI to coexist (docker aborts with a pthread_create EAGAIN).
# The capture is finished and scoring is offline, so Chrome is no longer needed.
if [[ -n "$CHROME_PID" ]]; then
  kill "$CHROME_PID" 2>/dev/null || true
  CHROME_PID=""
fi

echo "==> agent_response.json:"
cat "$TASK_OUT/agent_response.json"
echo

# Quick local sanity: the HAR must carry the save POST with a postData body, or
# the NetworkEventEvaluator's post_data subset check cannot pass. Surface it early
# so a missing body is diagnosed here, not as an opaque 0.0 from the evaluator.
if command -v python3 >/dev/null 2>&1; then
  python3 - "$TASK_OUT/network.har" <<'PY' || true
import json, sys
har = json.load(open(sys.argv[1]))
posts = [e for e in har["log"]["entries"]
         if e["request"]["method"] == "POST" and "cms/page/save" in e["request"]["url"]]
if not posts:
    print("    WARN: no cms/page/save POST entry in the HAR")
else:
    e = posts[0]
    pd = e["request"].get("postData")
    body = (pd or {}).get("text", "")
    print(f"    save POST: {e['request']['url']}")
    print(f"    response status: {e['response']['status']}")
    print(f"    postData present: {bool(body)} ({len(body)} bytes)")
PY
fi

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
echo "OK: external WebArena-Verified evaluator scored MUTATE task $TASK_ID = 1.0."
echo "    anchortree authenticated, drove a real Magento CMS save (native full-form"
echo "    POST to cms/page/save/back/edit), captured the mutating request body, and"
echo "    the upstream scorer agreed — both the AgentResponseEvaluator"
echo "    (MUTATE/SUCCESS) and the NetworkEventEvaluator (exact url + POST + 302 +"
echo "    post_data subset) — closing the RETRIEVE+NAVIGATE+MUTATE matrix."
