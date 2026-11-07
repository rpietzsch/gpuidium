# GPUIdium — Implementation Backlog

**Version:** 0.2 · 2026-11-07 (owner decisions incorporated)
**Locked decisions:** GPUI-only shell; isolated persistent browser profiles per app instance are P0; Linux X11/XWayland first, native Wayland later.  
**Purpose:** Ordered, dependency-aware tickets that a Rust desktop team can pick up immediately.  
**Suggested tracking fields:** ID, priority, owner, status, estimate, dependencies, PR link, evidence (screenshot/video/test).  
**Estimates:** S (≤1 day), M (2–3 days), L (4–7 days), XL (research/unknown); exploratory estimates are not commitments.

## Progress (updated as work lands)

| Item | Status | Evidence |
|---|---|---|
| SPIKE-001 | **Done** | Pins in `Cargo.toml`/`rust-toolchain.toml`, `Cargo.lock`; ADR-001 version table |
| BUILD-001 | **Done** (Linux verified; Win/macOS targets untested) | `Taskfile.yml`, README |
| SPIKE-004 | **Partial** - Xwayland session verified; pure X11, DPI, focus/resize/100-switch not yet | ADR-001 |
| SPIKE-005 | **Partial** - Linux PASS (cookie/localStorage/IndexedDB, restart); macOS/Windows need hardware | `spike_isolation`, ADR-001 |
| SPIKE-002, SPIKE-003 | **Blocked** - need macOS / Windows machines | - |
| SPIKE-006, SPIKE-007 | **Open** - ADR-001 is PROVISIONAL | ADR-001 |
| CORE-001, CORE-002, CORE-003 | **Done** (29 unit tests; fuzz-style URL sweep) | `crates/gpuidium-core` |
| PROFILE-004 (new, P0) | **Open** - per-profile HTTP cache / HSTS on Linux (found by SPIKE-005); verify equivalents on macOS/Windows | ADR-001 finding 2 |

## Definition of done (all tickets)

- Code reviewed, `cargo fmt`, `cargo clippy`, tests relevant to changed modules, and no leaked credentials in logs.
- Changed behavior covered by a test or documented manual verification.
- Feature/platform limitations recorded in README or known-issues page.
- For platform-specific changes, supply evidence on affected OS and avoid regressing others.
- PR explains how it affects whole-process performance and security, if applicable.

## Milestone 0 — Feasibility gate: GO / NO-GO (P0, do first)

| ID | Size | Ticket | Acceptance / proof | Dependencies |
|---|---|---|---|---|
| SPIKE-001 | M | Create Rust workspace; pin compatible GPUI/GPUI Kit/gpui-webview/Wry versions | Clean builds, version lock, linked upstream example, licenses documented | — |
| SPIKE-002 | L | macOS WKWebView child smoke | Page loads, focus, resize, URL/navigation, reload, switching 2 webviews, state across restart; test arm64 and x64 | 001 |
| SPIKE-003 | L | Windows 11 WebView2 child smoke | Same as 002; explicit test of DirectComposition workaround and z-order, toolbar unobstructed | 001 |
| SPIKE-004 | L | Linux X11 / XWayland WebKitGTK child smoke | Same as 002 on native X11 and Wayland session via XWayland; required GTK/WebKit packages documented; DPI tested | 001 |
| SPIKE-005 | XL | **Prove persistent per-app-instance profile isolation** | On each OS, create two same-origin apps with distinct `profile_id`s; verify distinct cookies/localStorage, feasible IndexedDB/service-worker partitioning, both accounts persist after restart; document underlying WebView store APIs and minimum supported OS versions | 002,003,004 |
| SPIKE-006 | M | Document GPUI WebView composition constraints | Reproduce menu/dialog behavior, fullscreen, resizes, focus, popup opening on 3 OSes | 002,003,004 |
| SPIKE-007 | S | ADR-001: GPUI-only GO/NO-GO | Pass only with WebView composition **and isolation** on all required OSes; if blocked, record GPUI-compatible adapter/upstream remediation and affected-platform hold; **no Tauri fallback** | 005,006 |
| BUILD-001 | S | Root `Taskfile.yml` with build/lifecycle targets | `task` lists targets; `clean`, `build`, `build:release`, `run`, `test`, `fmt`, `lint`, `check` work on Win11, macOS and Linux (or are documented as platform-gated); `check` is the CI entry point; README documents `task` install and usage | SPIKE-001 |

**Gate G1 must pass** before implementing polished UI. It includes real persistent isolation, not just cookie persistence. Parallelize OS spikes. GPUI is a hard constraint: failed gates block release claims; work toward a GPUI-compatible host/patch rather than substituting Tauri.

## Milestone 1 — MVP application (P0)

| ID | Size | Ticket | Acceptance / proof | Dependencies |
|---|---|---|---|---|
| CORE-001 | M | `AppEntry`, `AppId`, `AppState`, commands/events | Typed state transitions; duplicate URIs allowed; unit tests | SPIKE-007 |
| CORE-002 | M | IRI/URL parser + safe normalization | Unicode host/path, bare host; HTTPS default; HTTP loopback only; rejects dangerous schemes/userinfo; unit/fuzz cases | CORE-001 |
| CORE-003 | M | Storage schema v1 + atomic config writes | Persist unique `profile_id` per AppId; robust roundtrip, versioning, backup, corrupt-config recovery; config stored in OS paths without browser secrets | CORE-001 |
| PROFILE-001 | L | **Isolated persistent profile manager (P0)** | AppId maps 1:1 to opaque profile ID; create/open/suspend/close per-app data store through OS-specific backend; never reuse a store between entries | SPIKE-005,CORE-001,CORE-004 |
| PROFILE-002 | M | **Same-origin isolation regression tests (P0)** | Two entries for identical URL keep distinct cookies/localStorage and independent logins before/after restart on Win11, macOS and Linux X11/XWayland; no shared-store fallback | PROFILE-001 |
| PROFILE-004 | M | **Per-profile HTTP cache / HSTS isolation (P0)** | Cache, CacheStorage and HSTS stores live under the profile dir (or are disabled) on Linux; equivalent verified on macOS/Windows; regression test that a cached/HSTS entry in A is not visible in B | SPIKE-005 |
| PROFILE-003 | M | **Profile retention and deletion safety (P0)** | Default remove retains browsing data with notice; explicit delete-data action cleans only selected closed profile; errors/retry/orphan cleanup documented; no cross-profile deletion | PROFILE-001,CORE-003 |
| CORE-004 | M | WebviewHost trait and gpui-webview adapter | No native Wry resource in domain model; owns lifetimes; callbacks correctly hop to GPUI UI thread | SPIKE-007,CORE-001 |
| CORE-005 | L | Native WebView lifecycle for multiple entries | Create, show, hide, destroy; re-render does not recreate views; each view is attached to its own PROFILE-001 storage partition; 100 switches smoke | CORE-004,PROFILE-001 |
| CORE-006 | M | Navigation controls and status | Back, forward, reload, current origin, load/error state; no silent empty pane | CORE-002,CORE-005 |
| CORE-007 | M | Navigation & popup safety policy | New-window interception, explicit external-link behavior, blocked schemes, no JS/native IPC | CORE-002,CORE-004 |
| UI-001 | M | Sidebar, app selection, empty state | Add web app + select; no overlays above native pane; keyboard operable | CORE-001,CORE-005 |
| UI-002 | M | Add/Edit/Remove dialogs | Name/URL editing; remove confirmation and explicit retain/delete profile-data choice; invalid URL diagnostics; works on Windows z-order | UI-001,CORE-002,PROFILE-003 |
| UI-003 | M | Toolbar and keyboard shortcuts | Cmd/Ctrl+L/R/1–9; focus transfer; nav buttons; screen-reader labels | UI-001,CORE-006 |
| UI-004 | S | Visual icon fallback and ordering | Monogram avatar, sortable by stable persisted position (simple move up/down sufficient) | UI-001,CORE-003 |
| CORE-008 | M | Restore workspace and last-active app | Restart restores list/order/active entry and each unique profile mapping; independent same-origin sessions survive | CORE-003,CORE-005,PROFILE-002 |
| CORE-009 | M | Single-instance startup + crash diagnostics | Opening twice safe; errors surfaced; local opt-in diagnostic logs redact secrets | CORE-003 |

**Milestone 1 acceptance:** a clean launch can add three URLs (including two identical origins), log into different accounts in the duplicate entries, switch/navigate/restart and retain both separate sessions on every target. Explicit data deletion must leave other profiles unaffected.

## Milestone 2 — Cross-platform alpha release (P0/P1)

| ID | Priority | Size | Ticket | Acceptance / proof | Dependencies |
|---|---|---|---|---|---|
| QA-001 | P0 | L | Automated parser/state/persistence tests | CI runs `task check` (Rust unit tests, clippy, formatting) on supported hosts | Milestone 1 |
| QA-002 | P0 | L | OS-specific manual and scripted smoke suite | Recorded pass/fail for Win11, macOS arm64+x64, Linux X11 and XWayland | Milestone 1 |
| QA-003 | P0 | M | OAuth/auth and popup compatibility matrix | Test representative web services in separate profiles, including same-site dual accounts; document blockers and external-browser fallback; no deceptive universal claim | CORE-007,PROFILE-002 |
| QA-004 | P0 | M | Security review | No executable schemes, uncontrolled native IPC, secrets in logs or cross-profile data bleed; external links and delete-data handled safely | CORE-007,PROFILE-003 |
| PKG-001 | P0 | L | Windows installer and runtime handling | Install/uninstall, WebView2 preflight, fresh-user smoke, packaged executable signed if release policy requires | QA-002 |
| PKG-002 | P0 | L | macOS app bundle and distribution | Universal binary or separate arm64/x64 artifacts; launch, signing/notarization plan, clean install | QA-002 |
| PKG-003 | P0 | L | Linux deb/rpm/AppImage choice | At least one working distributable; WebKitGTK/GTK dependency and XWayland docs; fresh-user test | QA-002 |
| DOC-001 | P0 | M | README, quickstart, limits, privacy page | Platform prerequisites, no recipe catalog, per-app isolation guarantees and limits, XWayland constraint, known incompatible services | QA-003 |
| PERF-001 | P1 | M | Repeatable baseline benchmark harness | Three deterministic local pages; 0/1/3/5 workloads; process-tree CPU/RAM; 5 runs with p50/p95 | Milestone 1 |
| PERF-002 | P1 | M | Same-machine comparison to minimal Tauri/Wry shell | Shared pages and measurement method; report raw data and methodology, no framework-level overclaim | PERF-001 |
| REL-001 | P0 | S | v0.1-alpha release checklist + artifacts | Builds and test evidence linked; known gaps visible; changelog, reproducible version tags | PKG-001,PKG-002,PKG-003,DOC-001 |

## Milestone 3 — Iteration (P1, only after alpha)

| ID | Size | Ticket | Acceptance / proof |
|---|---|---|---|
| UX-010 | M | Drag/drop app reorder | Sidebar reorder stable across restart and high DPI. |
| UX-011 | M | Site favicon fetching/cache | Safe bounded fetch, no private URL leaks, fallback to monogram, no tracking by GPUIdium. |
| UX-012 | M | App shortcuts and keyboard command palette | No OS shortcut collisions; accessible. |
| PROFILE-010 | M | Export/import an app without leaking its profile | Import assigns a fresh profile ID unless explicitly designed for trusted profile migration; no session/cookie data in JSON exports. |
| PROFILE-011 | M | Profiles management UX | List disk usage, selectively purge orphaned retained profiles, confirm deletion and show failures; no silent cross-profile cleanup. |
| LIFE-010 | M | Pause / unload inactive sites | User-selected option; avoids losing session state; benchmark before/after. |
| UX-013 | M | Basic OS notifications | Permission-aware and opt-in; report platform behavior, no page-to-Rust privilege escalation. |
| UPD-010 | L | Updates and code-signing automation | Secure release path, integrity checks and rollback guidance. |

## Milestone 4 — Research / explicitly out of MVP (P2)

| ID | Ticket | Acceptance / proof |
|---|---|---|
| RND-001 | Native Linux Wayland hosting | Working prototype without forcing GPUI to X11; design reviewed for focus, compositor/security constraints. |
| RND-002 | GPUI/native-WebView overlay composition | Reliable toolbar popups and dialogs over child view across all three OSes, not just macOS. |
| RND-003 | Authentication deep-link return flow | Explicit protocol registration and audited link handling, with OS tests. |
| RND-004 | Site-specific scripting / unread count support | Threat model and permissions design first; no arbitrary injected scripts by default. |
| RND-005 | Webview engine alternative (CEF/Servo/etc.) | Justified by compatibility/performance evidence, shipping complexity and security update model. |

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
