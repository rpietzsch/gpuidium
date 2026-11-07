//! IRI/URL parsing and safe normalization (CORE-002).
//!
//! Accepts `https://` addresses and bare hostnames (defaulting to HTTPS).
//! Plain `http://` is accepted only for loopback hosts. Everything else,
//! including URLs with userinfo, is rejected.

use url::{Host, Url};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UriError {
    #[error("address is empty")]
    Empty,
    #[error("address is not a valid URL")]
    Invalid,
    #[error("scheme `{0}` is not allowed")]
    UnsupportedScheme(String),
    #[error("plain http is only allowed for localhost, 127.0.0.1 and [::1]")]
    InsecureHttp,
    #[error("addresses with embedded credentials are not allowed")]
    UserInfo,
    #[error("address has no host")]
    NoHost,
}

/// A validated, normalized start URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedUrl {
    url: Url,
}

impl NormalizedUrl {
    pub fn url(&self) -> &Url {
        &self.url
    }

    pub fn as_str(&self) -> &str {
        self.url.as_str()
    }

    /// Canonical origin (scheme, IDNA/punycode host, port) for display.
    pub fn origin(&self) -> String {
        self.url.origin().ascii_serialization()
    }

    /// Human-friendly form with Unicode host decoded, when possible.
    pub fn display(&self) -> String {
        let Some(host) = self.url.host_str() else {
            return self.url.to_string();
        };
        let (unicode, _) = idna::domain_to_unicode(host);
        if unicode == host {
            return self.url.to_string();
        }
        self.url.to_string().replacen(host, &unicode, 1)
    }

    pub fn is_loopback_http(&self) -> bool {
        self.url.scheme() == "http"
    }
}

const BLOCKED_SCHEMES: &[&str] = &[
    "javascript",
    "data",
    "about",
    "file",
    "chrome",
    "edge",
    "ftp",
    "blob",
    "mailto",
    "view-source",
    "vbscript",
    "ws",
    "wss",
];

fn is_loopback(host: &Host<&str>) -> bool {
    match host {
        Host::Domain(d) => d.eq_ignore_ascii_case("localhost"),
        Host::Ipv4(ip) => ip.is_loopback(),
        Host::Ipv6(ip) => ip.is_loopback(),
    }
}

/// Parse and normalize user input into a URL safe to load as an app's start page.
pub fn normalize_input(input: &str) -> Result<NormalizedUrl, UriError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(UriError::Empty);
    }

    let candidate = if input.contains("://") {
        input.to_owned()
    } else {
        // `javascript:alert(1)`, `data:...`, `mailto:x` have no `://`.
        if let Some((scheme, _)) = input.split_once(':') {
            let scheme = scheme.to_ascii_lowercase();
            if BLOCKED_SCHEMES.contains(&scheme.as_str()) {
                return Err(UriError::UnsupportedScheme(scheme));
            }
        }
        format!("https://{input}")
    };

    let url = Url::parse(&candidate).map_err(|_| UriError::Invalid)?;

    match url.scheme() {
        "https" => {}
        "http" => {
            let loopback = url.host().as_ref().is_some_and(is_loopback);
            if !loopback {
                return Err(UriError::InsecureHttp);
            }
        }
        other => return Err(UriError::UnsupportedScheme(other.to_owned())),
    }

    if !url.username().is_empty() || url.password().is_some() {
        return Err(UriError::UserInfo);
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err(UriError::NoHost);
    }

    Ok(NormalizedUrl { url })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(s: &str) -> NormalizedUrl {
        normalize_input(s).unwrap_or_else(|e| panic!("{s}: {e}"))
    }

    #[test]
    fn bare_host_defaults_to_https() {
        assert_eq!(ok("example.com").as_str(), "https://example.com/");
        assert_eq!(
            ok("  example.com/a?b=1#c ").as_str(),
            "https://example.com/a?b=1#c"
        );
    }

    #[test]
    fn keeps_path_query_fragment() {
        assert_eq!(
            ok("https://chat.example.org/work?x=1#top").as_str(),
            "https://chat.example.org/work?x=1#top"
        );
    }

    #[test]
    fn unicode_host_and_path() {
        let u = ok("https://bücher.example/über");
        assert_eq!(u.as_str(), "https://xn--bcher-kva.example/%C3%BCber");
        assert_eq!(u.origin(), "https://xn--bcher-kva.example");
        assert!(u.display().contains("bücher.example"));
        assert_eq!(
            ok("bücher.example").origin(),
            "https://xn--bcher-kva.example"
        );
    }

    #[test]
    fn http_only_for_loopback() {
        assert!(ok("http://localhost:3000").is_loopback_http());
        assert!(ok("http://127.0.0.1/").is_loopback_http());
        assert!(ok("http://[::1]:8080/").is_loopback_http());
        assert_eq!(
            normalize_input("http://example.com"),
            Err(UriError::InsecureHttp)
        );
        assert_eq!(
            normalize_input("http://10.0.0.1"),
            Err(UriError::InsecureHttp)
        );
    }

    #[test]
    fn bare_host_with_port() {
        assert_eq!(ok("example.com:8443").as_str(), "https://example.com:8443/");
    }

    #[test]
    fn rejects_dangerous_schemes() {
        for s in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "data:text/html,<b>x</b>",
            "about:blank",
            "file:///etc/passwd",
            "chrome://settings",
            "edge://flags",
            "ftp://example.com",
            "mailto:a@b.c",
            "blob:https://x/y",
            "custom://thing",
        ] {
            assert!(
                matches!(normalize_input(s), Err(UriError::UnsupportedScheme(_))),
                "{s} should be rejected"
            );
        }
    }

    #[test]
    fn rejects_userinfo() {
        assert_eq!(
            normalize_input("https://user:pw@example.com"),
            Err(UriError::UserInfo)
        );
        assert_eq!(
            normalize_input("https://user@example.com"),
            Err(UriError::UserInfo)
        );
    }

    #[test]
    fn rejects_empty_and_garbage() {
        assert_eq!(normalize_input("   "), Err(UriError::Empty));
        assert!(normalize_input("https://").is_err());
        assert!(normalize_input("https://exa mple.com").is_err());
        assert!(normalize_input("foo:bar").is_err());
    }

    /// Cheap fuzz-style sweep: nothing may panic, and anything accepted is safe.
    #[test]
    fn arbitrary_inputs_never_panic_and_accepted_are_safe() {
        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let alphabet: Vec<char> = "abcXYZ019:/?#@[]%.\\ \t\u{fc}\u{202e}-_&="
            .chars()
            .collect();
        for _ in 0..20_000 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let len = (seed >> 60) as usize + 1;
            let s: String = (0..len)
                .map(|i| alphabet[((seed >> (i * 3)) as usize ^ i) % alphabet.len()])
                .collect();
            if let Ok(n) = normalize_input(&s) {
                assert!(matches!(n.url().scheme(), "https" | "http"));
                assert!(n.url().username().is_empty() && n.url().password().is_none());
                assert!(n.url().host_str().is_some());
            }
        }
    }
}
