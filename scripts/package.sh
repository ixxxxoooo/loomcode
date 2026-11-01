#!/bin/sh
# 打包 macOS 应用:scripts/package.sh [--dmg]
# 产物在 dist/LoomCode.app(加 --dmg 时同时生成 dist/LoomCode.dmg)。不提交构建产物(dist 已在 .gitignore)。
set -eu
cd "$(dirname "$0")/.."

VERSION=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="loom-app"))')

# 构建号:取提交数,保证单调递增;编进二进制,用于「检查更新」比较
BUILD="${LOOM_BUILD:-$(git rev-list --count HEAD 2>/dev/null || echo 0)}"
export LOOM_BUILD="$BUILD"

echo "==> 编译发布版 (v$VERSION, build $BUILD)"
cargo build --release -p loom-app

APP=dist/LoomCode.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/loomcode "$APP/Contents/MacOS/loomcode"
# 应用图标(浅色版;运行中 Dock 图标会随主题切到深色版)。重新生成图标:assets/icon/build.sh
cp assets/icon/LoomCode.icns "$APP/Contents/Resources/LoomCode.icns"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>LoomCode</string>
  <key>CFBundleDisplayName</key><string>LoomCode</string>
  <key>CFBundleIdentifier</key><string>dev.loomcode.app</string>
  <key>CFBundleExecutable</key><string>loomcode</string>
  <key>CFBundleIconFile</key><string>LoomCode</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$BUILD</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleDevelopmentRegion</key><string>zh_CN</string>
</dict>
</plist>
PLIST

# 签名:设置 DEVELOPER_ID(如 "Developer ID Application: 名字 (TEAMID)")用开发者证书,否则本机临时签名(分发请公证)
IDENTITY="${DEVELOPER_ID:--}"
if [ "$IDENTITY" = "-" ]; then
  codesign --force --sign - "$APP" >/dev/null 2>&1 && echo "==> 已做临时签名" || echo "==> 跳过签名(未找到 codesign)"
else
  codesign --force --options runtime --timestamp --sign "$IDENTITY" "$APP" && echo "==> 已用开发者证书签名"
fi

if [ "${1:-}" = "--dmg" ]; then
  # 暂存目录放 app 与「Applications」快捷方式,用户可直接拖动安装;产物带版本号,供更新源使用
  STAGE=$(mktemp -d "${TMPDIR:-/tmp}/loom-dmg.XXXXXX")
  trap 'rm -rf "$STAGE"' EXIT
  cp -R "$APP" "$STAGE/LoomCode.app"
  ln -s /Applications "$STAGE/Applications"
  DMG="dist/LoomCode-$VERSION.dmg"
  rm -f dist/LoomCode*.dmg
  hdiutil create -volname "LoomCode $VERSION" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
  echo "==> 已生成 $DMG"
fi
echo "==> 完成:$APP ($(du -sh "$APP" | cut -f1))"
