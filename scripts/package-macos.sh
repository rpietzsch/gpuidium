#!/usr/bin/env bash
# Build dist/GPUIdium.app (+ dist/gpuidium-<ver>-macos-<arch>.zip) for the host architecture.
# The bundle is ad-hoc signed: fine on the machine that built it; for distribution it needs a
# Developer ID signature and notarization (PKG-002).
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
arch=$(uname -m)
cargo build --release -p gpuidium --bin gpuidium

app=dist/GPUIdium.app
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/gpuidium "$app/Contents/MacOS/gpuidium"
cp packaging/macos/AppIcon.icns "$app/Contents/Resources/AppIcon.icns"
sed "s/@VERSION@/$version/g" packaging/macos/Info.plist > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null

codesign --force --sign - --timestamp=none "$app"
codesign --verify --strict "$app"

zip="dist/gpuidium-$version-macos-$arch.zip"
rm -f "$zip"
ditto -c -k --keepParent "$app" "$zip"
shasum -a 256 "$zip" > "$zip.sha256"
echo "built $app and $zip"
