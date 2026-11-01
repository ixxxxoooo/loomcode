#!/bin/sh
# 重新生成全部图标:SVG 源文件 → PNG → .icns。需要 python3 + PyObjC(macOS 自带 AppKit 渲染)+ Pillow。
# 产物(都提交进仓库,打包与编译不依赖本脚本):
#   app-light.png / app-dark.png   1024 App 图标(运行时 Dock 图标随主题切换)
#   LoomCode.icns / LoomCode-dark.icns   打包用(Info.plist 用浅色)
#   tray.png                       36×36 菜单栏模板图标(18pt@2x,黑色 + 透明)
#
# @author ixxxxoooo
set -eu
cd "$(dirname "$0")"
python3 gen.py
for v in light dark; do
  python3 render.py "app-$v.svg" "app-$v.png" 1024
  SET=$(mktemp -d)/icon.iconset
  mkdir -p "$SET"
  for s in 16 32 128 256 512; do
    python3 render.py "app-$v.svg" "$SET/icon_${s}x${s}.png" "$s"
    python3 render.py "app-$v.svg" "$SET/icon_${s}x${s}@2x.png" "$((s * 2))"
  done
  NAME=LoomCode; [ "$v" = dark ] && NAME=LoomCode-dark
  iconutil -c icns "$SET" -o "$NAME.icns"
done
python3 render.py tray.svg tray.png 36 --template
ls -lh *.png *.icns
