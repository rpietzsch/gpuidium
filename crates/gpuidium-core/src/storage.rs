//! Config schema v1 and crash-safe persistence (CORE-003).
//!
//! `config.json` holds only app metadata. Cookies, tokens and other browsing
//! data live in engine-owned profile directories and never in this file.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{AppEntry, AppId, AppState};
use crate::uri::normalize_input;

pub const SCHEMA_VERSION: u32 = 1;
pub const CONFIG_FILE: &str = "config.json";
pub const BACKUP_FILE: &str = "config.json.bak";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowConfig {
    pub width: u32,
    pub height: u32,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            width: 1200,
            height: 800,
        }
    }
}

/// User options (additive, so schema stays at v1). Page notifications are per app
/// (`Config::notifications_off`); the old global `notifications` key is ignored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// LIFE-010: close the native view of apps inactive for this many minutes.
    /// Persistent cookies/storage survive; in-page state does not.
    pub unload_inactive_minutes: Option<u32>,
    /// Spell checking in page text fields (on by default).
    pub spellcheck: bool,
    /// Spell-check languages (e.g. `en_US`, `de_DE`), all used at once. Empty: the system's
    /// default. Linux only; macOS detects the language among the system's spelling languages.
    pub spell_languages: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            unload_inactive_minutes: None,
            spellcheck: true,
            spell_languages: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub schema_version: u32,
    #[serde(default)]
    pub apps: Vec<AppEntry>,
    #[serde(default)]
    pub active_app_id: Option<AppId>,
    #[serde(default)]
    pub window: WindowConfig,
    #[serde(default)]
    pub settings: Settings,
    /// Apps whose page notifications are switched off (notifications are on by default).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub notifications_off: BTreeSet<AppId>,
    /// Per-app page zoom in percent; apps at 100 % are not listed (additive, schema v1).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub zoom: BTreeMap<AppId, u16>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            apps: Vec::new(),
            active_app_id: None,
            window: WindowConfig::default(),
            settings: Settings::default(),
            notifications_off: BTreeSet::new(),
            zoom: BTreeMap::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("io error on {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("config is invalid: {0}")]
    Invalid(String),
    #[error("config schema {found} is newer than supported {supported}; refusing to overwrite")]
    TooNew { found: u32, supported: u32 },
}

fn io<'a>(path: &'a Path) -> impl FnOnce(std::io::Error) -> StorageError + 'a {
    move |source| StorageError::Io {
        path: path.to_owned(),
        source,
    }
}

impl Config {
    pub fn from_state(state: &AppState, window: WindowConfig) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            apps: state.apps().to_vec(),
            active_app_id: state.active(),
            window,
            settings: Settings::default(),
            notifications_off: BTreeSet::new(),
            zoom: BTreeMap::new(),
        }
    }

    pub fn to_state(&self) -> AppState {
        AppState::from_parts(self.apps.clone(), self.active_app_id)
    }

    /// Enforce invariants: unique app ids, **unique profile ids (no aliasing)**,
    /// start URLs that pass the current URL policy.
    pub fn validate(&self) -> Result<(), StorageError> {
        let mut ids = HashSet::new();
        let mut profiles = HashSet::new();
        for a in &self.apps {
            if !ids.insert(a.id) {
                return Err(StorageError::Invalid(format!("duplicate app id {}", a.id)));
            }
            if !profiles.insert(a.profile_id) {
                return Err(StorageError::Invalid(format!(
                    "profile id {} used by more than one app",
                    a.profile_id
                )));
            }
            normalize_input(&a.start_url)
                .map_err(|e| StorageError::Invalid(format!("app {}: {e}", a.id)))?;
        }
        Ok(())
    }

    /// Parse and migrate. Older schemas are migrated here; newer are refused.
    pub fn parse(bytes: &[u8]) -> Result<Self, StorageError> {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| StorageError::Invalid(e.to_string()))?;
        let found = value
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| StorageError::Invalid("missing schema_version".into()))?
            as u32;
        if found > SCHEMA_VERSION {
            return Err(StorageError::TooNew {
                found,
                supported: SCHEMA_VERSION,
            });
        }
        // Future: `while v < SCHEMA_VERSION { value = migrate(v, value) }`.
        let cfg: Config =
            serde_json::from_value(value).map_err(|e| StorageError::Invalid(e.to_string()))?;
        cfg.validate()?;
        Ok(cfg)
    }
}

/// How `load` obtained its config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovery {
    /// Loaded normally, or no config existed yet.
    None,
    /// Main file was unusable; last-known-good backup was used.
    RestoredBackup { corrupt_preserved_as: PathBuf },
    /// Neither file was usable; started empty. Originals are preserved.
    StartedEmpty { corrupt_preserved_as: PathBuf },
}

#[derive(Debug)]
pub struct Loaded {
    pub config: Config,
    pub recovery: Recovery,
}

/// Write atomically: temp file + fsync + rename, keeping the previous file as backup.
pub fn save(dir: &Path, config: &Config) -> Result<(), StorageError> {
    config.validate()?;
    fs::create_dir_all(dir).map_err(io(dir))?;
    let main = dir.join(CONFIG_FILE);
    let tmp = dir.join(format!("{CONFIG_FILE}.tmp"));
    let bak = dir.join(BACKUP_FILE);

    let json =
        serde_json::to_vec_pretty(config).map_err(|e| StorageError::Invalid(e.to_string()))?;
    {
        let mut f = File::create(&tmp).map_err(io(&tmp))?;
        f.write_all(&json).map_err(io(&tmp))?;
        f.write_all(b"\n").map_err(io(&tmp))?;
        f.sync_all().map_err(io(&tmp))?;
    }
    // Only back up a main file that is itself valid, so a corrupt file never
    // overwrites the last-known-good backup.
    if let Ok(existing) = fs::read(&main)
        && Config::parse(&existing).is_ok()
    {
        fs::write(&bak, existing).map_err(io(&bak))?;
    }
    fs::rename(&tmp, &main).map_err(io(&main))?;
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all(); // best effort; not supported on every platform
    }
    Ok(())
}

fn preserve_corrupt(path: &Path) -> Result<PathBuf, StorageError> {
    let mut n = 0u32;
    loop {
        let candidate = path.with_extension(format!("corrupt-{n}"));
        if !candidate.exists() {
            fs::rename(path, &candidate).map_err(io(path))?;
            return Ok(candidate);
        }
        n += 1;
    }
}

/// Load config, recovering from corruption without ever deleting data.
pub fn load(dir: &Path) -> Result<Loaded, StorageError> {
    let main = dir.join(CONFIG_FILE);
    let bak = dir.join(BACKUP_FILE);

    let bytes = match fs::read(&main) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Loaded {
                config: Config::default(),
                recovery: Recovery::None,
            });
        }
        Err(e) => return Err(io(&main)(e)),
    };

    match Config::parse(&bytes) {
        Ok(config) => Ok(Loaded {
            config,
            recovery: Recovery::None,
        }),
        // A newer app wrote this file: do not touch or "recover" it.
        Err(e @ StorageError::TooNew { .. }) => Err(e),
        Err(_) => {
            let preserved = preserve_corrupt(&main)?;
            if let Ok(b) = fs::read(&bak)
                && let Ok(config) = Config::parse(&b)
            {
                return Ok(Loaded {
                    config,
                    recovery: Recovery::RestoredBackup {
                        corrupt_preserved_as: preserved,
                    },
                });
            }
            Ok(Loaded {
                config: Config::default(),
                recovery: Recovery::StartedEmpty {
                    corrupt_preserved_as: preserved,
                },
            })
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn settings_defaults_and_old_configs_still_load() {
        let old = br#"{"schema_version":1,"apps":[],"active_app_id":null}"#;
        let c = super::Config::parse(old).unwrap();
        assert_eq!(c.settings, super::Settings::default());
        assert!(c.settings.unload_inactive_minutes.is_none());
        // Spell checking defaults on; notifications default on (nobody is in the off list).
        assert!(c.settings.spellcheck && c.settings.spell_languages.is_empty());
        assert!(c.notifications_off.is_empty());
        // The pre-0.4 global notification switch is ignored, not an error.
        let older = br#"{"schema_version":1,"apps":[],"settings":{"notifications":false}}"#;
        assert!(super::Config::parse(older).unwrap().settings.spellcheck);
    }

    use super::*;
    use crate::model::{Command, ProfileId};

    fn state_with(n: usize) -> AppState {
        let mut s = AppState::new();
        for i in 0..n {
            s.apply(Command::Add {
                name: format!("A{i}"),
                input: "chat.example.org".into(),
            })
            .unwrap();
        }
        s
    }

    #[test]
    fn missing_config_gives_default() {
        let d = tempfile::tempdir().unwrap();
        let l = load(d.path()).unwrap();
        assert_eq!(l.config, Config::default());
        assert_eq!(l.recovery, Recovery::None);
    }

    #[test]
    fn roundtrip_keeps_profiles_order_active() {
        let d = tempfile::tempdir().unwrap();
        let s = state_with(3); // includes duplicate URLs
        let cfg = Config::from_state(
            &s,
            WindowConfig {
                width: 900,
                height: 700,
            },
        );
        save(d.path(), &cfg).unwrap();
        let l = load(d.path()).unwrap();
        assert_eq!(l.config, cfg);
        assert_eq!(l.config.to_state(), s);
    }

    #[test]
    fn no_secrets_shape_only_known_fields() {
        let d = tempfile::tempdir().unwrap();
        save(
            d.path(),
            &Config::from_state(&state_with(1), WindowConfig::default()),
        )
        .unwrap();
        let text = fs::read_to_string(d.path().join(CONFIG_FILE)).unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let app = &v["apps"][0];
        let mut keys: Vec<_> = app.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["id", "name", "order", "profile_id", "start_url"]);
    }

    #[test]
    fn second_save_creates_backup_of_previous() {
        let d = tempfile::tempdir().unwrap();
        let c1 = Config::from_state(&state_with(1), WindowConfig::default());
        let c2 = Config::from_state(&state_with(2), WindowConfig::default());
        save(d.path(), &c1).unwrap();
        save(d.path(), &c2).unwrap();
        let bak = Config::parse(&fs::read(d.path().join(BACKUP_FILE)).unwrap()).unwrap();
        assert_eq!(bak, c1);
        assert!(!d.path().join("config.json.tmp").exists());
    }

    #[test]
    fn corrupt_main_restores_backup_and_preserves_original() {
        let d = tempfile::tempdir().unwrap();
        let c1 = Config::from_state(&state_with(1), WindowConfig::default());
        let c2 = Config::from_state(&state_with(2), WindowConfig::default());
        save(d.path(), &c1).unwrap();
        save(d.path(), &c2).unwrap();
        fs::write(d.path().join(CONFIG_FILE), b"{ not json").unwrap();
        let l = load(d.path()).unwrap();
        assert_eq!(l.config, c1);
        let Recovery::RestoredBackup {
            corrupt_preserved_as,
        } = l.recovery
        else {
            panic!("expected backup restore");
        };
        assert_eq!(fs::read(corrupt_preserved_as).unwrap(), b"{ not json");
    }

    #[test]
    fn corrupt_everything_starts_empty_without_deleting() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join(CONFIG_FILE), b"garbage").unwrap();
        let l = load(d.path()).unwrap();
        assert_eq!(l.config, Config::default());
        let Recovery::StartedEmpty {
            corrupt_preserved_as,
        } = l.recovery
        else {
            panic!("expected empty start");
        };
        assert_eq!(fs::read(corrupt_preserved_as).unwrap(), b"garbage");
    }

    #[test]
    fn corrupt_main_never_overwrites_good_backup() {
        let d = tempfile::tempdir().unwrap();
        let good = Config::from_state(&state_with(1), WindowConfig::default());
        save(d.path(), &good).unwrap();
        save(d.path(), &good).unwrap(); // now .bak == good
        fs::write(d.path().join(CONFIG_FILE), b"junk").unwrap();
        save(d.path(), &Config::default()).unwrap(); // junk must not become the backup
        let bak = Config::parse(&fs::read(d.path().join(BACKUP_FILE)).unwrap()).unwrap();
        assert_eq!(bak, good);
    }

    #[test]
    fn rejects_aliased_profile_ids() {
        let mut cfg = Config::from_state(&state_with(2), WindowConfig::default());
        cfg.apps[1].profile_id = cfg.apps[0].profile_id;
        assert!(matches!(cfg.validate(), Err(StorageError::Invalid(_))));
        let d = tempfile::tempdir().unwrap();
        assert!(save(d.path(), &cfg).is_err());
    }

    #[test]
    fn rejects_duplicate_app_ids_and_bad_urls() {
        let mut cfg = Config::from_state(&state_with(2), WindowConfig::default());
        cfg.apps[1].id = cfg.apps[0].id;
        assert!(cfg.validate().is_err());
        let mut cfg = Config::from_state(&state_with(1), WindowConfig::default());
        cfg.apps[0].start_url = "javascript:alert(1)".into();
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn newer_schema_is_refused_and_left_untouched() {
        let d = tempfile::tempdir().unwrap();
        let body = br#"{"schema_version": 99, "apps": []}"#;
        fs::write(d.path().join(CONFIG_FILE), body).unwrap();
        assert!(matches!(
            load(d.path()),
            Err(StorageError::TooNew { found: 99, .. })
        ));
        assert_eq!(fs::read(d.path().join(CONFIG_FILE)).unwrap(), body);
    }

    #[test]
    fn malicious_config_is_just_data() {
        let d = tempfile::tempdir().unwrap();
        let id = ProfileId::new();
        let body = format!(
            r#"{{"schema_version":1,"apps":[{{"id":"{}","name":"x","start_url":"file:///etc/passwd","profile_id":"{}","order":0}}]}}"#,
            AppId::new(),
            id
        );
        fs::write(d.path().join(CONFIG_FILE), body).unwrap();
        let l = load(d.path()).unwrap(); // rejected -> recovered empty, original kept
        assert!(l.config.apps.is_empty());
        assert!(matches!(l.recovery, Recovery::StartedEmpty { .. }));
    }

    #[test]
    fn zoom_roundtrips_and_is_optional() {
        let d = tempfile::tempdir().unwrap();
        let mut cfg = Config::from_state(&state_with(2), WindowConfig::default());
        let id = cfg.apps[0].id;
        cfg.zoom.insert(id, 120);
        save(d.path(), &cfg).unwrap();
        assert_eq!(load(d.path()).unwrap().config.zoom.get(&id), Some(&120));

        // Configs written before zoom existed still load, and no zoom means no key.
        let plain = Config::from_state(&state_with(1), WindowConfig::default());
        let json = serde_json::to_string(&plain).unwrap();
        assert!(!json.contains("zoom"));
        assert!(Config::parse(json.as_bytes()).unwrap().zoom.is_empty());
    }
}
