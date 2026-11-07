#!/usr/bin/env bash
# UPD-010: build the Linux tarball, checksum it and (optionally) sign it.
# Signing needs GPUIDIUM_SIGN_KEY (a gpg key id/fingerprint); without it the release is
# checksum-only and says so.
set -euo pipefail
cd "$(dirname "$0")/.."
scripts/package-linux.sh
tarball=$(ls -t dist/gpuidium-*-linux-x86_64.tar.gz | head -n 1)
if [ -n "${GPUIDIUM_SIGN_KEY:-}" ]; then
  gpg --batch --yes --local-user "$GPUIDIUM_SIGN_KEY" --armor --detach-sign --output "$tarball.asc" "$tarball"
  echo "signed: $tarball.asc"
else
  echo "note: GPUIDIUM_SIGN_KEY not set, release is NOT signed" >&2
fi
scripts/verify-release.sh "$tarball"
