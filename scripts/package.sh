#!/bin/sh
# 打包 macOS 应用:scripts/package.sh [--dmg]
# 产物在 dist/UCode.app(加 --dmg 时同时生成 dist/UCode.dmg)。不提交构建产物(dist 已在 .gitignore)。
set -eu
cd "$(dirname "$0")/.."

VERSION=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="ucode-app"))')

echo "==> 编译发布版 (v$VERSION)"
cargo build --release -p ucode-app

APP=dist/UCode.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/ucode "$APP/Contents/MacOS/ucode"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>UCode</string>
  <key>CFBundleDisplayName</key><string>UCode</string>
  <key>CFBundleIdentifier</key><string>dev.ucode.app</string>
  <key>CFBundleExecutable</key><string>ucode</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>CFBundleDevelopmentRegion</key><string>zh_CN</string>
</dict>
</plist>
PLIST

# 本地即用的临时签名(分发请换成开发者证书与公证)
codesign --force --sign - "$APP" >/dev/null 2>&1 && echo "==> 已做临时签名" || echo "==> 跳过签名(未找到 codesign)"

if [ "${1:-}" = "--dmg" ]; then
  rm -f dist/UCode.dmg
  hdiutil create -volname UCode -srcfolder "$APP" -ov -format UDZO dist/UCode.dmg >/dev/null
  echo "==> 已生成 dist/UCode.dmg"
fi
echo "==> 完成:$APP ($(du -sh "$APP" | cut -f1))"
