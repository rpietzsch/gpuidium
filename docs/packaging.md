# Packaging (Linux)

One application id everywhere: **`io.github.rpietzsch.GPUIdium`** (desktop file, icon, AppStream metadata). The binary is `gpuidium`. Files live in `packaging/`.

| Format | Where | Build | Notes |
|---|---|---|---|
| Flatpak | `packaging/flatpak/` | CI (`flatpak-builder`), or locally, see the manifest header | GNOME 49 runtime supplies WebKitGTK 4.1 and its security updates. Needs `--socket=x11` because the app runs on X11/XWayland. Offline Cargo sources are generated from `Cargo.lock` at build time. |
| `.deb` | `[package.metadata.deb]` in `crates/gpuidium/Cargo.toml` | `cargo build --release -p gpuidium && cargo deb -p gpuidium --no-build` | Library dependencies resolved at build time (`$auto`); needs WebKitGTK 4.1 >= 2.40 (Ubuntu 24.04, Debian 13). |
| `.rpm` | `[package.metadata.generate-rpm]` | `cargo build --release -p gpuidium && cargo generate-rpm -p crates/gpuidium` | Fedora and the RPM family. |
| Arch | `packaging/arch/PKGBUILD` | `cd packaging/arch && makepkg -f` | Builds from the tagged GitHub tarball; set `GPUIDIUM_SRC=/path/to/tar.gz` to build from your own. Run `updpkgsums` after tagging. |
| Tarball | `scripts/package-linux.sh` | `task package:linux` | Relocatable, plus `.sha256`; `task release:linux` adds a GPG signature. |

## Why the system WebKitGTK
GPUIdium renders pages with the system WebKitGTK instead of bundling an engine. The package stays small and the distribution (or the Flatpak runtime) ships the engine's security fixes. The cost: we depend on `webkit2gtk-4.1` >= 2.40 and on GTK 3.

## X11 / XWayland
The window is an X11 window (embedding the web view needs it). Packages therefore depend on an X server being available: XWayland on Wayland desktops. Native Wayland needs a different engine-hosting approach, tracked as RND-001/RND-005. For Flatpak this means `--socket=x11`.

## Release process
1. Update `CHANGELOG.md`, the version in `Cargo.toml`, `pkgver` in the `PKGBUILD` and the `<release>` entry in the metainfo file.
2. Merge to `main`, tag `vX.Y.Z`, push the tag.
3. `.github/workflows/release.yml` builds every package, writes `SHA256SUMS`, attaches build-provenance attestations (verify with `gh attestation verify <file> -R rpietzsch/gpuidium`) and creates the GitHub release with the changelog section as notes.

## Not done yet
Flathub submission, aarch64 builds, an apt/rpm repository (OBS or COPR), file dialogs through the portal (Flatpak export/import use fixed files in the app's config folder).
