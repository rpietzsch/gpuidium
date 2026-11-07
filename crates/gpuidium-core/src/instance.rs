//! Single-instance lock (CORE-009): prevents two processes from sharing
//! `config.json` and profile stores. The OS releases the lock if we crash.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;

use fd_lock::RwLock;

#[derive(Debug, thiserror::Error)]
pub enum LockError {
    #[error("another GPUIdium instance is already running")]
    AlreadyRunning,
    #[error("cannot create lock file: {0}")]
    Io(#[from] io::Error),
}

/// Holds the lock for as long as it lives.
pub struct InstanceLock {
    // Leaked on purpose: the guard borrows the RwLock, and the lock must live
    // for the whole process. The OS frees the file lock at exit.
    _guard: fd_lock::RwLockWriteGuard<'static, File>,
}

impl InstanceLock {
    pub fn acquire(dir: &Path) -> Result<Self, LockError> {
        fs::create_dir_all(dir)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(dir.join("instance.lock"))?;
        let lock: &'static mut RwLock<File> = Box::leak(Box::new(RwLock::new(file)));
        match lock.try_write() {
            Ok(guard) => Ok(Self { _guard: guard }),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Err(LockError::AlreadyRunning),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_in_same_dir_fails() {
        let d = tempfile::tempdir().unwrap();
        let first = InstanceLock::acquire(d.path()).unwrap();
        assert!(matches!(
            InstanceLock::acquire(d.path()),
            Err(LockError::AlreadyRunning)
        ));
        drop(first);
    }

    #[test]
    fn different_dirs_do_not_conflict() {
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let _x = InstanceLock::acquire(a.path()).unwrap();
        let _y = InstanceLock::acquire(b.path()).unwrap();
    }
}
