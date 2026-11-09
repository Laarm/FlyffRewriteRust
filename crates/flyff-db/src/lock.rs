//! Single-instance lock `data/flyff.db.lock` (Plan/13 §2.1, 10 §3.3).
//!
//! The server holds an exclusive `flock` on `<db file>.lock` for its whole lifetime. A second server, and
//! every CLI command that writes (`db restore`, `db migrate`, `db vacuum`, character undelete), refuses
//! while it is held (Plan/13 §2.3). The lock is released when the [`InstanceLock`] is dropped or the process
//! dies, so a crash never leaves a stale lock.

use crate::error::{DbError, DbErrorKind};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

/// An exclusive advisory lock on `<db file>.lock`.
#[derive(Debug)]
pub struct InstanceLock {
    // Dropping the file releases the flock.
    file: File,
    lock_path: PathBuf,
    db_path: PathBuf,
}

impl InstanceLock {
    /// The lock file belonging to `db_path` (`data/flyff.db` -> `data/flyff.db.lock`).
    pub fn lock_path_for(db_path: &Path) -> PathBuf {
        let mut name = db_path
            .file_name()
            .map(std::ffi::OsString::from)
            .unwrap_or_default();
        name.push(".lock");
        db_path.with_file_name(name)
    }

    /// Takes the lock for the database at `db_path`, creating the lock file (and nothing else) when missing.
    /// The parent directory must exist.
    ///
    /// Fails with [`DbErrorKind::Locked`] ("another flyff instance is using ...") when another process, or
    /// another [`InstanceLock`] of this process, holds it.
    pub fn acquire(db_path: &Path) -> Result<InstanceLock, DbError> {
        let lock_path = Self::lock_path_for(db_path);
        let file = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)?;
        match file.try_lock() {
            Ok(()) => {}
            Err(std::fs::TryLockError::WouldBlock) => {
                let holder = std::fs::read_to_string(&lock_path).unwrap_or_default();
                return Err(DbError::new(
                    DbErrorKind::Locked,
                    format!(
                        "another flyff instance is using {} ({})",
                        db_path.display(),
                        holder.lines().next().unwrap_or("holder unknown")
                    ),
                ));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
        }
        // Diagnostics for the next instance that finds the lock held.
        file.set_len(0)?;
        let mut writer = &file;
        writer.write_all(format!("pid {}\n", std::process::id()).as_bytes())?;
        Ok(InstanceLock {
            file,
            lock_path,
            db_path: db_path.to_owned(),
        })
    }

    /// `true` when some process holds the lock of `db_path` right now. A missing lock file means "not held".
    /// Used by CLI commands that must refuse while a server runs; it takes and releases the lock when free.
    pub fn is_held(db_path: &Path) -> Result<bool, DbError> {
        if !Self::lock_path_for(db_path).exists() {
            return Ok(false);
        }
        match Self::acquire(db_path) {
            Ok(_free) => Ok(false),
            Err(e) if e.kind == DbErrorKind::Locked => Ok(true),
            Err(e) => Err(e),
        }
    }

    /// The database this lock guards.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// The lock file.
    pub fn lock_path(&self) -> &Path {
        &self.lock_path
    }

    /// Releases the lock now (dropping it has the same effect).
    pub fn release(self) {
        // `File::unlock` can only fail for a closed descriptor; the drop below closes it anyway.
        let _ = self.file.unlock();
    }
}
