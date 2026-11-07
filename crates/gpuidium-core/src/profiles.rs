//! Profile directory lifecycle (PROFILE-001, PROFILE-003).
//!
//! One profile directory per `ProfileId`, named only by that opaque id, under
//! `<data_dir>/profiles/`. Deletion is scoped to exactly one such directory.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::model::ProfileId;
use crate::paths::profile_dir;

/// Create (if needed) and return the directory for a profile.
pub fn ensure(data_dir: &Path, id: ProfileId) -> io::Result<PathBuf> {
    let dir = profile_dir(data_dir, id);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Delete exactly one profile's data. The caller must have closed its WebView.
/// Refuses anything that is not `<data_dir>/profiles/<id>` and never follows a
/// symlink out of the profiles root.
pub fn delete(data_dir: &Path, id: ProfileId) -> io::Result<()> {
    let dir = profile_dir(data_dir, id);
    match fs::symlink_metadata(&dir) {
        Ok(m) if m.file_type().is_symlink() => {
            // Remove the link itself, never its target.
            fs::remove_file(&dir)
        }
        Ok(_) => {
            let root = fs::canonicalize(data_dir.join("profiles"))?;
            let real = fs::canonicalize(&dir)?;
            if real.parent() != Some(root.as_path()) {
                return Err(io::Error::other("profile path escapes profiles root"));
            }
            fs::remove_dir_all(real)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Profiles on disk that no app references (retained after "remove, keep data").
pub fn orphans(data_dir: &Path, known: &[ProfileId]) -> io::Result<Vec<ProfileId>> {
    let known: HashSet<_> = known.iter().copied().collect();
    let root = data_dir.join("profiles");
    let mut out = Vec::new();
    let entries = match fs::read_dir(&root) {
        Ok(e) => e,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        if let Some(id) = entry.file_name().to_str().and_then(ProfileId::parse)
            && !known.contains(&id)
        {
            out.push(id);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_creates_distinct_dirs_named_by_id() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (ProfileId::new(), ProfileId::new());
        let (pa, pb) = (ensure(d.path(), a).unwrap(), ensure(d.path(), b).unwrap());
        assert_ne!(pa, pb);
        assert!(pa.ends_with(a.to_string()) && pa.is_dir());
    }

    #[test]
    fn delete_removes_only_selected_profile() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (ProfileId::new(), ProfileId::new());
        let (pa, pb) = (ensure(d.path(), a).unwrap(), ensure(d.path(), b).unwrap());
        fs::write(pa.join("cookies"), "x").unwrap();
        fs::write(pb.join("cookies"), "y").unwrap();
        delete(d.path(), a).unwrap();
        assert!(!pa.exists());
        assert_eq!(fs::read(pb.join("cookies")).unwrap(), b"y");
    }

    #[test]
    fn delete_missing_is_ok() {
        let d = tempfile::tempdir().unwrap();
        delete(d.path(), ProfileId::new()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn delete_never_follows_symlink_out_of_root() {
        let d = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("precious"), "keep").unwrap();
        let id = ProfileId::new();
        fs::create_dir_all(d.path().join("profiles")).unwrap();
        std::os::unix::fs::symlink(outside.path(), profile_dir(d.path(), id)).unwrap();
        delete(d.path(), id).unwrap();
        assert!(outside.path().join("precious").exists());
    }

    #[test]
    fn orphans_lists_unreferenced_profiles_only() {
        let d = tempfile::tempdir().unwrap();
        let (kept, gone) = (ProfileId::new(), ProfileId::new());
        ensure(d.path(), kept).unwrap();
        ensure(d.path(), gone).unwrap();
        fs::create_dir_all(d.path().join("profiles").join("not-a-profile")).unwrap();
        assert_eq!(orphans(d.path(), &[kept]).unwrap(), vec![gone]);
        assert!(orphans(&d.path().join("nope"), &[]).unwrap().is_empty());
    }
}
