#!/usr/bin/env bash
# Regenerate the macOS icon assets from the SVG source (needs rsvg-convert and iconutil).
# Apple's grid puts the icon body at 824 of 1024 px, so the tile is drawn with that margin;
# the full-bleed SVG would look oversized next to other Dock icons.
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
src="$root/packaging/icons/io.github.rpietzsch.GPUIdium.svg"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# Wrap the SVG: its 480 px tile (at 16,16) is placed as an 824 px body centred in 1024.
inner=$(sed -e '1d' -e '$d' "$src")
cat > "$tmp/padded.svg" <<SVG
<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
<svg x="100" y="100" width="824" height="824" viewBox="16 16 480 480">
$inner
</svg>
</svg>
SVG

set_dir="$tmp/AppIcon.iconset"
mkdir "$set_dir"
for s in 16 32 128 256 512; do
  rsvg-convert -w $s -h $s "$tmp/padded.svg" -o "$set_dir/icon_${s}x${s}.png"
  rsvg-convert -w $((s * 2)) -h $((s * 2)) "$tmp/padded.svg" -o "$set_dir/icon_${s}x${s}@2x.png"
done
iconutil -c icns "$set_dir" -o "$root/packaging/macos/AppIcon.icns"
# Runtime Dock / Cmd+Tab icon for unbundled runs (`task run`).
cp "$set_dir/icon_256x256@2x.png" "$root/crates/gpuidium/assets/icon-macos-512.png"
echo "wrote packaging/macos/AppIcon.icns and crates/gpuidium/assets/icon-macos-512.png"
