#!/usr/bin/env bash
# Install GPUIdium from the working tree (distro-independent, no package manager).
#   scripts/install-linux.sh                 # per-user: ~/.local
#   PREFIX=/usr/local sudo -E scripts/install-linux.sh   # system-wide (build first as your user)
# Runtime deps (not installed): GTK 3, WebKitGTK 4.1, X11/XWayland.
set -euo pipefail
cd "$(dirname "$0")/.."
prefix="${PREFIX:-$HOME/.local}"
id=io.github.rpietzsch.GPUIdium
[ -x target/release/gpuidium ] && [ "$(id -u)" = 0 ] || cargo build --release -p gpuidium --bin gpuidium
install -Dm755 target/release/gpuidium "$prefix/bin/gpuidium"
install -Dm644 "packaging/$id.desktop" "$prefix/share/applications/$id.desktop"
install -Dm644 "packaging/$id.metainfo.xml" "$prefix/share/metainfo/$id.metainfo.xml"
install -Dm644 "packaging/icons/$id.svg" "$prefix/share/icons/hicolor/scalable/apps/$id.svg"
command -v update-desktop-database >/dev/null && update-desktop-database "$prefix/share/applications" 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" 2>/dev/null || true
echo "installed to $prefix (binary: $prefix/bin/gpuidium)"
