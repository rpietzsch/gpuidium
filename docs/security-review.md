# Security review (QA-004), Linux, 2026-11-07

Scope: code in `crates/` and the `lb-wry` patch. Self-review, not an external audit.

| Area | Finding | Status |
|---|---|---|
| Initial/typed URLs | Only https, and http for loopback; `javascript:`, `file:`, `data:`, userinfo rejected; fuzz-style sweep test | OK |
| In-page navigation | `with_navigation_handler` applies `decide_navigation`; non-web schemes blocked | OK |
| Popups / new windows | Always denied in-app; http(s) handed to the system browser, others blocked; URL redacted in status | OK |
| Page-to-native IPC | No IPC handler, no init scripts, no `evaluate_script` in product code; `devtools` feature not enabled | OK |
| Logs | Opt-in; `redact_url` drops query/fragment/userinfo; data paths not logged | OK |
| Cross-profile data | Cookies, localStorage, IndexedDB, HTTP cache, DOM cache, HSTS, ITP per profile; verified by `task test:isolation` including "nothing in shared dirs" | OK |
| Delete data | Scoped to one profile dir, refuses symlink escape, tested; orphan purge refuses ids still used by an app | OK |
| Import | Size and count capped; URLs revalidated by the same parser; never carries ids | OK |
| Engine updates | We depend on the system WebKitGTK; keep it patched. `lb-wry` is vendored with one patch | Documented |
| Residual | `GPUIDIUM_LOG` could log page paths (not queries); profile dirs are not encrypted | Accepted |
| Not covered | Windows/macOS; certificate error handling is WebKit default (no bypass UI exists) | Open |
