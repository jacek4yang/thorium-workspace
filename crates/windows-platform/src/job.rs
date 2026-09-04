//! Windows Job Object supervision for browser process trees.
//!
//! # Unsafe invariants
//!
//! - `CreateJobObjectW` returns an owned kernel handle stored in
//!   [`JobObject`]; [`CloseHandle`] runs exactly once in `Drop`, on
//!   whichever thread drops the value.
//! - `SetInformationJobObject` writes a plain struct
//!   (`JOBOBJECT_EXTENDED_LIMIT_INFORMATION`) by pointer for the duration
//!   of the call only; no Win32-side pointer is retained.
//! - `AssignProcessToJobObject` consumes a process handle borrowed for
//!   the call; the kernel takes a reference on the process object, so the
//!   caller's handle may outlive the assignment or be closed after.
//! - `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` means the OS kills every
//!   assigned process when the last job handle closes: dropping
//!   [`JobObject`] (or process exit) is the cleanup invariant — the
//!   browser tree cannot outlive the manager.
//! - `TerminateJobObject` is used for explicit shutdown; after a
//!   terminate, processes are gone but the handle stays valid.

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};

use crate::error::PlatformError;

/// Current `GetLastError()` as a `u32` (positive codes only).
fn last_os_error_code() -> u32 {
    u32::try_from(std::io::Error::last_os_error().raw_os_error().unwrap_or(0)).unwrap_or(0)
}

/// An owned Job Object configured with `KILL_ON_JOB_CLOSE`.
#[derive(Debug)]
pub struct JobObject {
    handle: HANDLE,
}

// SAFETY: HANDLE is a kernel handle valid across threads; Job Objects are
// synchronization/ownership objects with no thread affinity.
unsafe impl Send for JobObject {}
unsafe impl Sync for JobObject {}

impl JobObject {
    /// Creates a job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
    /// Every process assigned to the job dies when this handle closes —
    /// including on abnormal manager exit, preventing orphaned browser
    /// trees.
    pub fn new() -> Result<Self, PlatformError> {
        // SAFETY: `lpName` is NULL (anonymous) and `lpJobAttributes` is
        // NULL (default security). Both are read-only inputs for the call.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(PlatformError::win32(
                "CreateJobObjectW",
                last_os_error_code(),
            ));
        }
        // SAFETY: `JOBOBJECT_EXTENDED_LIMIT_INFORMATION` is a plain-data
        // Win32 struct whose all-zero bit pattern is the documented
        // initial state; zeroing is the standard way to initialize it
        // before setting only the fields we need.
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: `handle` is the valid job handle from CreateJobObjectW;
        // `limits` is a fully initialized struct whose pointer is only
        // borrowed for the call; the size argument matches the type.
        let ok = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                &mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *mut core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            let code = last_os_error_code();
            // SAFETY: `handle` is valid and owned by us; closing on the
            // error path prevents a handle leak.
            unsafe {
                CloseHandle(handle);
            }
            return Err(PlatformError::win32("SetInformationJobObject", code));
        }
        Ok(Self { handle })
    }

    /// Assigns a process (by raw handle) to the job. Each process may be
    /// in exactly one job, so this must happen once per process.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that `process_handle` is a valid,
    /// unattached kernel process handle (e.g. obtained from a freshly
    /// spawned `Child`). The handle is only borrowed for this call; the
    /// kernel retains its own reference to the process.
    pub unsafe fn assign_process(&self, process_handle: HANDLE) -> Result<(), PlatformError> {
        // SAFETY: `process_handle` validity is the caller's documented
        // obligation; `self.handle` is valid for the lifetime of `self`.
        let ok = unsafe { AssignProcessToJobObject(self.handle, process_handle) };
        if ok == 0 {
            return Err(PlatformError::win32(
                "AssignProcessToJobObject",
                last_os_error_code(),
            ));
        }
        Ok(())
    }

    /// Explicitly kills all processes in the job (clean shutdown path).
    pub fn terminate(&self) -> Result<(), PlatformError> {
        // SAFETY: `self.handle` is valid for the lifetime of `self`.
        let ok = unsafe { TerminateJobObject(self.handle, 1) };
        if ok == 0 {
            return Err(PlatformError::win32(
                "TerminateJobObject",
                last_os_error_code(),
            ));
        }
        Ok(())
    }
}

impl Drop for JobObject {
    fn drop(&mut self) {
        // SAFETY: `self.handle` came from CreateJobObjectW, is owned
        // exclusively by this struct (no Clone), and is closed here
        // exactly once. KILL_ON_JOB_CLOSE makes this drop the kill switch
        // for the whole process tree.
        unsafe {
            CloseHandle(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_object_lifecycle_works() {
        let job = JobObject::new().expect("create job");
        // Terminating an empty job is legal and a no-op for the OS.
        job.terminate().expect("terminate empty job");
        // Dropping closes the handle; repeated drop is impossible without
        // Clone, and a double CloseHandle would fail loudly under
        // Application Verifier, so this is the single-closure proof.
        drop(job);
    }
}
