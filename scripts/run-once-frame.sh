#!/usr/bin/env bash
#
# run-once-frame.sh — the FRAME-tier twin of run-once-m1.sh. Record one
# self-contained HAR of a cross-frame page live, then replay it with no live
# origin and run a MEASURED head-to-head over the replayed DOM: anchortree
# durable frame-namespaced identity vs a modelled Stagehand `frameOrdinal`
# resolver, across an inner-frame churn AND a sibling-frame reorder (the
# browser-tied twin of the CI-gated frame-tier head-to-head; DECISIONS D40/D41).
#
# This is an OPERATIONAL script, not a CI gate — it needs a real browser. It
# stands up the in-container Playwright headless-shell and a tiny static page
# whose interactive element lives one frame down inside a same-origin `srcdoc`
# iframe, runs the `webarena_capture` example to bank a SELF-CONTAINED
# `network.har` (srcdoc frames carry no request of their own, so the parent
# document alone is enough), then runs the `webarena_frame_replay` example
# against that HAR. The replay navigates served entirely from the recording.
# The observe loop mints durable frame-namespaced eids over the result, then the
# fixture's own inline scripts churn the checkout frame's card IN PLACE and then
# insert a sibling ad frame AHEAD of it. anchortree rebinds the checkout
# button's eid across both with zero LLM re-grounds; the modelled Stagehand
# frame-ordinal resolver resolves the inner-churn leg for free but pays a
# re-ground on the reorder, where its cached frame ordinal now points at the
# wrong frame. That re-ground is the LLM-call axis as a measured number, not an
# asserted sentence.
#
# Usage:
#   bash scripts/run-once-frame.sh
#
# Environment overrides (all optional):
#   CHROME_BIN   path to a CDP-capable Chrome/headless-shell
#   CDP_PORT     remote debugging port (default 9222)
#   HTTP_PORT    static file server port (default 8081)
#   KEEP_ARTIFACTS=1  leave the captured HAR + temp dirs in place on success
#
# Exit 0 means the frame-tier head-to-head was recorded; any non-zero exit means
# the run did not prove it.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

CHROME_BIN="${CHROME_BIN:-$HOME/.cache/ms-playwright/chromium_headless_shell-1217/chrome-headless-shell-linux64/chrome-headless-shell}"
CDP_PORT="${CDP_PORT:-9222}"
HTTP_PORT="${HTTP_PORT:-8081}"
SITE_DIR="$REPO_ROOT/scripts/fixtures/frame-site"
CAPTURE_URL="http://127.0.0.1:${HTTP_PORT}/index.html"
HAR_OUT="$(mktemp -d)/anchortree-frame-out"
PROFILE_DIR="$(mktemp -d)"

if [[ ! -x "$CHROME_BIN" ]]; then
  echo "error: no CDP-capable Chrome at $CHROME_BIN (set CHROME_BIN)" >&2
  exit 1
fi
if [[ ! -f "$SITE_DIR/index.html" ]]; then
  echo "error: fixture page missing at $SITE_DIR/index.html" >&2
  exit 1
fi

CHROME_PID=""
HTTP_PID=""
cleanup() {
  [[ -n "$HTTP_PID" ]] && kill "$HTTP_PID" 2>/dev/null || true
  [[ -n "$CHROME_PID" ]] && kill "$CHROME_PID" 2>/dev/null || true
  if [[ "${KEEP_ARTIFACTS:-0}" != "1" ]]; then
    rm -rf "$PROFILE_DIR" "$(dirname "$HAR_OUT")" 2>/dev/null || true
  fi
}
trap cleanup EXIT

echo "==> launching headless Chrome on :${CDP_PORT}"
"$CHROME_BIN" \
  --headless --no-sandbox --disable-gpu \
  --remote-debugging-port="${CDP_PORT}" \
  --user-data-dir="$PROFILE_DIR" \
  about:blank >/dev/null 2>&1 &
CHROME_PID=$!

echo "==> serving $SITE_DIR on :${HTTP_PORT}"
( cd "$SITE_DIR" && python3 -m http.server "${HTTP_PORT}" --bind 127.0.0.1 ) >/dev/null 2>&1 &
HTTP_PID=$!

# Wait for both endpoints to answer before driving the examples.
echo "==> waiting for CDP endpoint"
for _ in $(seq 1 50); do
  if curl -sf "http://127.0.0.1:${CDP_PORT}/json/version" >/dev/null 2>&1; then break; fi
  sleep 0.2
done
curl -sf "http://127.0.0.1:${CDP_PORT}/json/version" >/dev/null \
  || { echo "error: CDP endpoint never came up" >&2; exit 1; }

echo "==> waiting for static server"
for _ in $(seq 1 50); do
  if curl -sf "$CAPTURE_URL" >/dev/null 2>&1; then break; fi
  sleep 0.2
done
curl -sf "$CAPTURE_URL" >/dev/null \
  || { echo "error: static server never served the fixture" >&2; exit 1; }

echo "==> capturing a self-contained HAR (bodies inlined)"
ANCHORTREE_CDP_HTTP="http://127.0.0.1:${CDP_PORT}" \
ANCHORTREE_CAPTURE_URL="$CAPTURE_URL" \
ANCHORTREE_CAPTURE_OUT="$HAR_OUT" \
  cargo run -q -p anchortree-cdp --example webarena_capture

HAR_FILE="$HAR_OUT/network.har"
[[ -f "$HAR_FILE" ]] || { echo "error: capture produced no HAR at $HAR_FILE" >&2; exit 1; }

# The recording must actually carry an inline body, or the replay cannot fulfill.
if ! grep -q '"text"' "$HAR_FILE"; then
  echo "error: captured HAR has no inline body — replay would fulfill nothing" >&2
  exit 1
fi
echo "==> banked $HAR_FILE ($(wc -c <"$HAR_FILE") bytes)"

echo "==> replaying the HAR with no live origin (frame-tier head-to-head)"
ANCHORTREE_CDP_HTTP="http://127.0.0.1:${CDP_PORT}" \
ANCHORTREE_REPLAY_HAR="$HAR_FILE" \
ANCHORTREE_REPLAY_URL="$CAPTURE_URL" \
  cargo run -q -p anchortree-cdp --example webarena_frame_replay

echo
echo "OK: frame-tier head-to-head recorded — a cross-frame page reached entirely"
echo "    from a recorded HAR churned its checkout frame's card and then had a"
echo "    sibling ad frame inserted ahead of it; the checkout button's durable eid"
echo "    rebound across both at zero LLM re-grounds while the modelled Stagehand"
echo "    frame-ordinal resolver re-grounded on the frame reorder. No live origin"
echo "    was ever touched."
