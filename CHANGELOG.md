# Changelog

All notable changes to GPUIdium are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Fixed
- macOS: the tooltip of a sidebar icon was hidden behind the web page. It is now a native
  tooltip shown above the page.

## [0.4.1] - 2026-11-10

macOS fix for page dialogs. No changes for Linux.

### Fixed
- macOS: page dialogs from `alert()`, `confirm()` and `prompt()` did not appear (`confirm()`
  always returned false, `prompt()` null). They now show as native dialogs titled with the
  page's host ("example.com says").

## [0.4.0] - 2026-11-08

Unread badges, per-app notifications and spell checking. Packages are still Linux only.

### Added
- Unread badges: the sidebar shows the unread count from the page title (for example
  "(3) WhatsApp") or a dot for news without a count; on macOS the Dock icon shows the total.
- Page notifications per app, on by default (Manage: "Notifications" next to each app). macOS:
  in Notification Center for the app bundle; a click opens the app and the page's target (for
  example the chat). Linux: the desktop notification service as before.
- Spell checking with a switch in Manage. Linux: choose several dictionaries at once (for
  example en_US and de_DE). macOS: the system spell checker detects the language per word
  among the languages set in System Settings.
- macOS: `task install:macos` builds the app and copies it to `/Applications`;
  `GPUIDIUM_UNIVERSAL=1 scripts/package-macos.sh` builds a universal (Apple Silicon + Intel)
  app. macOS is built locally only: no CI job and no download, as the app is not notarized.

### Fixed
- macOS: arrow keys, Home/End, Page Up/Down and F-keys reach web pages as key events. Before,
  Up/Down could not browse pickers such as Mattermost's Cmd+K channel switcher, and keys the
  page did not handle typed "no glyph" boxes or control characters into the field.

### Changed
- The global "Page notifications" opt-in is replaced by the per-app switch; all apps start
  with notifications on.

## [0.3.1] - 2026-11-08

macOS app bundle. No changes for Linux.

### Added
- macOS: `task package:macos` builds `GPUIdium.app` (and a zip) with the app icon in the Dock,
  Cmd+Tab and Finder; unbundled runs also show the icon. Ad-hoc signed, host architecture
  only. Moving from `task run` to the app needs a one-time move of the WebKit data to keep
  logins, see `docs/packaging.md`.

## [0.3.0] - 2026-11-08

Adds page zoom and better behaviour for web apps with their own shortcuts. First version that
builds and runs on macOS (14 or later, from source; experimental, no package yet). Packages are
still Linux only.

### Added
- Page zoom with Ctrl/Cmd + `+` / `-` / `0` (also `=` and the numpad keys): 10 % steps between
  30 % and 300 %, kept per app across restarts.
- macOS: per-profile isolation with one WebKit data store per app (macOS 14 or later; on older
  systems pages are refused rather than sharing one store), Edit menu so copy and paste work
  in pages, Cmd+Q, page favicons, Safari user agent (sites such as WhatsApp Web no longer ask
  for a newer Safari), and Cmd+. reaches the page.
- `task test:isolation` and `task test:oidc` also run on macOS.

### Changed
- Ctrl/Cmd+K is no longer a GPUIdium shortcut, so pages can use it (for example Mattermost's
  quick switcher). The Open panel stays on Ctrl/Cmd+L and the + button.
- Linux: Ctrl+Shift combinations (for example Ctrl+Shift+K or Ctrl+Shift+L) now go to the page
  instead of being taken by GPUIdium.

### Fixed
- Building on macOS failed, and the vendored `lb-wry` produced compiler warnings there.

### Known limitations
- macOS: shortcuts other than Cmd+. are not intercepted while a page has focus, and there is
  no load-failure page and no page notifications yet. Undo/Redo are not in the Edit menu.

## [0.2.0] - 2026-11-07

First usable release after the 0.0.1 pre-alpha, for Linux (X11 and XWayland). Windows and macOS are not supported yet.

### Added
- Narrow icon sidebar with the page's own favicon (monogram only as fallback); no address or
  button bar. The address field appears only in the Open panel (Ctrl+L / Ctrl+K), which also
  finds existing apps by name.
- One isolated, persistent browser profile per app: cookies, localStorage, IndexedDB, HTTP
  cache, DOM cache, HSTS and favicons live in the profile folder, so two entries for the same
  site can be signed in as different users. Logins with persistent cookies survive restarts.
- Manage panel: edit name and address, reorder (also by drag and drop), remove with a choice
  to keep or delete the browsing data, list and delete saved data of removed apps.
- Keyboard shortcuts that work while a page has focus, and a shortcut sheet (Ctrl+? or F1).
- Load-failure page with Retry, Open in browser and Edit address.
- Export and import of the app list (names and addresses only, never sessions).
- Opt-in settings, off by default: unload apps that have been idle for a while, and page
  notifications (through the desktop notification service or the Flatpak portal).
- Single-instance lock, atomic config writes with backup and recovery, opt-in redacted logs
  (`GPUIDIUM_LOG`).
- Application icon (title bar and task bar).
- Packages: Flatpak, `.deb`, `.rpm`, Arch `PKGBUILD`, and a tarball with SHA-256 checksums and
  a CycloneDX SBOM; release artifacts carry build-provenance attestations. See
  `docs/packaging.md` and `docs/updates.md`.
- Test tooling: same-origin isolation test, mock OIDC login flow test, memory/CPU benchmark
  harness, signed-release and verification scripts.

### Performance
- Native events (shortcuts, page loads, favicons) are handled immediately instead of on a 100 ms timer: key press to visible page about 107 ms to 58 ms (median).
- No redundant move/resize/show calls to the web view on every shell redraw (about 20 per second while hovering the sidebar, now none).
- Pages are focused after switching apps, so the text caret is visible.
- Switching apps no longer relayouts the page: the web view is mapped and unmapped without hiding and showing its GTK window (0 relayouts per switch; key press to first frame about 50-75 ms on 20,000-node pages).
- `task run` builds in release mode (about 1.7x faster to first paint than debug). See `docs/performance.md`.

### Security
- Only `https` (and `http` for loopback) addresses are accepted; `javascript:`, `file:`, `data:`
  and credentials in URLs are rejected. Popups never open inside the app; there is no
  page-to-native bridge, and every web permission request is denied unless notifications are
  opted into. See `docs/security-review.md`.

### Known limitations
- Linux X11/XWayland only; native Wayland needs a different way to host the web engine.
- Session-only cookies (no `Max-Age`/`Expires`) are not kept across restarts; login popups
  open in the system browser. See `docs/known-limits.md`.
- In Flatpak, export and import use fixed files inside the app's config folder (no file
  dialog yet).

[Unreleased]: https://github.com/rpietzsch/gpuidium/compare/v0.4.1...HEAD
[0.4.1]: https://github.com/rpietzsch/gpuidium/releases/tag/v0.4.1
[0.4.0]: https://github.com/rpietzsch/gpuidium/releases/tag/v0.4.0
[0.3.1]: https://github.com/rpietzsch/gpuidium/releases/tag/v0.3.1
[0.3.0]: https://github.com/rpietzsch/gpuidium/releases/tag/v0.3.0
[0.2.0]: https://github.com/rpietzsch/gpuidium/releases/tag/v0.2.0
