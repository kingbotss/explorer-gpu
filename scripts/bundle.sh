#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
export MACOSX_DEPLOYMENT_TARGET=12.0
cargo build --release
app="$PWD/dist/Explorer.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" target/icon-modules
# Replace the executable atomically so an already-running app keeps its old inode.
bundle_binary="$app/Contents/MacOS/.Explorer.$$"
trap 'rm -f "$bundle_binary"' EXIT
cp target/release/explorer "$bundle_binary"
mv -f "$bundle_binary" "$app/Contents/MacOS/Explorer"
cp assets/Info.plist "$app/Contents/Info.plist"
clang -fobjc-arc -fmodules -fmodules-cache-path="$PWD/target/icon-modules" native/icon.m -framework AppKit -framework Foundation -o target/icon-generator
target/icon-generator "$PWD/target/Explorer.iconset"
cp target/Explorer.iconset.icns "$app/Contents/Resources/Explorer.icns"
codesign --force --deep --sign - "$app"
printf 'Built %s\n' "$app"
