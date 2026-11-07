# Known limits and privacy (Linux alpha)

## Privacy
- No telemetry, no account, no GPUIdium server. Diagnostics are off unless `GPUIDIUM_LOG` is set, and then log URLs without query, fragment or userinfo.
- `config.json` holds app names, start URLs, order and opaque profile ids. Cookies/tokens live only in the engine's per-profile directory under the user data root (`~/.local/share/GPUIdium/profiles/<profile_id>/`).
- Removing an app keeps its browsing data by default. "Saved data" in the sidebar lists retained profiles with their size and deletes them one at a time.
- Export writes names and start URLs only (no ids, no cookies). Import always creates fresh profiles.

## Isolation guarantee and its limits
Each app has its own WebKitGTK data store: cookies, localStorage, IndexedDB, HTTP cache, DOM cache, HSTS and ITP all live under the profile directory (`lb-wry` patched, see `third_party/lb-wry/PATCHES.md`). Two entries for the same site can be signed in as different users. This separates app instances; it does not protect against malware running as your user, nor does it sandbox page code from the OS beyond what WebKit itself does.

## Login behaviour (tested with a mock OIDC provider, `task test:oidc`)
| Case | Result |
|---|---|
| Authorization-code redirect flow (RP -> IdP login form POST -> callback -> RP), `state` cookie checked | Works |
| Session cookies with `Max-Age`/`Expires` after restart | Login persists |
| Cookies **without** `Max-Age`/`Expires` ("session cookies") after restart | **Lost**, user must log in again (WebKitGTK does not persist them) |
| Second app on same IdP/RP | Separate login, never sees the first one |
| Sites that open the login in a **popup** (`window.open`) | The popup is sent to the system browser and cannot hand the result back; use a redirect-based login or sign in via the site's non-popup option |
| Sites that refuse embedded browsers | Not supported; "Open in browser" is the fallback |

## Platform
- Linux X11 and XWayland only. Native Wayland is not supported. Requires GTK 3 and WebKitGTK 4.1.
- No load-progress UI beyond "(loading…)" and an error line when the engine reports a failed load.
- Windows and macOS are not verified (see ADR-001).

## Deleting data
After "Delete data" the profile directory is removed immediately and again after 2 s and 6 s, because WebKit's helper processes may recreate stub files. Quit within those seconds and a stub may remain; it is listed under "Saved data of removed apps" and can be deleted there.

## Testing aid
`GPUIDIUM_UNLOAD_SECS=<n>` overrides the unload-inactive limit (seconds). It exists only for tests.
