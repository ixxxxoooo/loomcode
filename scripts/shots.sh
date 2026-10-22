#!/bin/sh
# 批量运行场景并截图:scripts/shots.sh <场景id>...  输出到 /tmp/ucode-<id>.png
cd "$(dirname "$0")/.." || exit 1
for id in "$@"; do
  UCODE_DEMO=1 UCODE_SESSION="$id" ./target/debug/ucode >/dev/null 2>&1 &
  pid=$!
  sleep 3.5
  scripts/shot.py "/tmp/ucode-$id.png" >/dev/null 2>&1
  kill $pid 2>/dev/null
  wait $pid 2>/dev/null
done
