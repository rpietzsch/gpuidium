#!/usr/bin/env bash
# Build an Arch package from the committed tree (git HEAD) with the PKGBUILD and install it with pacman.
# Uncommitted changes are not included. Needs makepkg and sudo (pacman -U).
set -euo pipefail
cd "$(dirname "$0")/.."
ver=$(sed -n 's/^pkgver=//p' packaging/arch/PKGBUILD)
work=$(mktemp -d dist/arch.XXXXXX 2>/dev/null || { mkdir -p dist; mktemp -d dist/arch.XXXXXX; })
work=$(realpath "$work")
git archive --prefix="gpuidium-$ver/" -o "$work/gpuidium-$ver.tar.gz" HEAD
cp packaging/arch/PKGBUILD "$work/"
# cargo from rustup is not a pacman package, so makepkg's makedepends check would fail: skip it then.
flags=(-f --noconfirm)
if ! pacman -T cargo >/dev/null 2>&1 && command -v cargo >/dev/null; then flags+=(--nodeps); fi
# Persistent target dir (separate from `task build`: makepkg's RUSTFLAGS differ) so reruns reuse
# compiled dependencies. Workspace crates still rebuild since the source path changes per run.
export CARGO_TARGET_DIR="$PWD/target/arch"
( cd "$work" && GPUIDIUM_SRC="$work/gpuidium-$ver.tar.gz" makepkg "${flags[@]}" )
sudo pacman -U --noconfirm "$work"/gpuidium-"$ver"-*.pkg.tar.*
echo "installed gpuidium $ver via pacman (build dir: $work, delete by hand)"
