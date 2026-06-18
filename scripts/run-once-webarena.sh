#!/usr/bin/env bash
#
# run-once-webarena.sh — boot ONE real WebArena-Verified site, capture one task
# page into a self-contained HAR live, then replay it with the site torn down
# and mint durable anchortree eids over the result. This is the Phase 3.5b Tier 2
# growth datapoint (DECISIONS D43): the pure-Rust observe loop run end-to-end at
# M=1 against a genuine, server-rendered application page — no bespoke fixture,
# no instrumentation hooks of ours.
#
# Where run-once-m1.sh drives the in-repo `m1-site` fixture (a single static file
# with `__atRerender`/`__atReorder` hooks) to MEASURE a head-to-head, this script
# makes no assumption about the page: it stands up an actual WebArena site image,
# captures whatever it serves, and proves anchortree reconstructs and observes it
# entirely offline. The replay half uses `webarena_observe` (the general
# replay-and-observe rail), not `webarena_replay` (which is fixture-bound).
#
# This is an OPERATIONAL script, not a CI gate: it needs Docker and a real
# browser, and the site image is multi-GB. CI never runs it.
#
# ## What it does
#
#   1. `docker run` the per-site image as a sibling container.
#   2. `docker network connect` it to phantom's user-defined network so it is
#      reachable by container DNS (a bare `docker run -p` publishes on the HOST,
#      not on phantom's loopback — phantom and the sibling are otherwise on
#      different bridge networks and cannot see each other).
#   3. Wait for the site to answer over container DNS.
#   4. `webarena_capture` -> a self-contained inline-body `network.har` plus the
#      `agent_response.json` the WebArena evaluator scores from.
#   5. Tear the site down, then `webarena_observe` against the HAR: every request
#      is answered from the recording or honestly failed, the browser never
#      touches the network, and the observe loop mints durable eids over the
#      real page.
#
# ## Usage
#
#   bash scripts/run-once-webarena.sh
#
# ## Environment overrides (all optional; defaults are the verified `map` site —
#    the smallest per-site image, ~1.19 GB compressed)
#
#   SITE_IMAGE   per-site image            (default am1n3e/webarena-verified-map)
#   SITE_NAME    sibling container name    (default at-wa-map)
#   SITE_PORT    in-container HTTP port    (default 8080, apache)
#   TASK_PATH    path to capture           (default /about)
#   DOCKER_NET   phantom's docker network  (default phantom_phantom-net)
#   CHROME_BIN   CDP-capable headless shell
#   CDP_PORT     remote debugging port     (default 9222)
#   KEEP_SITE=1       leave the site container running on success
#   KEEP_ARTIFACTS=1  leave the captured HAR + temp dirs in place on success
#
# Exit 0 means the Tier-2 M=1 was recorded: a real WebArena page reconstructed
# from a HAR with no live origin, with at least one durable eid minted over it.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

SITE_IMAGE="${SITE_IMAGE:-am1n3e/webarena-verified-map}"
SITE_NAME="${SITE_NAME:-at-wa-map}"
SITE_PORT="${SITE_PORT:-8080}"
TASK_PATH="${TASK_PATH:-/about}"
DOCKER_NET="${DOCKER_NET:-phantom_phantom-net}"
CHROME_BIN="${CHROME_BIN:-$HOME/.cache/ms-playwright/chromium_headless_shell-1217/chrome-headless-shell-linux64/chrome-headless-shell}"
CDP_PORT="${CDP_PORT:-9222}"

TASK_URL="http://${SITE_NAME}:${SITE_PORT}${TASK_PATH}"
HAR_OUT="$(mktemp -d)/anchortree-capture-out"
PROFILE_DIR="$(mktemp -d)"

command -v docker >/dev/null 2>&1 || { echo "error: docker not on PATH" >&2; exit 1; }
if [[ ! -x "$CHROME_BIN" ]]; then
  echo "error: no CDP-capable Chrome at $CHROME_BIN (set CHROME_BIN)" >&2
  exit 1
fi

# Pre-build the two examples BEFORE launching the browser. The phantom container
# runs under a tight pids cgroup (pids.max=256, threads included); a headless
# Chrome holds ~150 of those, and a concurrent `cargo`/`rustc`/`ld` then fails to
# spawn its own threads (EAGAIN, surfacing as a compiler or linker abort). Build
# with the browser down, run with it up, and the two never contend.
echo "==> pre-building examples (browser down to keep pids headroom)"
cargo build -q -p anchortree-cdp --example webarena_capture --example webarena_observe

CHROME_PID=""
SITE_STARTED=""
cleanup() {
  [[ -n "$CHROME_PID" ]] && kill "$CHROME_PID" 2>/dev/null || true
  if [[ -n "$SITE_STARTED" && "${KEEP_SITE:-0}" != "1" ]]; then
    docker rm -f "$SITE_NAME" >/dev/null 2>&1 || true
  fi
  if [[ "${KEEP_ARTIFACTS:-0}" != "1" ]]; then
    rm -rf "$PROFILE_DIR" "$(dirname "$HAR_OUT")" 2>/dev/null || true
  fi
}
trap cleanup EXIT

echo "==> booting $SITE_IMAGE as sibling container '$SITE_NAME'"
docker rm -f "$SITE_NAME" >/dev/null 2>&1 || true
docker run -d --name "$SITE_NAME" "$SITE_IMAGE" >/dev/null
SITE_STARTED=1

# A bare sibling lands on the default bridge, isolated from phantom. Joining it to
# phantom's user-defined network makes it resolvable by container DNS name.
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
curl -sf "$TASK_URL" >/dev/null \
  || { echo "error: site never served $TASK_URL" >&2; exit 1; }
echo "    site is live ($(curl -s "$TASK_URL" | grep -o '<title>[^<]*</title>' | head -1))"

echo "==> capturing a self-contained HAR (bodies inlined)"
ANCHORTREE_CDP_HTTP="http://127.0.0.1:${CDP_PORT}" \
ANCHORTREE_CAPTURE_URL="$TASK_URL" \
ANCHORTREE_CAPTURE_OUT="$HAR_OUT" \
  cargo run -q -p anchortree-cdp --example webarena_capture

HAR_FILE="$HAR_OUT/network.har"
[[ -f "$HAR_FILE" ]] || { echo "error: capture produced no HAR at $HAR_FILE" >&2; exit 1; }
grep -q '"text"' "$HAR_FILE" \
  || { echo "error: captured HAR has no inline body — replay would fulfill nothing" >&2; exit 1; }
echo "==> banked $HAR_FILE ($(wc -c <"$HAR_FILE") bytes)"

# Tear the site down BEFORE replay so the proof is airtight: nothing the observe
# loop sees can come from a live origin.
echo "==> tearing the site down (replay must touch no live origin)"
docker rm -f "$SITE_NAME" >/dev/null 2>&1 || true
SITE_STARTED=""

echo "==> replaying the HAR with no live origin and observing (Tier-2 M=1 proof)"
ANCHORTREE_CDP_HTTP="http://127.0.0.1:${CDP_PORT}" \
ANCHORTREE_REPLAY_HAR="$HAR_FILE" \
ANCHORTREE_REPLAY_URL="$TASK_URL" \
  cargo run -q -p anchortree-cdp --example webarena_observe

echo
echo "OK: Tier-2 M=1 recorded — a real WebArena-Verified page ($SITE_IMAGE) was"
echo "    captured live, reconstructed ENTIRELY from the recorded HAR with the"
echo "    site torn down, and anchortree minted durable eids over it. No live"
echo "    origin was touched during replay."
