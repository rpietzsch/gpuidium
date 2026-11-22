# Windows verification log (2026-11-22)

Environment: Windows 11 Pro build 26300 (x64), WebView2 runtime 157.0.4341.60, Rust 1.99.1, release build unless noted. The app ran with `APPDATA`/`LOCALAPPDATA` pointed at throwaway folders, seeded with a small `config.json`; nothing touched the user's own GPUIdium data. Screenshots via `System.Drawing` screen capture; keys via `keybd_event` (only while GPUIdium had focus).

| Ticket | What was done | Observed |
|---|---|---|
| SPIKE-003 child WebView2 | Started the app with two apps (example.com, Wikipedia) | **Without** a fix the page area stayed blank: GPUI's DirectComposition swap chain covers the WebView2 child window although the page loaded. With `GPUI_DISABLE_DIRECT_COMPOSITION` (now set by the app at startup, `win::init_process`) the page renders inside the shell |
| SPIKE-003 z-order | Ctrl+L, Esc; hovered a sidebar icon | The Open panel is drawn by GPUI while the view is hidden (spec section 4), Esc shows the page again. The GPUI tooltip would be covered by the page, so Windows uses a native tooltip window (`win::set_tooltip`): it shows above the page |
| SPIKE-005 / PROFILE-002 isolation | `task test:isolation` | PASS: two same-origin profiles never see each other's cookie, localStorage or IndexedDB; all three persist across a restart (`--expect-persisted`). Each profile has its own `EBWebView` folder; several WebView2 environments with different user-data folders run in one process |
| PROFILE-004 shared dirs | Same run | No `<exe>.WebView2` default folder next to the binary (the task asserts it) |
| Mock OIDC login | `task test:oidc` | PASS: login, persistence across restart, second profile stays logged out |
| Shortcuts while the page has focus | Clicked into the page, pressed Ctrl+2, then Ctrl+L | Ctrl+2 switched to the second app, Ctrl+L opened the Open panel (WebView2 `AcceleratorKeyPressed`, `win.rs`). Mapping unit-tested (`win::tests`) |
| Permissions | Local page asking for geolocation, camera and notifications | Geolocation denied (code 1), camera `NotAllowedError`, notifications granted while the app's switch is on; no WebView2 prompt appeared |
| Page notifications | Same page called `new Notification(..)` | Shown as a Windows toast from "GPUIdium" (app name, title, body; text XML-escaped), checked via `ToastNotificationManager.History`. The page received its `show` event. Toast sender registered under `HKCU\Software\Classes\AppUserModelId\io.github.rpietzsch.GPUIdium` |
| Favicons | Wikipedia | 434-byte PNG received through `FaviconChanged`, cached under `favicons\` |
| Load errors | App with `http://localhost:1/` | Edge's error page is off; the shell's own "This page could not be loaded" state with Retry / Open in browser / Edit address |
| Icon | Taskbar, window | App icon from the embedded resource (`build.rs`) instead of the default |
| `task check` | fmt, clippy `-D warnings`, all tests | Clean |

## Bug found and fixed: crash when adding an app (reported by the owner)
**Symptom:** adding an app from the Open panel (e.g. `chat.eccenca.com`) closed GPUIdium at once (Windows event log: exception `0xc0000409`, a Rust abort).
**Cause (debug build backtrace):** creating a WebView2 waits for asynchronous results with a nested message loop (`webview2_com::wait_with_pump`). That loop also ran GPUI's queued foreground tasks, here the Open panel's text-cursor blink, while GPUI's app state was still borrowed by the Enter key handler; the task's borrow failed and GPUI aborted. Apps created at startup or by Ctrl+1..9 usually got lucky (nothing GPUI-related was queued).
**Fix:** `lb-wry` patch 8 (`third_party/lb-wry/PATCHES.md`): the wait dispatches only messages for non-GPUI windows and re-posts GPUI's messages afterwards.
**Result:** Ctrl+L, typed `chat.eccenca.com`, Enter: app added and saved, page and favicon loaded, no crash. `task check`, `task test:isolation`, `task test:oidc` still pass.

## Not covered yet
- Clicking a toast (routes to the app and calls the page's `onclick` via `ReportClicked`; written, not clicked in a test).
- Mute switch (`IsMuted`), taskbar unread dot, profile deletion while WebView2 releases its files (retried for 2 s): implemented, not observed.
- DPI scaling other than the test machine's, multiple monitors, resize stress, 100-switch run.
- Spell checking: WebView2 checks with the Windows languages; the switch has no effect yet.
- Windows on ARM, Windows 10, a machine without the WebView2 runtime, hosted CI runners.
