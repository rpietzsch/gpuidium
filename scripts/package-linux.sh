#!/usr/bin/env bash
# PKG-003 (Linux): build a relocatable tarball: bin/gpuidium + desktop entry + docs.
# Runtime deps (not bundled): GTK 3, WebKitGTK 4.1, X11/XWayland. See README.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -p gpuidium --bin gpuidium
ver=$(cargo metadata --no-deps --format-version 1 | python3 -c 'import sys,json;print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="gpuidium"))')
name="gpuidium-$ver-linux-x86_64"
# Fresh staging dir per run (no deletes); dist/ is git-ignored, clear old stages by hand.
mkdir -p dist
stage=$(mktemp -d dist/stage.XXXXXX)
out="$stage/$name"
mkdir -p "$out/bin" "$out/share/applications" "$out/share/metainfo" "$out/share/icons/hicolor/scalable/apps" "$out/share/icons/hicolor/256x256/apps"
install -m755 target/release/gpuidium "$out/bin/gpuidium"
install -m644 packaging/io.github.rpietzsch.GPUIdium.desktop "$out/share/applications/"
install -Dm644 packaging/io.github.rpietzsch.GPUIdium.metainfo.xml "$out/share/metainfo/io.github.rpietzsch.GPUIdium.metainfo.xml"
install -m644 packaging/icons/io.github.rpietzsch.GPUIdium.svg "$out/share/icons/hicolor/scalable/apps/io.github.rpietzsch.GPUIdium.svg"
rsvg-convert -w 256 packaging/icons/io.github.rpietzsch.GPUIdium.svg -o "$out/share/icons/hicolor/256x256/apps/io.github.rpietzsch.GPUIdium.png" 2>/dev/null || true
cp README.md LICENSE "$out/"; cp -r docs "$out/docs"
tar -C "$stage" -czf "dist/$name.tar.gz" "$name"
( cd dist && sha256sum "$name.tar.gz" > "$name.tar.gz.sha256" )
echo "built dist/$name.tar.gz"
