# Local patches to lb-wry 0.53.3

1. `src/webkitgtk/web_context.rs`: `WebContextImpl::new` sets base/disk/dom/appcache cache, HSTS and ITP
   directories under the profile data directory (PROFILE-004). Intended for upstream submission.
2. Same function: enables WebKit's favicon database under `<profile>/favicons` so `WebView::favicon()` and the
   `favicon` notify signal work (GPUIdium shows page favicons in its sidebar).
