# Linux verification log - Milestone 3 (2026-11-07)

Environment: CachyOS, KDE Plasma Wayland session, app run on XWayland, WebKitGTK 2.54.2, debug build. GUI checks drive the real app with `xdotool` (keys, mouse) and `spectacle` screenshots, always with isolated `XDG_*` directories. Config and profiles are seeded by a small script; nothing touches the user's own GPUIdium data.

| Ticket | What was done | Observed |
|---|---|---|
| UX-010 drag reorder | Seeded apps alpha, beta, notif; mouse-down on tile 1, dragged in steps onto tile 3, released | Saved order became beta, notif, alpha |
| UX-011 favicons | Page with `<link rel=icon>` and a page without | Favicon shown in the tile and cached under `<data>/favicons/`; page without one shows the letter; restored on restart before any load |
| UX-012 finder | Ctrl+L, typed `bet` | List filtered to *beta*; Enter switched and persisted the active app |
| PROFILE-010 export/import | Clicked Export, then Import with a file containing one `javascript:` URL | Export holds only name+URL (no ids); import added 2 apps with unique new profile ids, status "Imported 2 app(s), skipped 1" |
| PROFILE-011 data management | Three retained profile dirs; clicked a single Delete, then Delete all | One removed, then the rest; the app's own profile directories were untouched |
| LIFE-010 unload | `GPUIDIUM_UNLOAD_SECS=6` (test-only override), switched beta then alpha, waited | Idle app unloaded, active app kept, unloaded app re-created lazily on select. Rule also unit-tested (`lifecycle.rs`). Settings button cycles Off/10/30/60 min (clicked) |
| UX-013 notifications | Page requests permission on click; PATH has a fake `notify-send` writing its arguments to a file | Opt-in on: permission granted, `notify-send --app-name=GPUIdium -- "notif: Hello title" "Hello body"`; off: denied, no call. Settings toggle clicked on/off |
| UPD-010 integrity | `task release:linux` with a throwaway GPG key | Tarball + `.sha256` + `.asc`, verify OK; a tampered copy fails with exit 1 |
| UI-002 (M1) | Clicked Edit, changed name and URL, Save; Remove -> Delete data | Name/URL updated, profile id unchanged; profile removed |

## Bug found and fixed by this pass
After "Delete data", WebKit's helper processes recreated part of the profile directory (origin metadata, no cookies) right after it was removed. Deletion is now repeated after 2 s and 6 s; the directory ends up gone. If the app is closed within those seconds a stub can remain and shows up under "Saved data of removed apps".

## Not covered
Windows and macOS (no hardware); real-world sites for notifications; screen-reader behaviour (UI-003); `deb`/`rpm`/AppImage.

## Page focus and switching (reported by the owner, 2026-11-07)
**Symptom:** text caret often invisible in web inputs although typing worked.
**Cause (measured):** after every switch `document.hasFocus()` was `false` and no `focus` event fired. `lb-wry` wraps GPUI's X11 window in a GTK window whose focus events go to GPUI's connection, so GTK never marked the window active and WebKit never marked the page focused. The host also re-applied show/hide on every UI event.
**Fix:** `show_only` now applies visibility changes only, and when a page is shown the host delivers the missing `focus-in-event` to the GTK toplevel, then focuses the view (`host.rs`, `activate_page`).
**Result:** after every switch `hasFocus=true` and a `focus` event fires; typing into the input reaches the right page in both apps (tested with real key events).

**Slow re-render after switching (~2 s on real sites):** not reproduced. With test pages (including a 15k-node page with a title changing 4x/s) WebKit produced a frame ~100-200 ms after the switch and the screen showed the correct page by the first sample (~0.9 s) both with and without the old per-event show/hide churn. Likely tied to heavy real sites or the GPU/compositing path; see `docs/known-limits.md` for the A/B recipe.
