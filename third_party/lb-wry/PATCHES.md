# Local patches to lb-wry 0.53.3

1. `src/webkitgtk/web_context.rs`: `WebContextImpl::new` sets base/disk/dom/appcache cache, HSTS and ITP
   directories under the profile data directory (PROFILE-004). Intended for upstream submission.
2. Same function: enables WebKit's favicon database under `<profile>/favicons` so `WebView::favicon()` and the
   `favicon` notify signal work (GPUIdium shows page favicons in its sidebar).
3. `InnerWebView::set_bounds` / `set_visible` skip calls that would apply the same value as last time. Embedders (gpui-webview) call both on every frame, which caused X requests, GTK relayout and WebKit resizes on every hover redraw of the shell.
4. `InnerWebView::set_visible`: after the first show, an X11 child view is toggled by `XMapWindow`/`XUnmapWindow` plus `set_child_visible` instead of hiding/showing the wrapper `GtkWindow`. GTK resets that window to its 200x200 default on every show; that left the page a small square (with the de-dupe in 3) and, when repaired by re-applying bounds, cost two relayouts per switch on complex pages. Measured on 20k-node pages: 0 relayouts per switch, key-to-first-frame 164 ms -> 48 ms.
5. macOS (`src/wkwebview/mod.rs`, `src/error.rs`): when `with_data_store_identifier` is set but the OS has no
   per-identifier stores (macOS < 14), building the WebView fails with `Error::CustomDataStoreUnavailable` instead
   of silently using the shared default store (which would merge every profile).
6. macOS build hygiene: deprecated `NS*KeyMask` constants replaced by `NSEventModifierFlags::*`; unneeded `unsafe`
   blocks removed; `WebViewId<'_>` lifetime spelled out; the still-needed `NSFilenamesPboardType` use is
   `#[allow(deprecated)]` (unchanged behaviour); release-only unused `NSObject` import / `error` parameter fixed. No functional change.
