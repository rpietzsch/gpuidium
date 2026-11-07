//! OS-standard per-user locations. Never inside the install directory.

use std::path::PathBuf;

use crate::model::ProfileId;

const APP_DIR: &str = "GPUIdium";

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn home() -> Option<PathBuf> {
    env_path("HOME").or_else(|| env_path("USERPROFILE"))
}

/// Directory holding `config.json`.
pub fn config_dir() -> Option<PathBuf> {
    let base = if cfg!(target_os = "windows") {
        env_path("APPDATA")?
    } else if cfg!(target_os = "macos") {
        home()?.join("Library").join("Application Support")
    } else {
        env_path("XDG_CONFIG_HOME").or_else(|| home().map(|h| h.join(".config")))?
    };
    Some(base.join(APP_DIR))
}

/// Directory holding engine-owned browser profile data (kept apart from config).
pub fn data_dir() -> Option<PathBuf> {
    let base = if cfg!(target_os = "windows") {
        env_path("LOCALAPPDATA")?
    } else if cfg!(target_os = "macos") {
        home()?.join("Library").join("Application Support")
    } else {
        env_path("XDG_DATA_HOME").or_else(|| home().map(|h| h.join(".local").join("share")))?
    };
    Some(base.join(APP_DIR))
}

/// Cached favicon (PNG) for one app, outside the browser profile and named by app id only.
pub fn favicon_file(data_dir: &std::path::Path, id: crate::model::AppId) -> PathBuf {
    data_dir.join("favicons").join(format!("{id}.png"))
}

/// Per-profile data directory. Named only by the opaque profile id, never by URL.
pub fn profile_dir(data_dir: &std::path::Path, id: ProfileId) -> PathBuf {
    data_dir.join("profiles").join(id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AppId;

    #[test]
    fn favicon_file_is_keyed_by_app_id_only() {
        let id = AppId::new();
        let f = favicon_file(std::path::Path::new("/d"), id);
        assert_eq!(
            f,
            std::path::Path::new("/d/favicons").join(format!("{id}.png"))
        );
    }
}
