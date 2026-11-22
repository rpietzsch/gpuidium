# Packaging

One application id everywhere: **`io.github.rpietzsch.GPUIdium`** (desktop file, icon, AppStream metadata). The binary is `gpuidium`. Files live in `packaging/`.

| Format | Where | Build | Notes |
|---|---|---|---|
| Flatpak | `packaging/flatpak/` | CI (`flatpak-builder`), or locally, see the manifest header | GNOME 49 runtime supplies WebKitGTK 4.1 and its security updates. Needs `--socket=x11` because the app runs on X11/XWayland. Offline Cargo sources are generated from `Cargo.lock` at build time. |
| `.deb` | `[package.metadata.deb]` in `crates/gpuidium/Cargo.toml` | `cargo build --release -p gpuidium && cargo deb -p gpuidium --no-build` | Library dependencies resolved at build time (`$auto`); needs WebKitGTK 4.1 >= 2.40 (Ubuntu 24.04, Debian 13). |
| `.rpm` | `[package.metadata.generate-rpm]` | `cargo build --release -p gpuidium && cargo generate-rpm -p crates/gpuidium` | Fedora and the RPM family. |
| Arch | `packaging/arch/PKGBUILD` | `cd packaging/arch && makepkg -f` | Builds from the tagged GitHub tarball; set `GPUIDIUM_SRC=/path/to/tar.gz` to build from your own. Run `updpkgsums` after tagging. |
| Tarball | `scripts/package-linux.sh` | `task package:linux` | Relocatable, plus `.sha256`; `task release:linux` adds a GPG signature. |

`task install:linux` (`scripts/install-linux.sh`) builds the release binary and installs it with the desktop entry, AppStream metadata and icon under `~/.local` (set `PREFIX=/usr/local` and run with `sudo -E` after building for a system-wide install). `task install:arch` (`scripts/install-arch.sh`) packages git HEAD with the PKGBUILD and installs the result with `pacman -U`, so pacman tracks and can remove it; uncommitted changes are not included.

`task install:win` (`scripts/install-windows.ps1`, Windows PowerShell 5.1+) is the Windows counterpart: it builds the release binary from the working tree in the normal `target\` dir (so it reuses the cache of `task build:release` / `task run`), copies it to `%LOCALAPPDATA%\Programs\GPUIdium` with an icon made from the PNG assets, adds a Start menu shortcut and registers a per-user uninstall entry (HKCU) so Settings > Apps lists and removes it (`uninstall.ps1` in the install dir, from `scripts/uninstall-windows.ps1`; app data and logins are kept). No admin rights. Not an installer package (PKG-001): no WebView2 preflight, no signing.

Windows builds embed the app icon as icon resource 1 (`crates/gpuidium/build.rs` writes the `.res` from the PNG assets, no resource compiler needed; MSVC toolchain only), which GPUI uses for the window and taskbar and Explorer shows for the `.exe`. Release builds use the GUI subsystem (no console window); with `GPUIDIUM_LOG=1` started from a terminal the log still goes there. At startup the app registers its toast sender under `HKCU\Software\Classes\AppUserModelId\io.github.rpietzsch.GPUIdium` (display name, and the icon when installed by `install:win`); the uninstaller removes it.

## macOS app bundle (experimental)

`task package:macos` (`scripts/package-macos.sh`) builds `dist/GPUIdium.app` and `dist/gpuidium-<ver>-macos-<arch>.zip` (+ `.sha256`) for the host architecture: release binary, `Info.plist` from `packaging/macos/Info.plist` (bundle id `io.github.rpietzsch.GPUIdium`, minimum macOS 14), `AppIcon.icns`, ad-hoc signature. `task install:macos` builds it and copies it to `/Applications` (quit a running copy first). `GPUIDIUM_UNIVERSAL=1 scripts/package-macos.sh` builds a universal (arm64 + x86_64) binary instead (`...-macos-universal.zip`). macOS is not built in CI and not published: the app is ad-hoc signed and not notarized (a copied or downloaded app needs right-click → Open once, or `xattr -dr com.apple.quarantine /Applications/GPUIdium.app`), and macOS runners are expensive. The Dock, Cmd+Tab and Finder show the icon from the bundle; unbundled runs (`task run`) set the same icon at runtime.

The icon assets (`packaging/macos/AppIcon.icns`, `crates/gpuidium/assets/icon-macos-512.png`) are generated from the SVG by `scripts/make-macos-icon.sh` (needs `rsvg-convert`), padded to Apple's icon grid.

**Logins when switching from `task run` to the app:** WebKit keeps each profile's data under a folder named after the bundle id, so the app does not see the data of unbundled runs (`~/Library/WebKit/gpuidium/`). Config and app list are shared (`~/Library/Application Support/GPUIdium`). To keep the logins, quit GPUIdium and move the stores once (tested: cookies and localStorage carry over):

```sh
mkdir -p ~/Library/WebKit/io.github.rpietzsch.GPUIdium
mv ~/Library/WebKit/gpuidium/WebsiteDataStore ~/Library/WebKit/io.github.rpietzsch.GPUIdium/
```

Not done: universal (arm64 + x86_64) binary, Developer ID signing and notarization (an ad-hoc signed app downloaded from elsewhere is blocked by Gatekeeper), DMG, CI build.

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
