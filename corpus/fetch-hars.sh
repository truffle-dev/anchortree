#!/usr/bin/env bash
# Fetch the (large, git-ignored) network.har for each vendored demo task from
# ServiceNow/webarena-verified. The HARs are the replayable precondition for the
# 3.5b baseline-capture step; the score axis (eval_result.json) needs none of
# this. Source: https://github.com/ServiceNow/webarena-verified (Apache-2.0).
#
# Usage:  bash corpus/fetch-hars.sh
# Requires: gh (authenticated) — uses the contents API so it works on private CI.
set -euo pipefail

REPO="ServiceNow/webarena-verified"
SRC="examples/agent_logs/demo"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

for task in 107 108; do
  dest="$HERE/$task/network.har"
  echo "fetching $task/network.har ..."
  gh api "repos/$REPO/contents/$SRC/$task/network.har" \
    --jq '.content' | base64 -d > "$dest"
  bytes=$(wc -c < "$dest")
  echo "  wrote $dest ($bytes bytes)"
done

echo "done. HARs are git-ignored; they mark each task is_replayable for 3.5b."
