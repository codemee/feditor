#!/usr/bin/env bash
# 把 FEditor.app 包成 .dmg：磁碟映像裡有 App、一個「應用程式」捷徑（拖過去就安裝）與兩份 README。
# 只能在 macOS 上跑（用到 hdiutil）。
#
# 用法：packaging/macos/make-dmg.sh <FEditor.app 的路徑> <輸出的 .dmg 路徑>
# 例如：packaging/macos/make-dmg.sh dist/FEditor.app dist/feditor-macos.dmg
set -euo pipefail

app="$1"
dmg="$2"
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"

stage="$(mktemp -d)"
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
cp "$root/README.md" "$root/README.en.md" "$stage/"

rm -f "$dmg"
hdiutil create -volname "FEditor" -srcfolder "$stage" -ov -format UDZO "$dmg" > /dev/null
rm -rf "$stage"

echo "已建立 $dmg"
