//! Opt-in local diagnostics with secret redaction (CORE-009).
//!
//! Nothing is logged unless `GPUIDIUM_LOG` is set. URLs are reduced to
//! `scheme://host[:port]/path` - no userinfo, query or fragment - and local data
//! paths are never passed in.

use std::sync::OnceLock;
use std::time::Instant;

use url::Url;

static START: OnceLock<Instant> = OnceLock::new();

/// Start the clock used for log timestamps (call first thing in `main`).
pub fn init_clock() {
    START.get_or_init(Instant::now);
}

/// Reduce a URL to a form safe for logs.
pub fn redact_url(raw: &str) -> String {
    match Url::parse(raw) {
        Ok(u) => {
            let host = u.host_str().unwrap_or("");
            let port = u.port().map(|p| format!(":{p}")).unwrap_or_default();
            format!("{}://{host}{port}{}", u.scheme(), u.path())
        }
        // Unparseable input could be anything (including a secret): drop it.
        Err(_) => "<unparseable-url>".to_owned(),
    }
}

pub fn enabled() -> bool {
    std::env::var_os("GPUIDIUM_LOG").is_some_and(|v| !v.is_empty())
}

/// Log a line to stderr if diagnostics are enabled.
pub fn log(msg: &str) {
    if enabled() {
        let ms = START.get_or_init(Instant::now).elapsed().as_millis();
        eprintln!("[gpuidium +{ms}ms] {msg}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_query_fragment_and_userinfo() {
        assert_eq!(
            redact_url("https://user:pw@example.com:8443/a/b?token=SECRET#frag"),
            "https://example.com:8443/a/b"
        );
    }

    #[test]
    fn drops_unparseable() {
        assert_eq!(redact_url("token=SECRET"), "<unparseable-url>");
    }
}
