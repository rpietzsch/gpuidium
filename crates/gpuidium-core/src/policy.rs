//! Navigation and popup safety policy (CORE-007).
//!
//! Decides what the native boundary may do with a URL a page asks to navigate to
//! or open. No page-to-native IPC exists; this is the only bridge, and it is
//! decision-only (the caller performs the action).

use url::{Host, Url};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavDecision {
    /// Load in the same WebView.
    Allow,
    /// Hand to the system browser (user-visible, http/https only).
    OpenExternal,
    /// Refuse silently (logged without query string by the caller).
    Block,
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

fn is_web(url: &Url) -> bool {
    match url.scheme() {
        "https" => true,
        "http" => is_loopback(url),
        _ => false,
    }
}

/// In-WebView navigation (links, redirects, `location=`).
pub fn decide_navigation(target: &str) -> NavDecision {
    let Ok(url) = Url::parse(target) else {
        return NavDecision::Block;
    };
    if url.scheme() == "about" && url.as_str() == "about:blank" {
        return NavDecision::Allow;
    }
    if !url.username().is_empty() || url.password().is_some() {
        return NavDecision::Block;
    }
    if is_web(&url) {
        NavDecision::Allow
    } else {
        NavDecision::Block
    }
}

/// `window.open` / `target=_blank` requests. Never opens a new native window:
/// web URLs go to the system browser, everything else is blocked.
pub fn decide_new_window(target: &str) -> NavDecision {
    let Ok(url) = Url::parse(target) else {
        return NavDecision::Block;
    };
    if !url.username().is_empty() || url.password().is_some() {
        return NavDecision::Block;
    }
    match url.scheme() {
        "https" => NavDecision::OpenExternal,
        "http" if is_loopback(&url) => NavDecision::OpenExternal,
        _ => NavDecision::Block,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use NavDecision::*;

    #[test]
    fn navigation_allows_https_and_loopback_http_only() {
        assert_eq!(decide_navigation("https://example.com/a?b=c"), Allow);
        assert_eq!(
            decide_navigation("https://accounts.other.example/oauth"),
            Allow
        );
        assert_eq!(decide_navigation("http://127.0.0.1:8080/"), Allow);
        assert_eq!(decide_navigation("http://localhost/"), Allow);
        assert_eq!(decide_navigation("about:blank"), Allow);
    }

    #[test]
    fn navigation_blocks_everything_else() {
        for u in [
            "http://example.com",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,x",
            "ftp://x/y",
            "mailto:a@b.c",
            "custom-scheme://x",
            "about:srcdoc",
            "https://user:pw@example.com/",
            "not a url",
            "",
        ] {
            assert_eq!(decide_navigation(u), Block, "{u}");
        }
    }

    #[test]
    fn new_windows_go_external_or_get_blocked() {
        assert_eq!(decide_new_window("https://example.com/"), OpenExternal);
        assert_eq!(decide_new_window("http://localhost:3000"), OpenExternal);
        for u in [
            "http://example.com",
            "javascript:void(0)",
            "file:///x",
            "intent://x",
            "slack://open",
            "https://u:p@example.com",
            "about:blank",
        ] {
            assert_eq!(decide_new_window(u), Block, "{u}");
        }
    }
}
