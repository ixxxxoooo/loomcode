#!/bin/sh
# 重启开发版:先编译,成功后杀掉所有旧的开发版实例,再启动唯一的新实例。
# 开发版带 UCODE_DEV=1,界面标题栏会显示「DEV」标识。
# 用法:scripts/dev.sh [--demo]     日志:target/dev.log
#
# @author ixxxxoooo
set -e
cd "$(dirname "$0")/.."
BIN="$PWD/target/debug/ucode"
PID_FILE="$PWD/target/dev.pid"

# 先编译:失败时保留旧实例,不会出现「旧的杀了、新的没起来」
cargo build -p ucode-app

# 杀掉过往的开发版(按 pid 文件 + 按二进制路径兜底;不会动 dist/UCode.app 的打包版)
if [ -f "$PID_FILE" ]; then
  kill "$(cat "$PID_FILE")" 2>/dev/null || true
fi
# 路径特征匹配(忽略大小写、相对/绝对路径),兼容手动 cargo run 起的实例
pkill -if "target/debug/ucode( |\$)" 2>/dev/null || true
sleep 0.5

UCODE_DEV=1 nohup "$BIN" "$@" >"$PWD/target/dev.log" 2>&1 &
echo $! >"$PID_FILE"
echo "开发版已启动(pid $(cat "$PID_FILE"))"
