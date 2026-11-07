#!/usr/bin/env bash
# UPD-010: verify a downloaded GPUIdium release tarball.
#   scripts/verify-release.sh dist/gpuidium-<ver>-linux-x86_64.tar.gz
# Checks the SHA-256 file next to it and, if a .asc signature exists, the GPG signature
# (the signer's public key must already be in your keyring).
set -euo pipefail
tarball="${1:?usage: verify-release.sh <tarball>}"
dir=$(dirname "$tarball"); file=$(basename "$tarball")
( cd "$dir" && sha256sum --check "$file.sha256" )
if [ -f "$tarball.asc" ]; then
  gpg --verify "$tarball.asc" "$tarball"
else
  echo "note: no $file.asc signature found; only the checksum was verified" >&2
fi
echo "OK: $file"
