//! Win32 named mutex wrapper.
//!
//! # Unsafe invariants
//!
//! - `CreateMutexW` with `NULL` security attributes and a non-owning
//!   (`false`) initial-owner flag returns either a fresh or an existing
//!   mutex handle. The returned `HANDLE` is owned by
//!   [`NamedMutexGuard`]; [`CloseHandle`] is called exactly once, in
//!   [`Drop::drop`]. No other code may close the handle.
//! - `WaitForSingleObject` with a zero timeout never blocks; it only
//!   reports current ownership. `WAIT_ABANDONED` means the previous owner
//!   terminated without releasing; the kernel has already transferred
//!   ownership to this caller, so it is treated as acquired (crash
//!   recovery).
//! - Handles are process-wide kernel objects with no pointer lifetime
//!   beyond the handle value itself; there are no borrows into Win32
//!   memory. The wide name buffer outlives the `CreateMutexW` call
//!   because it is consumed before any unsafe use and is independent of
//!   the handle.

use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;

use windows_sys::Win32::Foundation::{CloseHandle, WAIT_ABANDONED, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{CreateMutexW, INFINITE, WaitForSingleObject};

use crate::error::PlatformError;

/// Current `GetLastError()` as a `u32` (positive codes only).
fn last_os_error_code() -> u32 {
    u32::try_from(std::io::Error::last_os_error().raw_os_error().unwrap_or(0)).unwrap_or(0)
}

/// A held named mutex. Dropping the guard releases the mutex (the handle
/// is closed, which destroys the object once all handles are closed).
#[derive(Debug)]
pub struct NamedMutexGuard {
    handle: windows_sys::Win32::Foundation::HANDLE,
    name: String,
}

// SAFETY: HANDLE is a kernel handle valid across threads; the mutex is a
// synchronization object designed for cross-thread contention. Closing
// happens on whichever thread drops the guard, which is sound for a
// kernel handle with no thread affinity.
unsafe impl Send for NamedMutexGuard {}
unsafe impl Sync for NamedMutexGuard {}

impl NamedMutexGuard {
    /// Builds a guard from a raw handle and the symbolic name.
    fn new(handle: windows_sys::Win32::Foundation::HANDLE, name: String) -> Self {
        Self { handle, name }
    }

    /// Symbolic name of the held mutex.
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for NamedMutexGuard {
    fn drop(&mut self) {
        // SAFETY: `self.handle` came from CreateMutexW and is closed here
        // exactly once (the guard owns it; Clone is not implemented).
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

/// Tries to acquire the named mutex without blocking. Returns `Ok(Some)`
/// when this process now owns the mutex and `Ok(None)` when another
/// *thread* holds it (same-thread re-acquisition succeeds: Windows
/// mutexes are re-entrant per owning thread). An abandoned mutex
/// (previous owner crashed) is claimed: ownership has already
/// transferred to this process.
///
/// The kernel keeps the object alive while any handle is open, so
/// callers holding multiple guards must drop them all before foreign
/// threads can acquire.
pub fn try_acquire_named_mutex(name: &str) -> Result<Option<NamedMutexGuard>, PlatformError> {
    let mut wide: Vec<u16> = std::ffi::OsStr::new(name).encode_wide().collect();
    wide.push(0);
    // SAFETY: `wide` is a NUL-terminated UTF-16 buffer valid for the
    // duration of the call (consumed before the handle is used).
    // `bMutex` (initial owner) is `false` so acquisition is decided only
    // by WaitForSingleObject below.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
    if handle.is_null() {
        return Err(PlatformError::win32("CreateMutexW", last_os_error_code()));
    }
    // SAFETY: `handle` is the valid mutex handle from CreateMutexW above;
    // a zero timeout makes this a non-blocking poll.
    let wait = unsafe { WaitForSingleObject(handle, 0) };
    match wait {
        WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(Some(NamedMutexGuard::new(handle, name.to_owned()))),
        WAIT_TIMEOUT => {
            // Another process owns the mutex; release our reference.
            // SAFETY: `handle` is valid and closed exactly once here.
            unsafe {
                CloseHandle(handle);
            }
            Ok(None)
        }
        code => Err(PlatformError::win32("WaitForSingleObject", code)),
    }
}

/// Convenience: try to acquire the workspace mutex derived from `root`.
pub fn try_acquire_workspace_mutex(root: &Path) -> Result<Option<NamedMutexGuard>, PlatformError> {
    let name = crate::mutex_name::mutex_name_for(root)?;
    try_acquire_named_mutex(&name)
}

/// Acquires the named mutex, blocking until it becomes available
/// (including after an abandoning owner exits). Used for per-resource
/// locks where waiting is the desired semantics.
pub fn acquire_named_mutex_blocking(name: &str) -> Result<NamedMutexGuard, PlatformError> {
    let mut wide: Vec<u16> = std::ffi::OsStr::new(name).encode_wide().collect();
    wide.push(0);
    // SAFETY: as in `try_acquire_named_mutex`: valid NUL-terminated name,
    // non-owning initial owner.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 0, wide.as_ptr()) };
    if handle.is_null() {
        return Err(PlatformError::win32("CreateMutexW", last_os_error_code()));
    }
    // SAFETY: `handle` is valid; blocking wait is the documented purpose
    // of this function.
    let wait = unsafe { WaitForSingleObject(handle, INFINITE) };
    match wait {
        WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(NamedMutexGuard::new(handle, name.to_owned())),
        _ => {
            // SAFETY: `handle` is valid and must be closed on the error
            // path to avoid a handle leak.
            unsafe {
                CloseHandle(handle);
            }
            Err(PlatformError::win32(
                "WaitForSingleObject",
                last_os_error_code(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquisition_is_exclusive_across_threads_and_reentrant_after_release() {
        let name = format!("Local\\ThoriumWorkspace-test-{}", std::process::id());
        let guard = try_acquire_named_mutex(&name)
            .expect("first acquire")
            .expect("must acquire");
        // Windows mutexes are re-entrant for the owning thread, so the
        // real exclusivity contract is cross-thread: a different thread
        // must observe the mutex as held.
        let name_for_thread = name.clone();
        let other_thread = std::thread::spawn(move || {
            try_acquire_named_mutex(&name_for_thread).expect("thread attempt")
        });
        let second = other_thread.join().expect("thread joins");
        assert!(second.is_none(), "mutex must remain held across threads");
        drop(guard);
        // After release (all handles closed) it can be acquired again.
        let third = try_acquire_named_mutex(&name)
            .expect("third attempt")
            .expect("must acquire after release");
        assert_eq!(third.name(), name);
    }

    #[test]
    fn same_thread_reacquisition_is_reentrant_and_release_needs_all_guards() {
        // Documents the Win32 re-entrancy: the same thread may acquire
        // the mutex again. The kernel frees the object only when ALL
        // handles close — so both guards must drop before a foreign
        // thread can take it.
        let name = format!("Local\\ThoriumWorkspace-reentrant-{}", std::process::id());
        let first = try_acquire_named_mutex(&name)
            .expect("first")
            .expect("held");
        let second = try_acquire_named_mutex(&name)
            .expect("second")
            .expect("re-entrant acquisition succeeds");
        drop(second);
        let name_for_thread = name.clone();
        let other_thread = std::thread::spawn(move || {
            try_acquire_named_mutex(&name_for_thread).expect("thread attempt")
        });
        let foreign = other_thread.join().expect("thread joins");
        assert!(foreign.is_none(), "one handle still holds the mutex");
        drop(first);
        let foreign_again = try_acquire_named_mutex(&name)
            .expect("after full release")
            .expect("all handles closed");
        drop(foreign_again);
    }

    #[test]
    fn blocking_acquisition_works_after_release() {
        let name = format!("Local\\ThoriumWorkspace-blocking-{}", std::process::id());
        {
            let guard = try_acquire_named_mutex(&name)
                .expect("acquire")
                .expect("held");
            drop(guard);
        }
        let guard = acquire_named_mutex_blocking(&name).expect("blocking acquire");
        assert_eq!(guard.name(), name);
    }
}
