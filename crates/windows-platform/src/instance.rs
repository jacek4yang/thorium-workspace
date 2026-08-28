//! Single-instance enforcement.
//!
//! Two managers pointed at the same portable folder would race on the database,
//! the vault and every profile lock. The guard is acquired once at startup,
//! before any of those are opened.

use std::path::Path;

use crate::{PlatformError, PlatformResult};

/// Derives the object name used to detect a second instance.
///
/// The name is a hash of the workspace path rather than the path itself:
/// object names have a length limit, cannot contain a backslash after the
/// namespace prefix, and a user's folder name may be sensitive. Hashing gives a
/// fixed-length, character-safe, non-revealing name that is still stable for a
/// given folder.
///
/// The path is lowercased first because Windows paths are case-insensitive, so
/// `C:\Tools\TW` and `c:\tools\tw` must collide.
#[must_use]
pub fn instance_name_for(workspace_dir: &Path) -> String {
    use sha2::{Digest, Sha256};
    let normalized = workspace_dir.to_string_lossy().to_lowercase().replace('/', "\\");
    let digest = Sha256::digest(normalized.as_bytes());
    // 16 hex characters is 64 bits: far beyond what is needed to avoid an
    // accidental collision between folders on one machine.
    format!("ThoriumWorkspace-{}", &hex::encode(digest)[..16])
}

/// Holds exclusive ownership of a workspace folder for this process.
///
/// Dropping the guard releases the claim. On Windows the kernel releases the
/// mutex even if the process is killed, so a crashed manager does not lock the
/// user out of their own workspace.
#[derive(Debug)]
pub struct SingleInstanceGuard {
    name: String,
    /// Held for its `Drop`: releasing the handle releases the claim.
    #[cfg(windows)]
    _mutex: windows_handle::OwnedHandle,
    #[cfg(not(windows))]
    _lock: std::fs::File,
}

impl SingleInstanceGuard {
    /// The object name this guard holds.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Claims `workspace_dir` for this process.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::AlreadyRunning`] when another instance holds the
    /// claim, and [`PlatformError::Api`] when the object cannot be created.
    #[cfg(windows)]
    pub fn acquire(workspace_dir: &Path) -> PlatformResult<Self> {
        use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
        use windows::Win32::System::Threading::CreateMutexW;
        use windows::core::HSTRING;

        let name = instance_name_for(workspace_dir);
        // The Local namespace is per-session, which is what we want: two
        // different signed-in users may each run their own copy.
        let object_name = HSTRING::from(format!("Local\\{name}"));

        // SAFETY: `CreateMutexW` is called with:
        //   * a null security attributes pointer, requesting the default
        //     descriptor, which the API explicitly documents as valid;
        //   * `binitialowner = true`, so on success this thread owns the mutex;
        //   * `object_name`, a pointer into `object_name`, which is a live
        //     `HSTRING` for the whole call and is NUL-terminated by construction.
        // On success the returned HANDLE is owned by this process and is closed
        // exactly once by `OwnedHandle::drop`. `GetLastError` is read
        // immediately after the call, before anything else can overwrite the
        // thread's last-error value.
        let (handle, already_existed) = unsafe {
            let handle = CreateMutexW(None, true, &object_name).map_err(|e| PlatformError::Api {
                operation: "CreateMutexW",
                detail: e.to_string(),
            })?;
            let existed = GetLastError() == ERROR_ALREADY_EXISTS;
            (handle, existed)
        };

        let owned = windows_handle::OwnedHandle::new(handle);
        if already_existed {
            // `owned` is dropped here, closing our handle to the existing mutex
            // without affecting the instance that actually owns it.
            drop(owned);
            return Err(PlatformError::AlreadyRunning);
        }
        Ok(Self { name, _mutex: owned })
    }

    /// Claims `workspace_dir` for this process.
    ///
    /// The non-Windows implementation takes an exclusive lock on a file in the
    /// workspace folder. It provides the same guarantee for development and CI
    /// on non-Windows hosts; the shipped product always uses the named mutex.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::AlreadyRunning`] when the lock is held, and
    /// [`PlatformError::Io`] when the lock file cannot be created.
    #[cfg(not(windows))]
    pub fn acquire(workspace_dir: &Path) -> PlatformResult<Self> {
        let name = instance_name_for(workspace_dir);
        std::fs::create_dir_all(workspace_dir)
            .map_err(|e| PlatformError::io("create the workspace folder", e))?;
        let path = workspace_dir.join(".thorium-workspace.lock");
        let file = std::fs::OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| PlatformError::io("create the instance lock file", e))?;
        // `File::try_lock` is the standard library's own advisory lock, so no
        // third-party dependency is needed for the development fallback.
        match file.try_lock() {
            Ok(()) => Ok(Self { name, _lock: file }),
            Err(_) => Err(PlatformError::AlreadyRunning),
        }
    }
}

#[cfg(windows)]
mod windows_handle {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};

    /// Owns a Windows kernel handle and closes it exactly once.
    #[derive(Debug)]
    pub(super) struct OwnedHandle(HANDLE);

    impl OwnedHandle {
        /// Takes ownership of `handle`.
        ///
        /// The caller must not close `handle` afterwards; this wrapper does.
        pub(super) const fn new(handle: HANDLE) -> Self {
            Self(handle)
        }
    }

    impl Drop for OwnedHandle {
        fn drop(&mut self) {
            if self.0.is_invalid() {
                return;
            }
            // SAFETY: `self.0` was returned by a successful `CreateMutexW`, has
            // not been closed (this type is the sole owner and is not `Clone`),
            // and is closed exactly once because `Drop` runs once. The result is
            // ignored deliberately: a failure here can only mean the handle was
            // already invalid, and there is no recovery action at drop time.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    // SAFETY: A Windows mutex HANDLE is a kernel object reference, not a
    // thread-affine resource. It may be closed from any thread, and this type
    // exposes no operation other than closing it, so moving the wrapper between
    // threads cannot introduce a data race.
    unsafe impl Send for OwnedHandle {}
    // SAFETY: `&OwnedHandle` exposes no operations at all, so sharing a
    // reference across threads cannot race.
    unsafe impl Sync for OwnedHandle {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_instance_name_is_stable_and_case_insensitive() {
        let a = instance_name_for(Path::new("C:\\Tools\\ThoriumWorkspace"));
        let b = instance_name_for(Path::new("c:\\tools\\thoriumworkspace"));
        assert_eq!(
            a, b,
            "Windows paths are case-insensitive, so the name must be too"
        );
        assert_eq!(a, instance_name_for(Path::new("C:\\Tools\\ThoriumWorkspace")));
    }

    #[test]
    fn different_folders_get_different_names() {
        assert_ne!(
            instance_name_for(Path::new("C:\\Tools\\A")),
            instance_name_for(Path::new("C:\\Tools\\B"))
        );
    }

    #[test]
    fn forward_and_back_slashes_name_the_same_workspace() {
        assert_eq!(
            instance_name_for(Path::new("C:/Tools/TW")),
            instance_name_for(Path::new("C:\\Tools\\TW"))
        );
    }

    #[test]
    fn the_name_is_short_and_contains_no_path_characters() {
        let name = instance_name_for(Path::new("C:\\Users\\Someone Private\\Portable Apps\\TW"));
        assert!(name.len() < 64, "object names are length limited");
        assert!(
            !name.contains('\\') && !name.contains('/') && !name.contains(' '),
            "{name}"
        );
        assert!(
            !name.contains("Someone Private"),
            "the name must not reveal the folder"
        );
    }

    #[test]
    fn a_second_instance_is_refused_while_the_first_holds_the_workspace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = SingleInstanceGuard::acquire(dir.path()).expect("first instance");
        assert!(first.name().starts_with("ThoriumWorkspace-"));

        match SingleInstanceGuard::acquire(dir.path()) {
            Err(PlatformError::AlreadyRunning) => {}
            Ok(_) => panic!("a second instance must not be able to claim the same workspace"),
            Err(other) => panic!("expected AlreadyRunning, got {other:?}"),
        }

        drop(first);
        // Releasing the guard must let a later instance start.
        let _second = SingleInstanceGuard::acquire(dir.path()).expect("after release");
    }

    #[test]
    fn separate_workspaces_can_run_concurrently() {
        let a = tempfile::tempdir().expect("tempdir");
        let b = tempfile::tempdir().expect("tempdir");
        let _first = SingleInstanceGuard::acquire(a.path()).expect("first");
        let _second = SingleInstanceGuard::acquire(b.path()).expect("second");
    }
}
