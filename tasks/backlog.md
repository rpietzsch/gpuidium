# GPUIdium — Implementation Backlog

**Version:** 0.2 · 2026-11-07 (owner decisions incorporated)
**Locked decisions:** GPUI-only shell; isolated persistent browser profiles per app instance are P0; Linux X11/XWayland first, native Wayland later.  
**Purpose:** Ordered, dependency-aware tickets that a Rust desktop team can pick up immediately.  
**Suggested tracking fields:** ID, priority, owner, status, estimate, dependencies, PR link, evidence (screenshot/video/test).  
**Estimates:** S (≤1 day), M (2–3 days), L (4–7 days), XL (research/unknown); exploratory estimates are not commitments.

## Legend

✅ done (on Linux unless the note says otherwise) · 🟡 partly done, or built but not yet verified in the running app · ⛔ blocked (needs macOS/Windows hardware) · ⬜ not started.
Windows and macOS are not verified anywhere, so no ticket is "done" for those platforms.

## Status summary (2026-11-07)

**M0 (G1 gate): Linux half complete, gate NOT passed.** Remaining items all need hardware or a decision: SPIKE-002 (macOS) and SPIKE-003 (Windows) need real machines; SPIKE-004 still needs pure X11, DPI, focus/resize stress; SPIKE-005 needs macOS/Windows; SPIKE-006/007 stay open until those report (ADR-001 is PROVISIONAL).
**M1: implemented and verified on Linux, with these gaps:**
- Linux gaps closed on 2026-11-07: CORE-006, PROFILE-004, PROFILE-002 (see Progress).
- UI-003: screen-reader labels and focus-return unverified.
- PROFILE-003: no orphan purge UI (also PROFILE-011 in M3).
- Win/macOS verification of every "Done on Linux" row.

## Progress (updated as work lands)

| Item | Status | Evidence |
|---|---|---|
| SPIKE-001 | **Done** | Pins in `Cargo.toml`/`rust-toolchain.toml`, `Cargo.lock`; ADR-001 version table |
| BUILD-001 | **Done** (Linux verified; macOS: `task build`/`check` clean, 2026-11-08; Windows untested) | `Taskfile.yml`, README |
| SPIKE-004 | **Partial** - Xwayland session verified; pure X11, DPI, focus/resize/100-switch not yet | ADR-001 |
| SPIKE-005 | **Partial** - Linux PASS; **macOS PASS** (2026-11-08, macOS 26.7.1 arm64; one `WKWebsiteDataStore` per profile UUID, min. macOS 14; before the fix all profiles shared one store); macOS x64 and Windows missing | `spike_isolation`, `spike_oidc`, ADR-001 |
| SPIKE-002, SPIKE-003 | **Blocked** - need macOS / Windows machines | - |
| SPIKE-006, SPIKE-007 | **Open** - ADR-001 is PROVISIONAL | ADR-001 |
| CORE-001, CORE-002, CORE-003 | **Done** (unit tests; fuzz-style URL sweep) | `crates/gpuidium-core` |
| CORE-004 | **Done** - `WebviewHost` trait + Wry adapter; native types never reach the domain model | `crates/gpuidium/src/host.rs` |
| CORE-005 | **Done on Linux** - lazy create, hide-all-then-show-one, 102 keyboard switches without crash; Win/macOS unverified | manual smoke, 2026-11-07 |
| CORE-006 | **Partial** - back/forward/reload/home/origin/loading shown; **no load-failure state yet** (Wry has no failure callback; needs a workaround) | `main.rs` |
| CORE-007 | **Done** - policy in core (tests), enforced via Wry navigation + new-window handlers, no IPC handler registered | `policy.rs`, `host.rs` |
| PROFILE-001 | **Done on Linux** - one `WebContext` + data dir per `ProfileId`, never shared | `host.rs`, `profiles.rs` |
| PROFILE-002 | **Partial** - automated same-origin test passes on Linux and macOS arm64 (`task test:isolation`); Linux in CI, macOS not; Windows missing | ADR-001 |
| PROFILE-003 | **Done on Linux** - default keep-data, explicit delete scoped to one profile (symlink-safe, tested); orphan listing exists, no purge UI yet | `profiles.rs`, `main.rs` |
| UI-001 | **Done on Linux** - sidebar, selection, empty state, no overlays above the native pane | screenshots |
| UI-002 | **Done on Linux** - add (URL diagnostics), inline Edit (name + start URL, re-validated by core, navigates on change, profile unchanged) and remove (keep/delete confirmation). Manual Windows z-order check pending | `main.rs` (`begin_edit`/`save_edit`) |
| UI-003 | **Partial** - Ctrl/Cmd+L/R/1-9 and Alt+Left/Right work; screen-reader labels and focus-return not verified; Ctrl+W deliberately unbound | `main.rs` |
| UI-004 | **Done** - monogram avatar, move up/down | `main.rs` |
| CORE-008 | **Done on Linux** - list/order/active/profile mapping restored after restart | manual test |
| CORE-009 | **Done** - single-instance lock, opt-in `GPUIDIUM_LOG` with URL redaction, config-corruption notice | `instance.rs`, `diagnostics.rs` |
| PROFILE-004 (P0) | **Done on Linux** - `lb-wry` vendored+patched: cache, DOM cache, HSTS, ITP under the profile dir; `task test:isolation` asserts nothing lands in shared dirs. macOS/Windows equivalents unverified | `third_party/lb-wry/PATCHES.md` |
| CORE-006 | **Done on Linux** - WebKit `load-failed` -> error line in toolbar, retry via reload (cancelled/policy-blocked loads ignored). Not yet exercised by hand | `host.rs`, `main.rs` |
| PROFILE-002 | **Done on Linux** - `task test:isolation` (CI under xvfb) | `Taskfile.yml`, `ci.yml` |
| OIDC login (new) | **Done on Linux** - mock IdP/RP redirect flow works; persistent-cookie login survives restart; second profile isolated; **session cookies without Max-Age are lost on restart** (documented limit); popup logins unsupported | `spike_oidc`, `docs/known-limits.md` |
| PROFILE-011 | **Partial** - retained-data list with size + per-profile delete in sidebar (core `disk_usage`, tested); no bulk purge | `main.rs`, `profiles.rs` |
| PROFILE-010 | **Done (core+UI)** - Export/Import of names+URLs only, fresh profiles on import; fixed file paths (`apps-export.json` / `apps-import.json` in config dir), no file dialog yet | `exchange.rs` |
| QA-001 | **Done** - `.github/workflows/ci.yml` (task check + xvfb tests); not yet run on GitHub | `ci.yml` |
| QA-004 | **Done (self-review, Linux)** | `docs/security-review.md` |
| DOC-001 | **Done for Linux** | `README.md`, `docs/known-limits.md` |
| PKG-003 | **Partial** - Flatpak manifest, `.deb`, `.rpm`, Arch `PKGBUILD`, tarball + SBOM, release workflow (see `docs/packaging.md`); CI-built results recorded below once green; no Flathub submission, no aarch64 | `packaging/`, `.github/workflows/release.yml` |
| PERF-001 | **Partial** - harness `task bench` works for 0 and 1 apps (2 runs, Linux/Xwayland, WebKitGTK 2.54): 0 apps ~101 MB PSS / ~1.2% CPU idle, 1 app (3 processes) ~266 MB / ~3.2%. **3/5-app workloads invalid**: xdotool key injection does not switch apps in GPUI (same 3 processes), so views were never created; needs a different driver (e.g. an in-app test hook). Only 2 runs, not the required 5 | `scripts/bench.py` |
| UI redesign (2026-11-07, from `Bildschirmfoto_20261107_200804.png`) | **Done, partly verified** - narrow icon sidebar, no address/button bar; IRI field only in the Open panel (Ctrl/Cmd+L or +; K dropped 2026-11-08), disappears after Enter; panels hide the native view (no overlays); error page replaces blank pane; transient 5 s status line. Verified by screenshots (real app, 2026-11-07): empty-state Open panel, add by Enter, bar-free page, shortcut sheet (Ctrl+Shift+/), Esc, Ctrl+L then typing + Enter adds a second app, Ctrl+1/2 switching, Ctrl+, Manage panel (apps, settings, saved data, export/import), restart restores list and active app, load-error page (red-ringed icon, Retry / Open in browser / Edit address) and switching away from it. Needed fixes: native GTK key interception (WebKit swallowed shortcuts), `focus_parent` + `activate_window` when a panel opens, `load-failed` returns true. **Not yet verified**: drag reorder, edit/remove in Manage, unload-inactive, notifications, Retry button. Minor: the error page's origin line is blank | `main.rs`, `host.rs` |
| Shortcut sheet | **Done on Linux (verified by screenshot)** - Ctrl/Cmd+? (also Ctrl+Shift+/ and F1) opens the reference; `?` icon too | `main.rs` (`shortcut_rows`) |
| UX-010 | **Done on Linux (verified, real mouse drag)** - drag icon onto another to reorder (uses the persisted move commands); move up/down kept in Manage | `main.rs` |
| UX-012 | **Done on Linux (verified)** - Open panel doubles as finder: typing a non-address filters apps, Enter switches | `main.rs` |
| LIFE-010 | **Done on Linux (verified; pure rule unit-tested in core)** - opt-in unload of apps idle 10/30/60 min (Manage > Settings), `Settings` in config (additive, default off, tested) | `main.rs`, `storage.rs` |
| UX-013 | **Done on Linux (verified with a fake notify-send)** - global opt-in; all permission requests denied unless on; notifications via `notify-send`; no page-to-native channel | `host.rs`, `external.rs` |
| App icon | **Done on Linux** - `packaging/icons/gpuidium.svg` (jagged R on violet tile); passed to GPUI as `WindowOptions.icon`, which sets `_NET_WM_ICON` when the X11 window is created, before it is mapped (earlier approach set it after the map and could show the default "X" for a moment); title-bar icon confirmed by screenshot after startup; first-frame timing could not be captured automatically, please eyeball. PNGs embedded from `crates/gpuidium/assets/`, installed by `task package:linux`. Sidebar shows no second logo. Win/macOS window icon not done |
| UX-011 favicons | **Done on Linux (verified by screenshot)** - WebKit's own favicon (per-profile favicon DB via `lb-wry` patch) -> PNG cached at `<data>/favicons/<app_id>.png` (<=256 KB, deleted with the app), shown at 16 px in a 28 px tile; monogram only as fallback; sidebar rail is 40 px wide. GPUIdium itself fetches nothing | `packaging/icons/` |
| PERF switching regression | **Fixed and guarded** - page shrank to a 200x200 square after switching (de-dupe + GTK window reset); fixed by mapping/unmapping the X window instead of hiding the GTK wrapper; `task test:switching` regression test | `scripts/test_switching.py`, `docs/performance.md` |
| PERF (investigation 2026-11-07) | **Done for Linux** - event-driven host events, geometry de-dupe, page focus fix, release `task run`; WebKitGTK's ~32 fps ceiling on this machine is engine-level (same in Epiphany); full write-up in `docs/performance.md` | `docs/performance.md` |
| M3 | **Complete on Linux** - see `docs/verification-linux.md` for the evidence | - |
| macOS first build (2026-11-08) | **Done** - `task build` failed (spike bins used GTK/`platform::linux` unconditionally) and warned (vendored `lb-wry` deprecations/unneeded `unsafe`, Linux-only `HostEvent` variants); both fixed, `task check` clean on macOS. Found and fixed a **P0 isolation leak on macOS** (all profiles shared WebKit's default store), see SPIKE-005 | `host.rs`, `third_party/lb-wry/PATCHES.md` #5-6 |
| macOS clipboard in pages (2026-11-08) | **Done (verified, macOS 26.7.1 arm64)** - Cmd+V did nothing in web page fields (no Edit menu); now app menu (Quit, Cmd+Q) + Edit menu (Cut/Copy/Paste/Select All). Scripted check in a sandboxed HOME: paste and cut in a page field, paste into the Open panel, Cmd+Q; paste fails again with the menu removed. Undo/Redo menu items deliberately absent (Cmd+Z in pages not checked) | `main.rs` (`install_menus`) |
| macOS user agent (2026-11-08) | **Done, partly verified** - WhatsApp Web demanded "Safari 15" because WKWebView's UA has no `Version/` token; now `...Version/27.0 Safari/605.1.15` from the installed Safari (verified by a local UA probe). Not yet confirmed that WhatsApp Web accepts it (screenshot not possible from the agent) | `host.rs` (`safari_user_agent`) |
| macOS favicons (2026-11-08) | **Done (verified)** - in-page script loads and rasterises the icon, host polls for it (`js/favicon_*.js`, `WryHost::refresh_favicon`); decoder unit-tested. Verified in a sandboxed HOME: local SVG icon stored as 64x64 PNG with correct colour, real WhatsApp Web icon stored. Not yet checked: icon updates on unread-count changes, sites with cross-origin icons without CORS (monogram fallback) | `host.rs`, `main.rs` |
| Web-app shortcuts / Mattermost (2026-11-08) | **Done on macOS (verified), Linux change untested** - Ctrl/Cmd+K no longer opens the Open panel (left to pages); Linux interception ignored Shift and swallowed Ctrl+Shift+K/L/R/... (now only exact combos; needs a Linux run); macOS Cmd+. forwarded to the page. Key probe page in a sandboxed HOME: Cmd+K, Cmd+Shift+K/L/A/M, Cmd+., Cmd+Shift+., Cmd+/, Alt+Up/Down, Alt+Shift+Down, Cmd+Up, Shift+Up, Esc, Shift+Esc, Cmd+B, Cmd+Alt+K all reach the page; Cmd+R/1/,/L still handled by GPUIdium and not seen by the page. Not tested against a real Mattermost server | `host.rs`, `main.rs` |
| Page zoom (2026-11-08) | **Done on macOS (verified), Linux untested** - Ctrl/Cmd + +/=/numpad+ , -/numpad-, 0; 10 % steps, 30-300 %, per app, persisted (`config.json` `zoom`), status line shows the level; core logic + config round-trip unit-tested (`gpuidium-core/src/zoom.rs`). Verified in a sandboxed HOME via page `innerWidth`: 100->110->120->110 %, kept across restart, Cmd+0 back to 100 %, German "+" key and numpad work. Linux: GTK interception added, not run | `zoom.rs`, `host.rs`, `main.rs` |
| macOS app bundle (2026-11-08) | **Partial** - see PKG-002. Verified: bundle id and foreground app status, `.icns` with all sizes, WebKit data moves to `~/Library/WebKit/<bundle id>/` and the documented one-time move keeps cookies + localStorage. Dock/Cmd+Tab icon not visually checked by the agent (no screen capture) | `scripts/package-macos.sh`, `packaging/macos/` |
| Unread badges (2026-11-08) | **Done (macOS verified), Linux untested** - title count / dot in the sidebar, total on the macOS Dock; notification dot until opened. Verified via `lsappinfo` StatusLabel: none -> `•` after a background notification -> `2` after title "(2) B" | `badge.rs`, `main.rs` |
| Per-app notifications (2026-11-08) | **Done (macOS partly verified), Linux untested** - per-app switch, on by default. macOS shim: permission `granted`, `requestPermission` resolves, `onshow` fires, host collects the queue (unbundled `osascript` and bundled Notification Center paths both run without errors). **Not verified**: the banner itself and click-to-switch (needs the user's one-time permission and eyes) | `notify_shim.js`, `mac.rs`, `host.rs` |
| Sound switch / mute all apps (2026-11-12) | **Done (macOS partly verified), Linux untested** - bell / bell-off button between + and ⚙: off mutes every page's own audio (engine-level, also views created later) and drops the system notification sound (`settings.sound`, default on; old configs load with it on, unit-tested). Verified in a sandboxed HOME: order +, bell, ⚙, ?, size matches, bell-off shown when off (window capture); macOS `_setPageMuted:` present and `_mediaMutedState` reads back 1 on a view created while muted (macOS 26.7.1). **Not verified**: that audio is actually silent (no loopback device), the click / live toggle, a silent banner (bundled app), Linux `set_is_muted` and `suppress-sound` hint | `main.rs`, `host.rs`, `icon.rs`, `mac.rs`, `external.rs`, `storage.rs` |
| macOS sidebar tooltips (2026-11-11) | **Built, not verified** - the GPUI tooltip of a sidebar icon was hidden under the native web view (only its left edge showed). macOS now sets the label as the native `toolTip` of GPUI's content view while an icon is hovered (`mac::set_tooltip`), which AppKit draws above the web view. Builds clean; hover not checked by the agent (no mouse automation). Linux likely has the same layering problem (unchecked) | `mac.rs`, `main.rs` |
| Spell checking (2026-11-08) | **Done (macOS partly verified), Linux untested** - Manage toggle; Linux multi-dictionary chips; macOS system spell checker (automatic per-word language among System Settings languages; German+English mixed text checked with NSSpellChecker). WebKit's switch is set before the first view (`WebContinuousSpellCheckingEnabled` = 1 verified). **Not verified**: red underlines and the live toggle (no screen capture) | `mac.rs`, `host.rs`, `spell.rs` |
| macOS navigation keys (2026-11-08) | **Done (verified, macOS 26.7.1 arm64)** - Up/Down did nothing in Mattermost's Cmd+K and unhandled keys inserted "no glyph" boxes. Key monitor in `mac.rs`. Probe page in a sandboxed HOME: Up/Down/Shift+Up/Option+Up reach `keydown` (preventDefault respected), Left/Home/End/Shift+Right/Forward Delete edit correctly, F1/Page Down delivered, no inserted characters; with the Open panel up arrows stay in GPUIdium. Follow-up: repeated presses fell back to the old behaviour because AppKit reuses `NSEvent` objects and the resend check compared objects; now (timestamp, key code). Fast bursts of 4x Down/End/Home/Up verified with and without page `preventDefault`. Not checked against a real Mattermost server by the agent | `mac.rs` |
| macOS JS dialogs (2026-11-10) | **Done (verified, macOS 26.7.1 arm64), Linux unchanged** - `alert()` showed nothing, `confirm()` returned false, `prompt()` null (WKWebView UI delegate lacked the dialog methods); now native `NSAlert` titled "<host> says" (`lb-wry` patch #7). Probe page in a sandboxed HOME, driven via System Events: alert blocks until OK, confirm OK/Cancel -> true/false, prompt shows the default text and returns the typed text / null on Cancel. Not checked: dialogs from a background (hidden) app, dialog spam suppression (none yet) | `wry_web_view_ui_delegate.rs` |
| macOS host gaps (new) | **Open** - no native shortcut interception while a page has focus (Cmd+L etc. only work when GPUI has focus), no load-failure page. Running the real app on macOS not yet tried | `host.rs` |
| Windows/macOS rows | Still blocked on hardware (SPIKE-002/003, PKG-001/002, QA-002 for those OSes) | - |

## Definition of done (all tickets)

- Code reviewed, `cargo fmt`, `cargo clippy`, tests relevant to changed modules, and no leaked credentials in logs.
- Changed behavior covered by a test or documented manual verification.
- Feature/platform limitations recorded in README or known-issues page.
- For platform-specific changes, supply evidence on affected OS and avoid regressing others.
- PR explains how it affects whole-process performance and security, if applicable.

## Milestone 0 — Feasibility gate: GO / NO-GO (P0, do first)

**Status:** ✅ 2 done · 🟡 3 partial/unverified · ⛔ 2 blocked · ⬜ 1 not started (of 8)

| Status | ID | Size | Ticket | Acceptance / proof | Dependencies | Notes |
|---|---|---|---|---|---|---|
| ✅ | SPIKE-001 | M | Create Rust workspace; pin compatible GPUI/GPUI Kit/gpui-webview/Wry versions | Clean builds, version lock, linked upstream example, licenses documented | — | pinned, builds |
| ⛔ | SPIKE-002 | L | macOS WKWebView child smoke | Page loads, focus, resize, URL/navigation, reload, switching 2 webviews, state across restart; test arm64 and x64 | 001 | needs a Mac (arm64+x64) |
| ⛔ | SPIKE-003 | L | Windows 11 WebView2 child smoke | Same as 002; explicit test of DirectComposition workaround and z-order, toolbar unobstructed | 001 | needs Windows 11 |
| 🟡 | SPIKE-004 | L | Linux X11 / XWayland WebKitGTK child smoke | Same as 002 on native X11 and Wayland session via XWayland; required GTK/WebKit packages documented; DPI tested | 001 | Xwayland verified; pure X11 + DPI + stress still open |
| 🟡 | SPIKE-005 | XL | **Prove persistent per-app-instance profile isolation** | On each OS, create two same-origin apps with distinct `profile_id`s; verify distinct cookies/localStorage, feasible IndexedDB/service-worker partitioning, both accounts persist after restart; document underlying WebView store APIs and minimum supported OS versions | 002,003,004 | Linux PASS; macOS arm64 PASS (min. macOS 14); macOS x64/Windows missing |
| ⬜ | SPIKE-006 | M | Document GPUI WebView composition constraints | Reproduce menu/dialog behavior, fullscreen, resizes, focus, popup opening on 3 OSes | 002,003,004 | composition notes only for Linux so far |
| 🟡 | SPIKE-007 | S | ADR-001: GPUI-only GO/NO-GO | Pass only with WebView composition **and isolation** on all required OSes; if blocked, record GPUI-compatible adapter/upstream remediation and affected-platform hold; **no Tauri fallback** | 005,006 | ADR-001 provisional: Linux GO, Win/macOS HOLD |
| ✅ | BUILD-001 | S | Root `Taskfile.yml` with build/lifecycle targets | `task` lists targets; `clean`, `build`, `build:release`, `run`, `test`, `fmt`, `lint`, `check` work on Win11, macOS and Linux (or are documented as platform-gated); `check` is the CI entry point; README documents `task` install and usage | SPIKE-001 | Linux verified |

**Gate G1 must pass** before implementing polished UI. It includes real persistent isolation, not just cookie persistence. Parallelize OS spikes. GPUI is a hard constraint: failed gates block release claims; work toward a GPUI-compatible host/patch rather than substituting Tauri.

## Milestone 1 — MVP application (P0)

**Status:** ✅ 16 done · 🟡 1 partial/unverified · ⛔ 0 blocked · ⬜ 0 not started (of 17)

| Status | ID | Size | Ticket | Acceptance / proof | Dependencies | Notes |
|---|---|---|---|---|---|---|
| ✅ | CORE-001 | M | `AppEntry`, `AppId`, `AppState`, commands/events | Typed state transitions; duplicate URIs allowed; unit tests | SPIKE-007 |  |
| ✅ | CORE-002 | M | IRI/URL parser + safe normalization | Unicode host/path, bare host; HTTPS default; HTTP loopback only; rejects dangerous schemes/userinfo; unit/fuzz cases | CORE-001 |  |
| ✅ | CORE-003 | M | Storage schema v1 + atomic config writes | Persist unique `profile_id` per AppId; robust roundtrip, versioning, backup, corrupt-config recovery; config stored in OS paths without browser secrets | CORE-001 |  |
| ✅ | PROFILE-001 | L | **Isolated persistent profile manager (P0)** | AppId maps 1:1 to opaque profile ID; create/open/suspend/close per-app data store through OS-specific backend; never reuse a store between entries | SPIKE-005,CORE-001,CORE-004 | Linux |
| ✅ | PROFILE-002 | M | **Same-origin isolation regression tests (P0)** | Two entries for identical URL keep distinct cookies/localStorage and independent logins before/after restart on Win11, macOS and Linux X11/XWayland; no shared-store fallback | PROFILE-001 | Linux, `task test:isolation` |
| ✅ | PROFILE-004 | M | **Per-profile HTTP cache / HSTS isolation (P0)** | Cache, CacheStorage and HSTS stores live under the profile dir (or are disabled) on Linux; equivalent verified on macOS/Windows; regression test that a cached/HSTS entry in A is not visible in B | SPIKE-005 | Linux, cache/HSTS in profile |
| ✅ | PROFILE-003 | M | **Profile retention and deletion safety (P0)** | Default remove retains browsing data with notice; explicit delete-data action cleans only selected closed profile; errors/retry/orphan cleanup documented; no cross-profile deletion | PROFILE-001,CORE-003 | Linux; delete re-checks at 2 s and 6 s because WebKit recreates stub files after the view closes |
| ✅ | CORE-004 | M | WebviewHost trait and gpui-webview adapter | No native Wry resource in domain model; owns lifetimes; callbacks correctly hop to GPUI UI thread | SPIKE-007,CORE-001 |  |
| ✅ | CORE-005 | L | Native WebView lifecycle for multiple entries | Create, show, hide, destroy; re-render does not recreate views; each view is attached to its own PROFILE-001 storage partition; 100 switches smoke | CORE-004,PROFILE-001 | Linux |
| ✅ | CORE-006 | M | Navigation controls and status | Back, forward, reload, current origin, load/error state; no silent empty pane | CORE-002,CORE-005 | Linux, error page verified |
| ✅ | CORE-007 | M | Navigation & popup safety policy | New-window interception, explicit external-link behavior, blocked schemes, no JS/native IPC | CORE-002,CORE-004 |  |
| ✅ | UI-001 | M | Sidebar, app selection, empty state | Add web app + select; no overlays above native pane; keyboard operable | CORE-001,CORE-005 | Linux |
| ✅ | UI-002 | M | Add/Edit/Remove dialogs | Name/URL editing; remove confirmation and explicit retain/delete profile-data choice; invalid URL diagnostics; works on Windows z-order | UI-001,CORE-002,PROFILE-003 | Linux, verified: edit name+URL (profile unchanged), remove keep/delete |
| 🟡 | UI-003 | M | Toolbar and keyboard shortcuts | Cmd/Ctrl+L/R/1–9; focus transfer; nav buttons; screen-reader labels | UI-001,CORE-006 | shortcuts verified; screen-reader labels unverified |
| ✅ | UI-004 | S | Visual icon fallback and ordering | Monogram avatar, sortable by stable persisted position (simple move up/down sufficient) | UI-001,CORE-003 | monogram fallback + favicons + reorder |
| ✅ | CORE-008 | M | Restore workspace and last-active app | Restart restores list/order/active entry and each unique profile mapping; independent same-origin sessions survive | CORE-003,CORE-005,PROFILE-002 | Linux |
| ✅ | CORE-009 | M | Single-instance startup + crash diagnostics | Opening twice safe; errors surfaced; local opt-in diagnostic logs redact secrets | CORE-003 |  |

**Milestone 1 acceptance:** a clean launch can add three URLs (including two identical origins), log into different accounts in the duplicate entries, switch/navigate/restart and retain both separate sessions on every target. Explicit data deletion must leave other profiles unaffected.

## Milestone 2 — Cross-platform alpha release (P0/P1)

**Status:** ✅ 2 done · 🟡 5 partial/unverified · ⛔ 2 blocked · ⬜ 2 not started (of 11)

| Status | ID | Priority | Size | Ticket | Acceptance / proof | Dependencies | Notes |
|---|---|---|---|---|---|---|---|
| 🟡 | QA-001 | P0 | L | Automated parser/state/persistence tests | CI runs `task check` (Rust unit tests, clippy, formatting) on supported hosts | Milestone 1 | CI workflow written, not yet run on GitHub |
| 🟡 | QA-002 | P0 | L | OS-specific manual and scripted smoke suite | Recorded pass/fail for Win11, macOS arm64+x64, Linux X11 and XWayland | Milestone 1 | Linux manual runs only; no recorded suite; no Win/macOS |
| 🟡 | QA-003 | P0 | M | OAuth/auth and popup compatibility matrix | Test representative web services in separate profiles, including same-site dual accounts; document blockers and external-browser fallback; no deceptive universal claim | CORE-007,PROFILE-002 | mock OIDC flow tested; real services not |
| ✅ | QA-004 | P0 | M | Security review | No executable schemes, uncontrolled native IPC, secrets in logs or cross-profile data bleed; external links and delete-data handled safely | CORE-007,PROFILE-003 | Linux self-review |
| ⛔ | PKG-001 | P0 | L | Windows installer and runtime handling | Install/uninstall, WebView2 preflight, fresh-user smoke, packaged executable signed if release policy requires | QA-002 | needs Windows |
| 🟡 | PKG-002 | P0 | L | macOS app bundle and distribution | Universal binary or separate arm64/x64 artifacts; launch, signing/notarization plan, clean install | QA-002 | `task package:macos`: `.app` + zip, ad-hoc signed, icon; launches with bundle id (verified 2026-11-08). Universal arm64+x86_64 build (`GPUIDIUM_UNIVERSAL=1`, `lipo` verified locally), local only; `task install:macos` copies to `/Applications`. A macOS CI job and release zip were tried and removed (owner decision 2026-11-08: not notarized, runner minutes). Missing: Developer ID + notarization (then CI/release), running on Intel hardware, clean-install test |
| 🟡 | PKG-003 | P0 | L | Linux deb/rpm/AppImage choice | At least one working distributable; WebKitGTK/GTK dependency and XWayland docs; fresh-user test | QA-002 | tarball only; no deb/rpm/AppImage |
| ✅ | DOC-001 | P0 | M | README, quickstart, limits, privacy page | Platform prerequisites, no recipe catalog, per-app isolation guarantees and limits, XWayland constraint, known incompatible services | QA-003 | Linux |
| 🟡 | PERF-001 | P1 | M | Repeatable baseline benchmark harness | Three deterministic local pages; 0/1/3/5 workloads; process-tree CPU/RAM; 5 runs with p50/p95 | Milestone 1 | 0/1 apps only; 3/5 invalid; 2 runs |
| ⬜ | PERF-002 | P1 | M | Same-machine comparison to minimal Tauri/Wry shell | Shared pages and measurement method; report raw data and methodology, no framework-level overclaim | PERF-001 |  |
| ⬜ | REL-001 | P0 | S | v0.1-alpha release checklist + artifacts | Builds and test evidence linked; known gaps visible; changelog, reproducible version tags | PKG-001,PKG-002,PKG-003,DOC-001 |  |

## Milestone 3 — Iteration (P1, only after alpha)

**Status:** ✅ 8 done · 🟡 0 partial/unverified · ⛔ 0 blocked · ⬜ 0 not started (of 8)

| Status | ID | Size | Ticket | Acceptance / proof | Notes |
|---|---|---|---|---|---|
| ✅ | UX-010 | M | Drag/drop app reorder | Sidebar reorder stable across restart and high DPI. | Linux, verified with a real mouse drag (order persisted) |
| ✅ | UX-011 | M | Site favicon fetching/cache | Safe bounded fetch, no private URL leaks, fallback to monogram, no tracking by GPUIdium. | Linux, verified |
| ✅ | UX-012 | M | App shortcuts and keyboard command palette | No OS shortcut collisions; accessible. | Linux, verified: typing a name filters, Enter switches |
| ✅ | PROFILE-010 | M | Export/import an app without leaking its profile | Import assigns a fresh profile ID unless explicitly designed for trusted profile migration; no session/cookie data in JSON exports. | Linux, verified via the buttons: export has no ids; import skips bad URLs, fresh profiles |
| ✅ | PROFILE-011 | M | Profiles management UX | List disk usage, selectively purge orphaned retained profiles, confirm deletion and show failures; no silent cross-profile cleanup. | Linux, verified: list with sizes, per-profile Delete, Delete all; apps' own profiles untouched |
| ✅ | LIFE-010 | M | Pause / unload inactive sites | User-selected option; avoids losing session state; benchmark before/after. | Linux, verified: idle app unloaded, active kept, lazy re-create; setting button cycles Off/10/30/60 |
| ✅ | UX-013 | M | Basic OS notifications | Permission-aware and opt-in; report platform behavior, no page-to-Rust privilege escalation. | Linux, verified: opt-in grants + notify-send called; off = denied; toggle button works |
| ✅ | UPD-010 | L | Updates and code-signing automation | Secure release path, integrity checks and rollback guidance. | Linux: signed+checksummed tarball, verify script (tamper test fails), rollback docs; no in-app updater by design |

## Milestone 4 — Research / explicitly out of MVP (P2)

**Status:** ✅ 0 done · 🟡 0 partial/unverified · ⛔ 0 blocked · ⬜ 5 not started (of 5)

| Status | ID | Ticket | Acceptance / proof | Notes |
|---|---|---|---|---|
| ⬜ | RND-001 | Native Linux Wayland hosting | Working prototype without forcing GPUI to X11; design reviewed for focus, compositor/security constraints. |  |
| ⬜ | RND-002 | GPUI/native-WebView overlay composition | Reliable toolbar popups and dialogs over child view across all three OSes, not just macOS. |  |
| ⬜ | RND-003 | Authentication deep-link return flow | Explicit protocol registration and audited link handling, with OS tests. |  |
| ⬜ | RND-004 | Site-specific scripting / unread count support | Threat model and permissions design first; no arbitrary injected scripts by default. |  |
| ⬜ | RND-005 | Webview engine alternative (CEF/Servo/etc.) | Justified by compatibility/performance evidence, shipping complexity and security update model. |  |

## Work breakdown and ordering

```text
SPIKE-001 ── BUILD-001 (Taskfile)
   ├── SPIKE-002 (macOS) ──┐
   ├── SPIKE-003 (Win11) ──┼── SPIKE-005/006 ── SPIKE-007 (GPUI-only GO/NO-GO)
   └── SPIKE-004 (Linux) ─┘                         │
                       ┌───────────────────────────┘
                       ├── CORE-001 ── CORE-002 ── CORE-003
                       ├── CORE-004 ── PROFILE-001 ── CORE-005 ── UI-001
                       └── CORE-007       PROFILE-002/003 ── UI-002/003
                                       └── QA + Packaging + Alpha
```

**Suggested first team split:** Desktop integration owner (Windows/macOS/Linux spikes), Rust domain/storage owner (parser/data/policy tests), UX owner (shell and navigation after spike), QA/release owner (platform matrix, builds, diagnostics). On a small team, assign one OS-spike owner per OS first rather than parallelizing UI too early.

## Locked decisions and remaining owner inputs

**Locked (do not reopen without explicit change request):**
1. **Linux MVP:** X11 and XWayland; native Wayland is RND-001, not a launch gate.
2. **MVP profiles:** mandatory isolated persistent browser data per **app instance**, including two entries for the same origin; PROFILE-001/002/003 and SPIKE-005 are P0 blockers.
3. **Framework:** GPUI mandatory on all supported platforms; there is **no Tauri/Electron shell fallback**. If a required platform cannot pass G1, hold that platform's release and pursue GPUI-compatible integration work.

**Decisions taken (focus Linux, defaults applied 2026-11-07):** PROFILE-004 via vendored `lb-wry` patch (to be offered upstream); load failure via WebKit `load-failed` signal. Remaining open: hardware for macOS/Windows, license, HTTP beyond localhost.

**Original open decisions (resolved above):**
1. **PROFILE-004 approach:** (a) vendor/`[patch]` `lb-wry` to add a constructor taking per-profile cache/HSTS dirs (recommended; also submit upstream), (b) wait on upstream only, (c) accept shared cache/HSTS on Linux and document it (violates the no-shared-store rule in spirit).
2. **Hardware for SPIKE-002/003:** who runs macOS (arm64+x64) and Windows 11 tests, or do we add CI runners for them?
3. **Load-failure detection (CORE-006):** accept a timeout-based "may have failed" state, or patch Wry for a real failure callback.

**Still to decide (not blockers for starting G0/G1):**
1. Should arbitrary HTTP beyond localhost be allowed? Default **no**.
2. Concrete macOS minimum version and supported Linux distributions (determine after profile API verification; initial smoke Ubuntu LTS + Fedora).
3. License (proposal Apache-2.0) and signing/distribution policy.

## Links and evidence sources

- https://gpui-kit.com/docs/webview/
- https://github.com/longbridge/gpui-kit/blob/main/crates/webview/README.md
- https://docs.rs/wry/latest/wry/
- https://docs.rs/wry/latest/wry/struct.WebContext.html
- https://docs.rs/wry/latest/wry/struct.WebViewBuilder.html
