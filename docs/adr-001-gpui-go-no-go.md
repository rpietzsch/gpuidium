# ADR-001: GPUI-only GO / NO-GO

**Status:** PROVISIONAL - Linux X11/XWayland evidence only. Windows 11 and macOS are **unverified**; G1 is **not** passed until SPIKE-002 and SPIKE-003 report.
**Date:** 2026-11-07

## Pinned stack (SPIKE-001)

| Component | Version | License |
|---|---|---|
| Rust toolchain | 1.99.1 (`rust-toolchain.toml`; `gpui-pre` needs `std::hint::cold_path`, absent in 1.93) | - |
| `gpui-kit` | =0.9.0 (re-exports the **`gpui-pre` 0.3.11** GPUI fork, not Zed's `gpui` on crates.io) | Apache-2.0 |
| `gpui-webview` | =0.9.0 | Apache-2.0 |
| `lb-wry` (fork of Wry) | =0.53.3 | Apache-2.0 / MIT |

Clean debug build of the full graph on Linux: 4m27s cold. `Cargo.lock` is committed. The `gpui-fast` feature is **not** enabled (it is only needed for GPUI overlays above the WebView on macOS/Linux; see RND-002).

## Linux X11/XWayland findings (SPIKE-004, SPIKE-005 Linux half)

Harness: `crates/gpuidium/src/bin/spike_isolation.rs`. Two WebViews in one GPUI window, **identical origin** (`http://127.0.0.1:<port>`), each with its own `WebContext` data directory `profiles/<profile_id>`. Pages write cookie, localStorage and IndexedDB; a loopback server records what each page could read before writing.

Run on CachyOS, Wayland session via Xwayland (`DISPLAY=:0`), WebKitGTK 4.1 2.54.2, GTK 3.24.52:

- **Isolation: PASS.** Neither profile ever observed the other's cookie, localStorage or IndexedDB value.
- **Persistence within a run: PASS** (reload sees own values).
- **Restart persistence: PASS** (second process on the same data root sees own values on first load, same origin).
- Profile directories are fully separate on disk (`cookies/`, `localstorage/`, `databases/indexeddb/`, `storage/`).

Not yet covered on Linux: pure X11 session, service workers, DPI scaling, focus/resize/100-switch stress, popup handling.

### Findings that shape the design

1. **GTK must be initialised before `WebContext::new`.** `gpui-webview` initialises GTK only inside `WebView::build`, so creating per-profile contexts first panics ("GTK has not been initialized"). The host adapter must call `gdk::set_allowed_backends("x11")` + `gtk::init()` itself. Needs a direct `gtk = "0.18"` dependency on Linux.
2. **Residual cross-profile leakage on Linux: HTTP cache and HSTS.** Wry's `WebContext` only relocates the base data dir and cookies. `~/.cache/<exe>/WebKitCache`, `CacheStorage` and `~/.local/share/<exe>/hsts-storage.sqlite` stayed in shared default locations, and libsoup warned "database is locked" with two contexts open. Storage that apps use for identity (cookies, localStorage, IndexedDB) is isolated, but the shared cache/HSTS violate the "no shared store" rule in spirit. **Follow-up PROFILE-004** (below) must relocate or disable them per profile (WebKit `WebsiteDataManager` cache/HSTS directories) or document the limit.
3. **Linux requires an X11 window.** `WebView::build` errors on a Wayland window; the app must start via `gpui_kit::platform::linux(WindowingModes::X11)`.
4. **localStorage/IndexedDB are origin(port)-scoped; cookies are host-scoped.** Tests and, later, any local test pages need a stable port.

## Platform API map (from source, to be verified by spikes)

| OS | Mechanism for per-profile persistent store | Constraint |
|---|---|---|
| Linux | `WebContext::new(Some(dir))` per profile (WebKitGTK `WebsiteDataManager`) | **Verified.** See findings 1-2. |
| macOS | `WebViewBuilderExtDarwin::with_data_store_identifier([u8;16])` (WKWebsiteDataStore); `ProfileId` is a UUID so its 16 bytes map directly. `data_directory` is **not** available on WKWebView | **Unverified.** Requires **macOS 14+**; baseline is therefore macOS 14 unless a fallback is found. |
| Windows | `WebContext::new(Some(dir))` = separate WebView2 user data folder | **Unverified.** WebView2 environments with different user-data folders must be created per profile; test concurrently in one process. |

## Decision

- **Linux: GO** for continuing with GPUI + gpui-webview, pending PROFILE-004.
- **macOS, Windows: HOLD** until SPIKE-002/003/005 produce evidence on real hardware. No Tauri fallback; on failure pursue a GPUI-compatible adapter or upstream fixes and hold that platform's release.
- Milestone 1 work proceeds against the `WebviewHost` abstraction so platform gaps stay contained.
