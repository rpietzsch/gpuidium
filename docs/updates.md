# Updates, integrity and rollback (Linux)

GPUIdium has **no built-in updater and never contacts a server**. Updating is a manual, verifiable step.

## Verify a release
```text
scripts/verify-release.sh dist/gpuidium-<version>-linux-x86_64.tar.gz
```
It checks the SHA-256 file and, when a `.asc` signature is published, the GPG signature (import the maintainer's public key first and compare its fingerprint with the one in the release notes). A release without a `.asc` is checksum-only and is labelled so by `scripts/release-linux.sh`.

## Update
1. Download the tarball, its `.sha256` and `.asc`, verify as above.
2. Extract next to the old version (do not overwrite it): `tar -xzf gpuidium-<new>-linux-x86_64.tar.gz`.
3. Start the new `bin/gpuidium`. Your apps, logins and favicons live in your user directories (`~/.config/GPUIdium`, `~/.local/share/GPUIdium`), not in the install folder, so they carry over.

## Roll back
Quit GPUIdium and start the old extracted folder again. `config.json` keeps a `config.json.bak` of the last good write, and a config written by a newer schema is refused rather than rewritten, so an older build never silently damages newer data. Browsing data is per profile and is never migrated by an update.

## Engine updates
The web engine is the system WebKitGTK; update it with your distribution. Keep it current: it is the main security surface.

## Not done
Auto-update, package-repository signing (deb/rpm), reproducible-build attestation.
