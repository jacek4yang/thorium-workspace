//! A running Browser Profile: lock + supervised process tree.
//!
//! Lifecycle:
//! 1. [`Session::launch`] acquires a per-profile named mutex
//!    (double-launch protection) and spawns the browser into a Job
//!    Object with `KILL_ON_JOB_CLOSE`.
//! 2. While the session exists, the lock is held: a second launch of the
//!    same profile fails with [`ProfileError::AlreadyRunning`].
//! 3. [`Session::shutdown`] (or drop) terminates the tree and reaps the
//!    head process, so shutdown cannot orphan children.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::process::Child;

use thorium_workspace_domain::ProfileId;
use thorium_workspace_windows_platform::error::PlatformError;
use thorium_workspace_windows_platform::job::JobObject;
use thorium_workspace_windows_platform::process::spawn_hidden_in_job;

use crate::lock::ProfileLock;

use crate::error::ProfileError;

/// Bounded wait for shutdown before force-killing the head process.
/// The job kill is instant; the wait is only for reaping.
const SHUTDOWN_POLL_MS: u64 = 100;
const SHUTDOWN_POLLS: usize = 30;

/// A live, supervised browser session for one profile.
#[derive(Debug)]
pub struct Session {
    lock: ProfileLock,
    job: JobObject,
    head: Child,
    profile_id: ProfileId,
    user_data_dir: PathBuf,
}

impl Session {
    /// Launches the browser for `profile_id`.
    ///
    /// `executable` must be the absolute path of the browser binary.
    /// `arguments` comes from [`crate::launch::LaunchSpec::build_arguments`].
    /// `user_data_dir` must already exist (see
    /// [`crate::launch::prepare_user_data_dir`]).
    pub fn launch(
        profile_id: ProfileId,
        executable: &Path,
        arguments: &[String],
        user_data_dir: &Path,
    ) -> Result<Self, ProfileError> {
        if !executable.is_file() {
            return Err(ProfileError::MissingExecutable {
                path: executable.to_path_buf(),
            });
        }
        if !user_data_dir.is_dir() {
            return Err(ProfileError::UserDataDir {
                source: std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "user data directory does not exist",
                ),
            });
        }
        // The lock decides double-launch protection; acquire it before
        // spawning so two racing launches cannot both succeed.
        let lock = ProfileLock::try_acquire(&profile_id)?.ok_or(ProfileError::AlreadyRunning)?;
        let job = JobObject::new()?;
        Self::spawn(profile_id, executable, arguments, user_data_dir, lock, job)
    }

    fn spawn(
        profile_id: ProfileId,
        executable: &Path,
        arguments: &[String],
        user_data_dir: &Path,
        lock: ProfileLock,
        job: JobObject,
    ) -> Result<Self, ProfileError> {
        let head =
            spawn_hidden_in_job(&job, executable, arguments, user_data_dir).map_err(|error| {
                ProfileError::Spawn {
                    source: match &error {
                        PlatformError::Io { source, .. } => {
                            std::io::Error::new(source.kind(), source.to_string())
                        }
                        other => std::io::Error::other(other.to_string()),
                    },
                }
            })?;
        Ok(Self {
            lock,
            job,
            head,
            profile_id,
            user_data_dir: user_data_dir.to_path_buf(),
        })
    }

    /// Whether the head process is still alive.
    pub fn is_running(&mut self) -> bool {
        self.head.try_wait().ok().flatten().is_none()
    }

    /// The supervised profile.
    pub fn profile_id(&self) -> ProfileId {
        self.profile_id
    }

    /// The profile's user data directory.
    pub fn user_data_dir(&self) -> &Path {
        &self.user_data_dir
    }

    /// The launch-time lock name (diagnostics).
    pub fn lock_name(&self) -> &str {
        self.lock.name()
    }

    /// Terminates the process tree and waits for the head process.
    pub fn shutdown(mut self) -> Result<(), ProfileError> {
        // The job kill is instant; the bounded reap below is only for
        // bookkeeping (avoiding zombie head processes).
        self.job.terminate()?;
        self.reap_head();
        Ok(())
    }

    fn reap_head(&mut self) {
        for _ in 0..SHUTDOWN_POLLS {
            match self.head.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(SHUTDOWN_POLL_MS)),
                Err(_) => return,
            }
        }
        let _ = self.head.kill();
        let _ = self.head.wait();
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // KILL_ON_JOB_CLOSE makes dropping the job the safety net; an
        // explicit terminate first keeps ordering deterministic.
        let _ = self.job.terminate();
        if self.is_running() {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        let _ = self.head.kill();
        let _ = self.head.wait();
    }
}
