# Local patches to lb-wry 0.53.3

1. `src/webkitgtk/web_context.rs`: `WebContextImpl::new` sets base/disk/dom/appcache cache, HSTS and ITP
   directories under the profile data directory (PROFILE-004). Intended for upstream submission.
2. Same function: enables WebKit's favicon database under `<profile>/favicons` so `WebView::favicon()` and the
   `favicon` notify signal work (GPUIdium shows page favicons in its sidebar).
3. `InnerWebView::set_bounds` / `set_visible` skip calls that would apply the same value as last time. Embedders (gpui-webview) call both on every frame, which caused X requests, GTK relayout and WebKit resizes on every hover redraw of the shell.
4. `InnerWebView::set_visible`: after the first show, an X11 child view is toggled by `XMapWindow`/`XUnmapWindow` plus `set_child_visible` instead of hiding/showing the wrapper `GtkWindow`. GTK resets that window to its 200x200 default on every show; that left the page a small square (with the de-dupe in 3) and, when repaired by re-applying bounds, cost two relayouts per switch on complex pages. Measured on 20k-node pages: 0 relayouts per switch, key-to-first-frame 164 ms -> 48 ms.
