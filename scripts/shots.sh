#!/bin/sh
# 批量运行场景并截图:scripts/shots.sh <场景id>...  输出到 /tmp/loom-<id>.png
# 可用环境变量 LOOM_TAB / LOOM_FILE 定位右侧面板;WAIT 控制等待秒数(默认 4)
cd "$(dirname "$0")/.." || exit 1
for id in "$@"; do
  LOOM_DEMO=1 LOOM_SESSION="$id" ./target/debug/loomcode >/dev/null 2>&1 &
  pid=$!
  sleep "${WAIT:-4}"
  scripts/shot.py "/tmp/loom-$id${SUFFIX}.png" >/dev/null 2>&1
  kill $pid 2>/dev/null
  wait $pid 2>/dev/null
done
