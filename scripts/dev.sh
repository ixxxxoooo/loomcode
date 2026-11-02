#!/bin/sh
# 重启开发版:先编译,成功后杀掉所有旧的开发版实例,再启动唯一的新实例。
# 开发版带 LOOM_DEV=1,界面左下角会显示「开发版」徽章(演示模式下显示「演示模式」)。
# 用法:scripts/dev.sh [--demo|--real]     日志:target/dev.log(上一个实例的在 dev.log.prev)
# 不带开关时沿用上次在应用里「切换演示 / 真实」的选择(记在 ~/.loomcode/dev-mode),没有记忆就是真实模式。
# 应用内切换会自己重启并更新 target/dev.pid,始终只留一个实例。
#
# 开发版也跑在一个最小的 .app 包里(target/dev/LoomCode Dev.app):裸二进制没有包,
# 程序坞退出时图标会退回成终端 / exec 图标,系统通知也拿不到应用图标;进了包就都正常了。
#
# @author ixxxxoooo
set -e
cd "$(dirname "$0")/.."
BUILT="$PWD/target/debug/loomcode"
APP="$PWD/target/dev/LoomCode Dev.app"
BIN="$APP/Contents/MacOS/loomcode"
PID_FILE="$PWD/target/dev.pid"

# 先编译:失败时保留旧实例,不会出现「旧的杀了、新的没起来」
cargo build -p loom-app

# 杀掉过往的开发版(按 pid 文件 + 按二进制路径兜底;不会动 dist/LoomCode.app 的打包版)
if [ -f "$PID_FILE" ]; then
  kill "$(cat "$PID_FILE")" 2>/dev/null || true
fi
# 路径特征匹配(忽略大小写、相对/绝对路径),兼容手动 cargo run 起的实例与包内的开发版
pkill -if "target/debug/loomcode( |\$)" 2>/dev/null || true
pkill -if "LoomCode Dev.app/Contents/MacOS/loomcode( |\$)" 2>/dev/null || true
sleep 0.5

# 组装开发版的 .app 包(二进制用 APFS 克隆,几乎不占时间与空间)
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
rm -f "$BIN"
cp -c "$BUILT" "$BIN" 2>/dev/null || cp "$BUILT" "$BIN"
cp assets/icon/LoomCode.icns "$APP/Contents/Resources/LoomCode.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>LoomCode Dev</string>
  <key>CFBundleDisplayName</key><string>LoomCode Dev</string>
  <key>CFBundleIdentifier</key><string>dev.loomcode.app.dev</string>
  <key>CFBundleExecutable</key><string>loomcode</string>
  <key>CFBundleIconFile</key><string>LoomCode</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>dev</string>
  <key>CFBundleVersion</key><string>0</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleDevelopmentRegion</key><string>zh_CN</string>
</dict>
</plist>
PLIST
# 临时签名(系统通知等能力要求有签名的包;失败不影响启动)
codesign --force --sign - "$APP" >/dev/null 2>&1 || true

# 上一个实例的标准错误(Objective-C 异常、系统报错等)留一份,崩溃后重启也不会丢
[ -f "$PWD/target/dev.log" ] && mv -f "$PWD/target/dev.log" "$PWD/target/dev.log.prev"
RUST_BACKTRACE=1 LOOM_DEV=1 nohup "$BIN" "$@" >"$PWD/target/dev.log" 2>&1 &
echo $! >"$PID_FILE"
echo "开发版已启动(pid $(cat "$PID_FILE"))"
