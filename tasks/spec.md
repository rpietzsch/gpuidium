# GPUIdium — Product & Engineering Specification

**Status:** Proposed / implementation-ready, subject to feasibility gates  
**Version:** 0.2 (2026-11-07) — owner decisions incorporated  
**Target:** Windows 11, macOS, Linux (X11 and XWayland initially)  
**License proposal:** Apache-2.0 or MIT (owner decision before publishing)  
**Locked decisions:** GPUI mandatory; persistent isolated profile per app instance in MVP; Linux X11/XWayland supported initially (native Wayland deferred).  
**Product hypothesis:** A lightweight, keyboard-friendly, GPUI-native shell that treats every web app as a URL/IRI, not as a curated service integration.

## 1. Executive decision

**Build a GPUI desktop shell with native embedded WebViews, not a browser engine.** Start with GPUI + GPUI Kit `gpui-webview` (experimental Wry integration), using the system's WebView engine: WebView2 (Win11), WKWebView (macOS), WebKitGTK (Linux). Keep an internal `WebviewHost` abstraction so we can replace the integration without rewriting product code.

**Hard gate:** Do not promise a three-platform release until spikes prove child WebViews, switching, resizing, focus, navigation **and persistent per-app-instance storage isolation** on Windows 11, macOS and Linux X11/XWayland. Pure Wayland is explicitly *not* MVP-supported. **GPUI is mandatory**: if a required platform fails, stop that platform’s release and pursue GPUI-compatible adapter work or upstream fixes. **Do not switch to Tauri or another UI framework.**

This is a **web-app container**, not a general-purpose secure browser. It must never claim to reduce page-renderer memory to GPUI levels: web content still runs in a browser engine.

## 2. User story and goals

As a user, I paste a web address and immediately get a persistent sidebar entry. I can open multiple web applications, switch rapidly using keyboard or mouse, remain logged in across restarts, and open external authentication links safely. I don't need a recipe catalog, built-in service definitions, script injection, or an account with GPUIdium.

**MVP goals**
1. Add an app by an HTTPS URL (IRI syntax allowed through deliberate normalization to a URL understood by Wry).
2. Persist apps and their order locally, and restore the workspace across restarts.
3. Display one active embedded WebView; inactive services may retain their session as resources permit. Every app instance has its own persistent browser-data partition.
4. Basic navigation: back, forward, reload, open in external browser; clearly show current origin.
5. Persist authentication sessions across restarts **without sharing cookies, local storage or other origin-scoped browsing data between app instances**, including two instances of the same site; disclose unsupported or problematic sites.
6. Run on Windows 11, macOS and Linux X11/XWayland with documented prerequisites.
7. Remain usable and stable without a GPUIdium server, recipes, analytics, or telemetry.

**Non-goals (MVP)**: Ferdium recipe compatibility; unread badges; service-specific scripts; unified notifications; password vault; synchronization; cross-device accounts; extensions; ad blocking; automatic suspend/hibernate; browser-tab parity; mobile; native Wayland; user-defined arbitrary protocols or script URLs.

## 3. Terminology and URL/IRI handling

A user may enter an **IRI** (Internationalized Resource Identifier), including Unicode characters. The embedded WebView loads a **URL**, after parsing and normalization.

- Accept absolute `https://` addresses and a bare hostname (interpreted as `https://hostname/`). For example `https://bücher.example/über` becomes a valid percent-encoded URL with IDNA-normalized hostname.
- Optionally accept `http://` **only** for loopback hosts (`localhost`, `127.0.0.1`, `[::1]`) with an explicit warning. Reject plain HTTP on remote hosts in MVP.
- Reject `file:`, `javascript:`, `data:`, `about:`, `chrome:`, `edge:`, custom schemes and `ftp:` as an app's *initial URL*; never auto-convert input to code.
- Do not strip path, query or fragment: the initial URI might identify a specific workspace. Never store raw credentials/userinfo in URLs; reject userinfo inputs.
- Canonicalize for safe navigation, but preserve a friendly display IRI when possible. The platform browser decides actual navigation behavior. Domain Unicode/IDNA confusables are a risk: show the canonical host/origin in the app-detail view.
- One app entry is a **user-managed website instance**, not a predetermined service. Duplicate sites are permitted and **must use distinct profiles from day one**; even two entries with the same URL must not share account data.

## 4. UX / interaction contract

**Window:** native title bar, left sidebar (~56–240 px adaptable), content region reserved exclusively for the native WebView, compact top toolbar *outside* its rectangle. Avoid GPUI popovers drawn over the WebView, particularly on Windows. Prefer full-width fixed GPUI top bars, native context menus, or temporarily hide/resize the WebView for dialogs.

**Empty state:** “Add a web app” URL field with an example. Return validates, adds the app, creates its view, and navigates.

**Sidebar:** app icon (use a generated monogram initially; website favicons only if safely fetched later), editable display name, drag/drop ordering optional after MVP, active highlight, add button, remove action with confirmation.

**Toolbar:** back, forward, reload, URL/origin information, open externally. A single instance takes the content pane; switching must hide the previous child WebView before showing the next; no stale view may cover GPUI chrome.

**Keyboard MVP:** Ctrl/Cmd+L (focus add/navigate URL field), Ctrl/Cmd+R (reload), Ctrl/Cmd+1…9 (switch first nine apps), Ctrl/Cmd+W (close active app **with confirmation**, or avoid binding if platform semantics conflict). Esc returns focus to content where appropriate. Accessibility: visible focus indicator, labeled controls, keyboard traversal, OS screen-reader smoke test.

**Empty/error states:** invalid URI; network failure; web engine unavailable; site blocks embedded sign-in; failed external auth handoff; corrupt local config. Never silently present a blank pane.

## 5. Architecture

```text
┌──────────────────────────────────────────────┐
│ GPUI shell (sidebar, toolbar, settings)      │
│ AppState  +  Commands  +  NavigationPolicy   │
├──────────────────────────────────────────────┤
│ WebviewHost trait                            │
│ create/show/hide/navigate/reload/focus/close  │
│ events: load-start/finish/fail, URL, title    │
├──────────────────────────────────────────────┤
│ gpui-webview adapter (experimental)          │
│ Wry / platform-native child WebViews         │
├───────────────┬───────────────┬──────────────┤
│ Win: WebView2 │ mac: WKWebView│ Linux:       │
│               │               │ WebKitGTK X11│
└───────────────┴───────────────┴──────────────┘
             Local JSON configuration
            Native browser profile storage
```

**Suggested Rust modules:** `app` (GPUI entry), `model` (typed state), `ui` (sidebar/toolbar), `uri` (parse/normalize/policy), `webview_host` (trait), `webview_gpui_wry` (adapter), `storage` (atomic persistence), `platform` (OS preflight), `diagnostics` (opt-in local logs). No service registry.

**WebviewHost contract (conceptual, not guaranteed actual crate signatures):**

```rust
trait WebviewHost {
    fn create(&mut self, app: AppId, initial_url: &url::Url) -> Result<(), HostError>;
    fn show(&mut self, app: AppId) -> Result<(), HostError>;
    fn hide(&mut self, app: AppId) -> Result<(), HostError>;
    fn navigate(&mut self, app: AppId, url: &url::Url) -> Result<(), HostError>;
    fn back(&mut self, app: AppId) -> Result<(), HostError>;
    fn forward(&mut self, app: AppId) -> Result<(), HostError>;
    fn reload(&mut self, app: AppId) -> Result<(), HostError>;
    fn focus(&mut self, app: AppId) -> Result<(), HostError>;
    fn close(&mut self, app: AppId) -> Result<(), HostError>;
    // Emit events via a bounded channel; all UI state updates occur on GPUI's UI thread.
}
```

The adapter owns WebViews and contexts with valid lifetimes. Construct native children only after the GPUI window exists. Reuse each WebView between GPUI render passes. Synchronize bounds and visibility with the wrapper; do not mutate raw native handles without coordinating layout.

### Session and profile strategy — mandatory MVP isolation

**Owner decision (binding): each `AppId` owns a separate, durable browser profile.** A profile is automatically created when an app is added; there is no shared default profile. Separate entries pointing to the same origin must be able to use different signed-in accounts concurrently. An app's WebView(s) must use only that app's profile. Profile identity is keyed by an opaque `profile_id`, **not** URL, host, or display name.

**Isolation acceptance:** for two entries A and B on the same origin, write cookies, `localStorage`, and (where supported) IndexedDB/service-worker data in A; verify B cannot read them. Repeat in the opposite direction, after restarting the app, and after switching/removing unrelated entries. This is an **app-instance separation** requirement, not a guarantee of OS-process isolation or protection from malware running as the same OS user.

**Cross-platform feasibility is a blocking G1 gate.** Test that GPUI's embedded-Wry adapter can construct independent persistent WebView data stores on Windows WebView2, macOS WKWebView, and Linux WebKitGTK. Investigate Wry `WebContext`, WebView2 user-data folders, WKWebsiteDataStore persistent identifiers/OS constraints, and WebKitGTK website data manager/context lifetime. APIs and behavior may differ by OS, and a filesystem `data_directory` path alone is **not** proof of isolation, especially on macOS. Record exact library/runtime versions and minimum supported macOS release after testing; upgrade support baselines if required rather than silently using a shared store.

**Data lifecycle:** store profile metadata under the platform's user-data root and WebView engine-owned data separately from `config.json`; no cookies or tokens in JSON. Removing an app must not silently destroy its data: MVP provides an explicit **“Remove app and delete its browsing data”** option and a default **“Remove app but retain data”** behavior (with a documented way to purge orphaned profiles), or a safer equivalent approved through a data-retention review. Profile deletion must be scoped to the selected `profile_id` and run only after the WebView and browser context are closed. On re-add, allocate a fresh ID, never implicitly attach an old account. Handle profile-in-use and partial-cleanup errors without erasing other profiles.

**If isolation is not achievable on any required platform, G1 is NO-GO for a full cross-platform MVP.** Do not degrade to a shared profile, claim a passing release, or substitute Tauri. Pursue a GPUI-compatible native hosting adapter or upstream work and rerun the test matrix.

### Navigation and security

- Treat every page as untrusted. Disable arbitrary page-to-native IPC; **do not add a JavaScript bridge in MVP**.
- Allow normal HTTPS same-window navigation; route new windows/popups and external protocols via explicit policy, never silently execute arbitrary OS handlers.
- Authentication: test redirects, OAuth provider flows, and popup handoff; allow trusted user-initiated external-browser handoff with confirmation for unusual schemes, but never promise browser-to-app login completion without verified callback handling.
- Use Wry navigation and new-window handlers to intercept at the native boundary; do not trust a cosmetic address bar as security enforcement.
- Never pass secrets (cookie headers, URL credentials, tokens, local data paths) to telemetry or default logs. Minimize logging of URL query strings.
- Native WebViews have vulnerabilities too; publish update guidance and keep Wry/GPUI dependencies current.
- No remote script injection, recipes, arbitrary user JS, universal permission bypass, custom certificate ignore modes, or insecure devtools in release builds.

### Build and lifecycle tooling

- The repo provides a root `Taskfile.yml` ([go-task](https://taskfile.dev)) as the single entry point for build and lifecycle targets. Developers, CI and agents invoke `task <target>` rather than ad-hoc cargo/shell commands.
- Minimum targets: `clean`, `build` (debug), `build:release`, `run`, `test`, `fmt`, `lint` (clippy, `-D warnings`), `check` (fmt check + lint + test; the CI gate) and `default` (lists targets). Packaging targets (`package:*`) are added with the PKG tickets.
- Targets must be cross-platform (Windows 11, macOS, Linux) or explicitly platform-gated, and must wrap the Rust workspace pinned in SPIKE-001. Required system packages (e.g. GTK/WebKitGTK on Linux) are documented, not silently installed.
- CI (QA-001) runs the same `task check` used locally, so local and CI results cannot drift.

## 6. Platform-specific constraints

| Platform | Web engine | MVP target | Important constraints |
|---|---|---|---|
| Windows 11 x64 | WebView2 | Required | Confirm WebView2 runtime, GPU child composition, focus, native view z-order; GPUI Kit example disables DirectComposition for visibility; do not assume that toggle is an ideal production solution. |
| macOS arm64 + x64 | WKWebView | Required | Verify signing/notarization, focus, cookie persistence, native WKWebView attachment and multi-view switching; independent persistent app-instance profiles must pass G1. |
| Linux x64 (X11) | WebKitGTK 4.1 / GTK3 | Required | System packages, GTK initialization and event-loop integration, X11 child view, focus, sizing. |
| Linux Wayland desktop via XWayland | WebKitGTK 4.1 | Required as compatibility mode | Launch the *entire GPUI app* using X11 backend; check that XWayland is present; scaling/focus may differ. |
| Linux native Wayland | — | Not in MVP | `gpui-webview` cannot natively embed its Wry view in GPUI's Wayland window. Separate research item. |
| Windows arm64, Linux arm64 | platform-dependent | Later | Build/test only after core targets stabilize. |

Define and publish actual minimum supported OS versions once the chosen GPUI/Wry combination is built and tested; do not infer them from framework marketing pages.

## 7. Data model and persistence

**Config example** (`$APP_CONFIG/GPUIdium/config.json`; use platform-standard config paths):

```json
{
  "schema_version": 1,
  "apps": [
    {
      "id": "7e337f1e-95d2-4e28-94df-a91791eb6fea",
      "name": "Work chat",
      "start_url": "https://chat.example.org/work",
      "profile_id": "a7ac6491-27c1-45e5-a392-a64d4ec23471",
      "order": 0
    }
  ],
  "active_app_id": "7e337f1e-95d2-4e28-94df-a91791eb6fea",
  "window": { "width": 1200, "height": 800 }
}
```

- UTF-8 JSON, versioned schema, explicit migrations, robust defaults; UUID app IDs and distinct opaque profile IDs, never use a raw URL as a directory name. Enforce one profile ID per app instance and prevent accidental ID aliasing during import/migration.
- Atomic write (temp file + flush + rename), backup last-known-good; avoid saving on every re-render.
- On corrupted config: preserve original, restore backup or present recoverable empty state. Never delete web engine session profiles automatically.
- Store data under OS-appropriate per-user config/data/cache roots, rather than an installation directory.
- Single-instance lock or documented safe multi-instance behavior to prevent profile/config corruption.
- Remove app: default keeps its browser data, with clear retention notice; a separate explicit action deletes only the selected app profile after its WebView closes. Never wipe another app’s data; surface partial deletion failures.

## 8. Performance targets and fair benchmarks

There is **no claimed GPUI-vs-Tauri memory victory** until measured end to end. Measure complete process tree, not just the Rust parent process. WebView2, WKWebView and WebKitGTK use different process models, so compare on the **same OS/machine/site set**, warmed and freshly launched.

**Provisional engineering budgets (non-blocking until baseline):** shell-only cold first-paint p50 < 2 s on reference development machines; tab switch p50 < 150 ms for already loaded views; no sustained CPU spin on idle shell; zero invisible WebViews overlapping chrome. Report memory, CPU and startup p50/p95 rather than asserting absolute limits across devices.

**Benchmark matrix:** 0 apps; 1 static page; 3 local deterministic pages; 5 real services (manual opt-in, no stored real user credentials in CI). Record OS/architecture, display server, native WebView engine version, baseline idle, cold startup, warm switch, resize, memory RSS/PSS or platform equivalent, CPU over 60 s, package size and crashes. For apples-to-apples shell comparisons, build a minimal Tauri+Wry prototype using identical static test sites and identical process-tree accounting.

## 9. Tests and acceptance criteria

**Unit:** IRI/IDNA normalization; scheme rejection; origin policy; JSON migration; persistence failure recovery; UUID stability; state transitions; event lifecycle; external-link policy.

**Integration:** create/switch/close 3 webviews; prove two same-origin app instances have separate persistent cookie/localStorage and compatible IndexedDB/service-worker partitions; resize/DPI changes; back/forward/reload; persist app order and active app; keyboard focus return; no WebViews leak on repeated add/remove; crash recovery; hyperlink and popup interception.

**Security:** `javascript:`/`file:` app URI rejected; new-window requests never silently execute external protocols; JS cannot invoke privileged native commands; URL credentials rejected; cross-origin navigation honors explicit policy; maliciously malformed config cannot execute code.

**Platform smoke acceptance** (must be manually recorded on each OS):
1. Clean install launches without development tools.
2. Add `https://example.com` and a Unicode IRI test page, both load correctly.
3. Add three local synthetic pages and switch 100 times without crash, stuck focus, blank screen or z-order artifacts.
4. Resize across 1000/1250/1500 logical pixels (or platform equivalents); content never overlaps toolbar/sidebar.
5. Reload and navigate back/forward.
6. Restart and recover list, active app, **distinct profile IDs**, and separate test cookie sessions for two same-origin instances.
7. Exercise external links, popup/login flow, unavailable network, network recovery.
8. Remove one app with data retained, then perform explicit delete-data flow on another test profile; other apps’ sessions remain intact and removed data does not silently reattach.
9. Test clipboard, copy/paste, accessibility focus and high-DPI behavior.
10. Run five webviews for 30 minutes with no unbounded growth or crashes (document known browser memory retention).

**Release criterion:** all MVP acceptance tests pass on Win11, macOS arm64 and x64, Linux X11, and Wayland-session XWayland compatibility mode. If a target fails, label preview release **unsupported** on that target rather than asserting full multiplatform support.

## 10. Delivery gates and GPUI-only remediation

**G0 — research:** Pin compatible versions of `gpui`, `gpui-kit`, `gpui-webview`, `wry`; run the maintained GPUI Kit WebView example without modifying its dependency graph blindly. Record license and OS requirements.

**G1 — WebView + isolation spike (blocking):** Native child WebViews inside the GPUI shell on each required OS; resize/focus/navigation; prove switching two same-origin views, **distinct persistent storage partitions**, no cross-session cookie/localStorage leakage and session survival after restart. Explicitly validate Windows DirectComposition behavior and Linux XWayland. Produce short repros for broken rendering.

**G2 — minimal daily driver:** Local config, add/edit/remove entries, sidebar, one active view, keyboard operations, recoverable errors, update docs.

**G3 — public alpha:** Packaging, CI + manual cross-platform smoke, signing strategy, basic security review, benchmark report, known-limits page.

**Mandatory GPUI decision:** If G1 fails on a required platform, record NO-GO and suspend the cross-platform MVP claim. Pursue a per-OS **GPUI-compatible** WebView host, upstream changes, or an explicit platform-specific preview while keeping the GPUI shell. Re-run G1 before claiming support. **Tauri/Electron/framework replacement is out of scope as a fallback.** Avoid unsupported hacks and avoid building a bespoke browser engine.

## 11. Key risks (owner review)

| Risk | Severity | Mitigation / decision |
|---|---|---|
| GPUI/native child WebView layering on Windows | High | G1 physical Windows test; put controls outside view; never depend on overlay. |
| Linux native Wayland not supported | High | Clearly state XWayland compatibility mode; no native Wayland claim. |
| Login sites reject embedded browsers or OAuth | High | Try representative sites; external browser handoff; document incompatibility. |
| Persistent per-app-instance isolation differs by OS | Critical / release blocker | Isolated profiles are mandatory at G1; test two same-origin accounts plus restart on each OS; adjust OS baseline or GPUI host adapter, never silently share a profile. |
| Experimental gpui-webview API churn | Medium | Pin versions + adapter boundary; upstream fixes; no unchecked semver upgrade. |
| Different engines render services differently | Medium | Test service site matrix per platform, no universal compatibility guarantee. |
| GPUI overhead savings overshadowed by browser memory | Medium | Report whole-process memory, avoid unsupported efficiency claims. |
| Security surface of arbitrary remote pages | High | No native bridge; strict scheme and navigation policy; dependency updates. |

## 12. Recorded owner decisions and remaining choices

1. **Product definition:** Is this fundamentally a Ferdium-like multi-service shell? **Assumed yes.** No built-in service list, no predefined recipes.
2. **Profile behavior:** **Decided: isolated persistent profile per app instance in MVP.** No shared-profile fallback, including for duplicate URLs.
3. **Linux requirement:** **Decided: X11 and XWayland for MVP.** Native Wayland is a distinct R&D project.
4. **License:** Apache-2.0 vs MIT? **Proposed Apache-2.0.**
5. **Framework:** **Decided: GPUI mandatory.** No Tauri/Electron fallback if a spike fails; record NO-GO and fix the GPUI integration or explicitly defer the affected platform release.

## 13. Primary technical references (verified 2026-11-07)

- GPUI Kit WebView docs: https://gpui-kit.com/docs/webview/
- GPUI WebView source/readme: https://github.com/longbridge/gpui-kit/blob/main/crates/webview/README.md
- Wry documentation: https://docs.rs/wry/latest/wry/
- Wry `WebViewBuilder` and Linux child/GTK constraint: https://docs.rs/wry/latest/wry/struct.WebViewBuilder.html
- Wry `WebContext`: https://docs.rs/wry/latest/wry/struct.WebContext.html
- GPUI platform examples: https://gpui.rs/examples/

**Implementation note:** Documentation and published APIs change. Pin and compile a known compatible version set at G0; the sample interfaces above are conceptual, not a promise of drop-in Rust code.
