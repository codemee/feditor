#!/usr/bin/env bash
# 把編譯好的 feditor 執行檔包成 macOS 的 FEditor.app。只能在 macOS 上跑（用到 sips、iconutil、codesign）。
#
# 用法：packaging/macos/make-app.sh <執行檔> <版本號> <輸出資料夾>
# 例如：packaging/macos/make-app.sh target/release/feditor 0.1.0 dist
set -euo pipefail

bin="$1"
version="$2"
out="$3"
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"

app="$out/FEditor.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp "$bin" "$app/Contents/MacOS/feditor"
chmod +x "$app/Contents/MacOS/feditor"

# Info.plist：把版本號填進範本
sed "s/@VERSION@/$version/g" "$here/Info.plist" > "$app/Contents/Info.plist"

# 圖示：由 256x256 的 icon.png 產生各種尺寸，再轉成 .icns
iconset="$(mktemp -d)/FEditor.iconset"
mkdir -p "$iconset"
src="$root/ui/assets/icon.png"
for spec in "16 icon_16x16" "32 icon_16x16@2x" "32 icon_32x32" "64 icon_32x32@2x" \
            "128 icon_128x128" "256 icon_128x128@2x" "256 icon_256x256"; do
  size="${spec%% *}"
  name="${spec#* }"
  sips -z "$size" "$size" "$src" --out "$iconset/$name.png" > /dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/FEditor.icns"

# 沒有 Apple Developer 憑證，只做 ad-hoc 簽章（不是公證）：至少讓套件結構完整、不是「已損毀」
codesign --force --deep --sign - "$app"

echo "已建立 $app"
