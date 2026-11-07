//! Export/import of the app list without any profile or session data (PROFILE-010).
//!
//! The export contains only display name and start URL (plus order). Profile ids and app ids
//! are never exported; importing allocates fresh ones via `Command::Add`, so an import can
//! never attach an existing browser profile.

use serde::{Deserialize, Serialize};

use crate::model::AppEntry;
use crate::uri::normalize_input;

const FORMAT: &str = "gpuidium-apps";
const MAX_BYTES: usize = 1 << 20;
const MAX_APPS: usize = 500;

#[derive(Debug, Serialize, Deserialize)]
struct Export {
    format: String,
    version: u32,
    apps: Vec<ExportApp>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExportApp {
    name: String,
    start_url: String,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ExchangeError {
    #[error("file is too large")]
    TooLarge,
    #[error("not a GPUIdium export: {0}")]
    Invalid(String),
    #[error("too many apps (max {MAX_APPS})")]
    TooMany,
}

/// One importable entry, URL already validated and normalised.
#[derive(Debug, PartialEq, Eq)]
pub struct ImportItem {
    pub name: String,
    pub url: String,
}

pub fn export(apps: &[AppEntry]) -> String {
    let e = Export {
        format: FORMAT.into(),
        version: 1,
        apps: apps
            .iter()
            .map(|a| ExportApp {
                name: a.name.clone(),
                start_url: a.start_url.clone(),
            })
            .collect(),
    };
    serde_json::to_string_pretty(&e).expect("serialisable")
}

/// Parse an export. Entries with unacceptable URLs are skipped and counted, not fatal.
pub fn import(bytes: &[u8]) -> Result<(Vec<ImportItem>, usize), ExchangeError> {
    if bytes.len() > MAX_BYTES {
        return Err(ExchangeError::TooLarge);
    }
    let e: Export =
        serde_json::from_slice(bytes).map_err(|e| ExchangeError::Invalid(e.to_string()))?;
    if e.format != FORMAT || e.version != 1 {
        return Err(ExchangeError::Invalid("unknown format or version".into()));
    }
    if e.apps.len() > MAX_APPS {
        return Err(ExchangeError::TooMany);
    }
    let mut items = Vec::new();
    let mut skipped = 0;
    for a in e.apps {
        match normalize_input(&a.start_url) {
            Ok(u) => items.push(ImportItem {
                name: a.name,
                url: u.as_str().to_string(),
            }),
            Err(_) => skipped += 1,
        }
    }
    Ok((items, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AppId, ProfileId};

    fn entry(name: &str, url: &str) -> AppEntry {
        AppEntry {
            id: AppId::new(),
            name: name.into(),
            start_url: url.into(),
            profile_id: ProfileId::new(),
            order: 0,
        }
    }

    #[test]
    fn export_contains_no_ids() {
        let a = entry("Work", "https://example.org/");
        let json = export(std::slice::from_ref(&a));
        assert!(!json.contains(&a.id.to_string()));
        assert!(!json.contains(&a.profile_id.to_string()));
        assert!(!json.contains("profile"));
    }

    #[test]
    fn roundtrip_and_skips_dangerous_urls() {
        let json = export(&[
            entry("A", "https://example.org/"),
            entry("B", "https://example.com/"),
        ]);
        let (items, skipped) = import(json.as_bytes()).unwrap();
        assert_eq!((items.len(), skipped), (2, 0));
        let evil = br#"{"format":"gpuidium-apps","version":1,"apps":[
            {"name":"x","start_url":"javascript:alert(1)"},
            {"name":"y","start_url":"file:///etc/passwd"},
            {"name":"z","start_url":"https://ok.example/"}]}"#;
        let (items, skipped) = import(evil).unwrap();
        assert_eq!((items.len(), skipped), (1, 2));
    }

    #[test]
    fn rejects_garbage_and_oversize() {
        assert!(matches!(import(b"{}"), Err(ExchangeError::Invalid(_))));
        assert!(matches!(import(b"nope"), Err(ExchangeError::Invalid(_))));
        assert_eq!(
            import(&vec![b' '; MAX_BYTES + 1]),
            Err(ExchangeError::TooLarge)
        );
    }
}
