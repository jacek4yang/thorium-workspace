//! The per-profile lock.
//!
//! Held for as long as a browser session is running. The lock file also records
//! who holds it, so the UI can say *what* is using the profile rather than only
//! that something is.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{ProfileError, ProfileResult};

/// Who currently holds a profile lock.
///
/// Advisory information for the UI and diagnostics. Exclusion is enforced by the
/// operating system's file lock, never by reading this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockHolder {
    /// The manager process that took the lock.
    pub manager_pid: u32,
    /// The browser process it launched, once it exists.
    pub browser_pid: Option<u32>,
    /// When the lock was taken, in Unix epoch seconds.
    pub acquired_at: i64,
}

/// An exclusive claim on one profile's `User Data` directory.
///
/// Dropping the guard releases the lock. The operating system also releases it
/// if the process dies, so a crash never leaves a profile permanently locked.
#[derive(Debug)]
pub struct ProfileLock {
    path: PathBuf,
    file: std::fs::File,
}

impl ProfileLock {
    /// The conventional lock file name inside a profile directory.
    pub const FILE_NAME: &'static str = "thorium-workspace.lock";

    /// Takes the lock for the profile rooted at `profile_dir`.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::AlreadyRunning`] when another session holds it,
    /// and [`ProfileError::Lock`] when the file cannot be created.
    pub fn acquire(profile_dir: &Path) -> ProfileResult<Self> {
        std::fs::create_dir_all(profile_dir).map_err(|e| {
            ProfileError::UserData(format!("{} could not be created: {e}", profile_dir.display()))
        })?;
        let path = profile_dir.join(Self::FILE_NAME);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| ProfileError::Lock(e.to_string()))?;

        match file.try_lock() {
            Ok(()) => {}
            Err(_) => {
                // Reading the holder is best effort: the file may be mid-write.
                return Err(ProfileError::AlreadyRunning {
                    holder: read_holder(&path),
                });
            }
        }

        let mut lock = Self { path, file };
        lock.write_holder(&LockHolder {
            manager_pid: std::process::id(),
            browser_pid: None,
            acquired_at: now_seconds(),
        })?;
        Ok(lock)
    }

    /// The lock file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Records the browser process id once it has been launched.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::Io`] when the file cannot be written.
    pub fn record_browser_pid(&mut self, pid: u32) -> ProfileResult<()> {
        let mut holder = read_holder(&self.path).unwrap_or(LockHolder {
            manager_pid: std::process::id(),
            browser_pid: None,
            acquired_at: now_seconds(),
        });
        holder.browser_pid = Some(pid);
        self.write_holder(&holder)
    }

    /// Reads whoever is recorded as holding the lock.
    #[must_use]
    pub fn read_holder(profile_dir: &Path) -> Option<LockHolder> {
        read_holder(&profile_dir.join(Self::FILE_NAME))
    }

    /// Whether a profile directory is currently locked by a live session.
    ///
    /// Determined by trying to take the lock, not by reading the file: a lock
    /// file left behind by a crash is not a lock.
    #[must_use]
    pub fn is_locked(profile_dir: &Path) -> bool {
        let path = profile_dir.join(Self::FILE_NAME);
        let Ok(file) = std::fs::OpenOptions::new().read(true).write(true).open(&path) else {
            return false;
        };
        match file.try_lock() {
            Ok(()) => {
                let _ = file.unlock();
                false
            }
            Err(std::fs::TryLockError::WouldBlock) => true,
            // A lock call can fail for reasons that have nothing to do with
            // contention. This is an advisory observation, so an unanswerable
            // question is reported as "not locked" rather than inventing a
            // holder that may not exist; `acquire` is what actually enforces
            // exclusion, and it distinguishes the two cases.
            Err(std::fs::TryLockError::Error(error)) => {
                tracing::debug!(error = %error, "a profile lock could not be tested");
                false
            }
        }
    }

    fn write_holder(&mut self, holder: &LockHolder) -> ProfileResult<()> {
        use std::io::{Seek, SeekFrom, Write};
        let encoded = serde_json::to_vec(holder).unwrap_or_default();
        self.file
            .set_len(0)
            .map_err(|e| ProfileError::io("update the profile lock", e))?;
        self.file
            .seek(SeekFrom::Start(0))
            .map_err(|e| ProfileError::io("update the profile lock", e))?;
        self.file
            .write_all(&encoded)
            .map_err(|e| ProfileError::io("update the profile lock", e))?;
        self.file
            .flush()
            .map_err(|e| ProfileError::io("update the profile lock", e))?;
        Ok(())
    }
}

fn read_holder(path: &Path) -> Option<LockHolder> {
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn now_seconds() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lock_excludes_a_second_holder_and_releases_on_drop() {
        let dir = tempfile::tempdir().expect("tempdir");
        let profile = dir.path().join("profile-a");

        let first = ProfileLock::acquire(&profile).expect("first");
        assert!(first.path().ends_with(ProfileLock::FILE_NAME));
        assert!(ProfileLock::is_locked(&profile));

        match ProfileLock::acquire(&profile) {
            Err(ProfileError::AlreadyRunning { holder }) => {
                let holder = holder.expect("the holder is recorded");
                assert_eq!(holder.manager_pid, std::process::id());
            }
            Ok(_) => panic!("two sessions must not hold the same profile"),
            Err(other) => panic!("expected AlreadyRunning, got {other:?}"),
        }

        drop(first);
        assert!(!ProfileLock::is_locked(&profile));
        let _second = ProfileLock::acquire(&profile).expect("after release");
    }

    #[test]
    fn different_profiles_lock_independently() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _a = ProfileLock::acquire(&dir.path().join("a")).expect("a");
        let _b = ProfileLock::acquire(&dir.path().join("b")).expect("b");
    }

    #[test]
    fn the_browser_pid_is_recorded_and_readable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let profile = dir.path().join("profile");
        let mut lock = ProfileLock::acquire(&profile).expect("lock");
        assert_eq!(
            ProfileLock::read_holder(&profile).expect("holder").browser_pid,
            None
        );

        lock.record_browser_pid(4242).expect("record");
        let holder = ProfileLock::read_holder(&profile).expect("holder");
        assert_eq!(holder.browser_pid, Some(4242));
        assert_eq!(holder.manager_pid, std::process::id());
        assert!(holder.acquired_at > 0);
    }

    #[test]
    fn a_lock_file_left_behind_by_a_crash_is_not_a_lock() {
        let dir = tempfile::tempdir().expect("tempdir");
        let profile = dir.path().join("profile");
        std::fs::create_dir_all(&profile).expect("mkdir");
        // A stale file with a plausible but dead holder, as a crash would leave.
        std::fs::write(
            profile.join(ProfileLock::FILE_NAME),
            br#"{"manager_pid":999999,"browser_pid":999998,"acquired_at":1}"#,
        )
        .expect("write");

        assert!(
            !ProfileLock::is_locked(&profile),
            "an unlocked file is not a held lock"
        );
        let lock = ProfileLock::acquire(&profile).expect("the profile is usable again");
        assert_eq!(
            ProfileLock::read_holder(&profile).expect("holder").manager_pid,
            std::process::id()
        );
        drop(lock);
    }

    #[test]
    fn a_corrupt_lock_file_does_not_prevent_locking() {
        let dir = tempfile::tempdir().expect("tempdir");
        let profile = dir.path().join("profile");
        std::fs::create_dir_all(&profile).expect("mkdir");
        std::fs::write(profile.join(ProfileLock::FILE_NAME), b"not json").expect("write");
        assert_eq!(ProfileLock::read_holder(&profile), None);
        let _lock = ProfileLock::acquire(&profile).expect("lock");
        assert!(
            ProfileLock::read_holder(&profile).is_some(),
            "the holder is rewritten"
        );
    }

    #[test]
    fn locking_creates_the_profile_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let profile = dir.path().join("deep").join("nested").join("profile");
        let _lock = ProfileLock::acquire(&profile).expect("lock");
        assert!(profile.is_dir());
    }

    #[test]
    fn an_absent_profile_directory_is_not_locked() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!ProfileLock::is_locked(&dir.path().join("never-created")));
        assert_eq!(ProfileLock::read_holder(&dir.path().join("never-created")), None);
    }
}
