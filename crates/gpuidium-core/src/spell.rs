//! Spell-checking languages on Linux: WebKitGTK checks against several hunspell
//! dictionaries at once (e.g. `en_US` and `de_DE`), so the Manage panel offers the ones that
//! are installed. macOS uses the system spell checker instead (automatic per-word language).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Where distributions install hunspell dictionaries (`<lang>.dic` + `<lang>.aff`).
pub fn dictionary_dirs() -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = [
        "/usr/share/hunspell",
        "/usr/share/myspell",
        "/usr/share/myspell/dicts",
        "/app/share/hunspell", // Flatpak extensions
    ]
    .into_iter()
    .map(PathBuf::from)
    .collect();
    if let Some(home) = std::env::var_os("HOME") {
        dirs.push(Path::new(&home).join(".local/share/hunspell"));
    }
    dirs
}

/// Language codes of the complete dictionaries (both `.dic` and `.aff`) in `dirs`, sorted.
pub fn installed_dictionaries(dirs: &[PathBuf]) -> Vec<String> {
    let mut out = BTreeSet::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("dic") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if is_language_code(stem) && path.with_extension("aff").is_file() {
                out.insert(stem.to_string());
            }
        }
    }
    out.into_iter().collect()
}

/// `en`, `en_US`, `de_DE`, `pt_BR`, `sr_Latn_RS`: letters and underscores, no hyphenation
/// or thesaurus files (`hyph_de_DE`, `th_en_US_v2`).
fn is_language_code(s: &str) -> bool {
    let mut parts = s.split('_');
    let lang = parts.next().unwrap_or("");
    (2..=3).contains(&lang.len())
        && lang.bytes().all(|b| b.is_ascii_lowercase())
        && parts.all(|p| (2..=4).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// The user's locale as a dictionary code (`LANG=de_DE.UTF-8` -> `de_DE`), used when no
/// languages are chosen.
pub fn locale_language(lang_env: &str) -> Option<String> {
    let code = lang_env.split(['.', '@']).next()?.trim();
    (is_language_code(code) && code != "C" && code != "POSIX").then(|| code.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_complete_dictionaries_only() {
        let d = tempfile::tempdir().unwrap();
        for f in [
            "de_DE.dic",
            "de_DE.aff",
            "en_US.dic",
            "en_US.aff",
            "fr_FR.dic", // no .aff
            "hyph_de_DE.dic",
            "hyph_de_DE.aff",
            "README.txt",
        ] {
            std::fs::write(d.path().join(f), "").unwrap();
        }
        let missing = d.path().join("nope");
        assert_eq!(
            installed_dictionaries(&[d.path().to_path_buf(), missing]),
            vec!["de_DE".to_string(), "en_US".to_string()]
        );
    }

    #[test]
    fn locale_to_dictionary_code() {
        assert_eq!(locale_language("de_DE.UTF-8").as_deref(), Some("de_DE"));
        assert_eq!(locale_language("en_US").as_deref(), Some("en_US"));
        assert_eq!(locale_language("sr_RS@latin").as_deref(), Some("sr_RS"));
        assert_eq!(locale_language("C.UTF-8"), None);
        assert_eq!(locale_language(""), None);
    }
}
