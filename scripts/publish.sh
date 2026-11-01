#!/bin/sh
# 发布正式版:打包 dmg → 生成 appcast.json → 打 tag → 推送 → 创建 GitHub Release(附 dmg 与 appcast.json)。
# 应用内「检查更新」默认读取 releases/latest/download/appcast.json,发布后自动生效。
#
# 用法:先在根 Cargo.toml 的 [workspace.package] 把 version 改成新版本并提交,再运行  scripts/publish.sh
#   LOOM_NOTES="更新说明"        可选,缺省取上一个 tag 以来的提交标题
#   DEVELOPER_ID="Developer ID Application: 名字 (TEAMID)"   可选,缺省为临时签名(用户首次打开可能需要右键打开)
set -eu
cd "$(dirname "$0")/.."

REPO="ixxxxoooo/loomcode"
VERSION=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="loom-app"))')
case "$VERSION" in [0-9]*.[0-9]*) ;; *) echo "错误:版本号格式不正确:$VERSION"; exit 1;; esac
TAG="v$VERSION"
[ -z "$(git status --porcelain)" ] || { echo "错误:工作区有未提交改动,请先提交"; exit 1; }
git rev-parse "$TAG" >/dev/null 2>&1 && { echo "错误:tag $TAG 已存在,请先在 Cargo.toml 里提升版本号"; exit 1; }
command -v gh >/dev/null || { echo "错误:需要 GitHub CLI(gh)并已登录"; exit 1; }

BUILD=$(git rev-list --count HEAD)
export LOOM_BUILD="$BUILD"
PREV=$(git describe --tags --abbrev=0 2>/dev/null || true)
if [ -z "${LOOM_NOTES:-}" ]; then
  RANGE="${PREV:+$PREV..}HEAD"
  LOOM_NOTES=$(git log "$RANGE" -n 30 --pretty='%s' | paste -sd ';' -)
fi

scripts/package.sh --dmg
DMG="dist/LoomCode-$VERSION.dmg"
SHA=$(shasum -a 256 "$DMG" | awk '{print $1}')

# appcast:发布源,应用只认 stable 通道;notes 用 python 转义,避免引号破坏 JSON
VERSION="$VERSION" BUILD="$BUILD" SHA="$SHA" REPO="$REPO" TAG="$TAG" LOOM_NOTES="$LOOM_NOTES" python3 - <<'PY'
import json, os
e = os.environ
feed = {"stable": {"version": e["VERSION"], "build": int(e["BUILD"]),
        "url": f'https://github.com/{e["REPO"]}/releases/download/{e["TAG"]}/LoomCode-{e["VERSION"]}.dmg',
        "sha256": e["SHA"], "notes": e["LOOM_NOTES"]}}
json.dump(feed, open("dist/appcast.json", "w"), ensure_ascii=False, indent=2)
PY

git tag -a "$TAG" -m "LoomCode $VERSION"
git push origin "$TAG"
gh release create "$TAG" --repo "$REPO" --title "LoomCode $VERSION" \
  --notes "$(printf '%s\n' "$LOOM_NOTES" | tr ';' '\n' | sed 's/^/- /')" --latest \
  "$DMG" dist/appcast.json
echo "已发布:https://github.com/$REPO/releases/tag/$TAG"
