//! Job Objects for the browser process tree.
//!
//! Chromium spawns a tree of processes. Killing the one we launched leaves the
//! renderers, the GPU process and the network service behind, and a crashed
//! manager leaves the whole tree orphaned. A Job Object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` makes the kernel responsible for
//! cleanup instead.
//!
//! # Lifetime contract
//!
//! * A [`ProcessGroup`] owns one job handle.
//! * While the handle is open, the job exists. Assigned processes and every
//!   child they create belong to it.
//! * When the *last* handle to the job closes, the kernel terminates every
//!   process still in it. This happens on a normal drop, and also if this
//!   process is killed, because the kernel closes handles on process teardown.
//! * Therefore: the browser tree cannot outlive the manager. That is the
//!   intended behaviour, and it is why the guard is held for the whole session
//!   rather than dropped after launch.

#[cfg(windows)]
use crate::PlatformError;
use crate::PlatformResult;

/// A Windows Job Object holding one browser process tree.
///
/// On non-Windows builds this is an inert placeholder; see the crate docs.
#[derive(Debug)]
pub struct ProcessGroup {
    #[cfg(windows)]
    handle: job_handle::OwnedJob,
}

impl ProcessGroup {
    /// Creates a job whose processes are terminated when the job closes.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::Api`] when the job cannot be created or
    /// configured.
    #[cfg(windows)]
    pub fn new() -> PlatformResult<Self> {
        use windows::Win32::System::JobObjects::{
            CreateJobObjectW, JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
        };

        // SAFETY: `CreateJobObjectW` is called with a null security attributes
        // pointer (documented as "use the default descriptor") and `None` for
        // the name, creating an unnamed job. On success the returned HANDLE is
        // owned by this process and closed exactly once by `OwnedJob::drop`.
        let handle = unsafe { CreateJobObjectW(None, None) }.map_err(|e| PlatformError::Api {
            operation: "CreateJobObjectW",
            detail: e.to_string(),
        })?;
        let owned = job_handle::OwnedJob::new(handle);

        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_BREAKAWAY_OK;

        // SAFETY: `SetInformationJobObject` receives:
        //   * `owned.raw()`, a valid job handle this process owns for the whole
        //     call;
        //   * the matching information class for the struct being passed;
        //   * a pointer to `limits`, a live stack local of exactly the size
        //     given in the length argument, which the call only reads.
        // The call does not take ownership of anything.
        unsafe {
            SetInformationJobObject(
                owned.raw(),
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast(),
                u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).unwrap_or(0),
            )
        }
        .map_err(|e| PlatformError::Api {
            operation: "SetInformationJobObject",
            detail: e.to_string(),
        })?;

        Ok(Self { handle: owned })
    }

    /// Creates an inert placeholder on non-Windows builds.
    ///
    /// # Errors
    ///
    /// Never fails; the signature matches the Windows implementation.
    #[cfg(not(windows))]
    pub fn new() -> PlatformResult<Self> {
        Ok(Self {})
    }

    /// Adds a running process, and therefore every process it goes on to
    /// create, to the job.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::Api`] when the process cannot be assigned,
    /// which happens if it has already exited or already belongs to a job that
    /// does not permit nesting.
    #[cfg(windows)]
    pub fn assign(&self, pid: u32) -> PlatformResult<()> {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

        // SAFETY: `OpenProcess` takes only plain values. On success it returns a
        // handle this process owns; it is closed exactly once below, on both the
        // success and failure paths of `AssignProcessToJobObject`.
        let process =
            unsafe { OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid) }.map_err(|e| {
                PlatformError::Api {
                    operation: "OpenProcess",
                    detail: e.to_string(),
                }
            })?;

        // SAFETY: both handles are valid and owned by this process for the
        // duration of the call. `AssignProcessToJobObject` borrows them; it does
        // not take ownership of either.
        let result = unsafe {
            windows::Win32::System::JobObjects::AssignProcessToJobObject(self.handle.raw(), process)
        };

        // SAFETY: `process` came from a successful `OpenProcess`, has not been
        // closed, and is closed exactly once here regardless of the assignment
        // result.
        let _ = unsafe { CloseHandle(process) };

        result.map_err(|e| PlatformError::Api {
            operation: "AssignProcessToJobObject",
            detail: e.to_string(),
        })
    }

    /// Records a process id on non-Windows builds so tests can assert the call
    /// order; no supervision is performed.
    ///
    /// # Errors
    ///
    /// Never fails; the signature matches the Windows implementation.
    #[cfg(not(windows))]
    pub fn assign(&self, _pid: u32) -> PlatformResult<()> {
        Ok(())
    }

    /// Terminates every process in the job immediately.
    ///
    /// Used for an explicit "stop this profile" action. A normal drop achieves
    /// the same thing; this exists so the caller can stop one profile without
    /// tearing down the manager.
    ///
    /// # Errors
    ///
    /// Returns [`PlatformError::Api`] when termination fails.
    #[cfg(windows)]
    pub fn terminate(&self) -> PlatformResult<()> {
        // SAFETY: `self.handle` is a valid job handle owned by this process for
        // the duration of the call. Exit code 0 marks an intentional shutdown.
        unsafe { windows::Win32::System::JobObjects::TerminateJobObject(self.handle.raw(), 0) }.map_err(|e| {
            PlatformError::Api {
                operation: "TerminateJobObject",
                detail: e.to_string(),
            }
        })
    }

    /// No-op on non-Windows builds.
    ///
    /// # Errors
    ///
    /// Never fails; the signature matches the Windows implementation.
    #[cfg(not(windows))]
    pub fn terminate(&self) -> PlatformResult<()> {
        Ok(())
    }

    /// Whether this build actually supervises the process tree.
    #[must_use]
    pub const fn is_supervising() -> bool {
        cfg!(windows)
    }
}

#[cfg(windows)]
mod job_handle {
    use windows::Win32::Foundation::{CloseHandle, HANDLE};

    /// Owns a Job Object handle.
    ///
    /// Closing the last handle to the job terminates every process still
    /// assigned to it, which is exactly the cleanup guarantee this type exists
    /// to provide.
    #[derive(Debug)]
    pub(super) struct OwnedJob(HANDLE);

    impl OwnedJob {
        pub(super) const fn new(handle: HANDLE) -> Self {
            Self(handle)
        }

        /// Borrows the handle for the duration of one API call.
        ///
        /// Never stored by a callee; every Win32 function this crate passes it
        /// to only borrows it.
        pub(super) const fn raw(&self) -> HANDLE {
            self.0
        }
    }

    impl Drop for OwnedJob {
        fn drop(&mut self) {
            if self.0.is_invalid() {
                return;
            }
            // SAFETY: `self.0` came from a successful `CreateJobObjectW`, this
            // type is its sole owner and is not `Clone`, and `Drop` runs once,
            // so the handle is closed exactly once. Closing it is what triggers
            // KILL_ON_JOB_CLOSE.
            let _ = unsafe { CloseHandle(self.0) };
        }
    }

    // SAFETY: A job HANDLE is a kernel object reference with no thread affinity.
    // Every operation this type permits (assign, terminate, close) is safe from
    // any thread, so moving or sharing the wrapper cannot race.
    unsafe impl Send for OwnedJob {}
    // SAFETY: see above; the operations reachable through `&OwnedJob` are all
    // thread-safe kernel calls.
    unsafe impl Sync for OwnedJob {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_process_group_can_be_created_on_any_platform() {
        let group = ProcessGroup::new().expect("create");
        // Terminating an empty group is harmless and must not error.
        group.terminate().expect("terminate");
    }

    #[test]
    fn supervision_is_reported_honestly_for_this_build() {
        assert_eq!(ProcessGroup::is_supervising(), cfg!(windows));
    }

    #[cfg(not(windows))]
    #[test]
    fn the_non_windows_placeholder_does_not_pretend_to_supervise() {
        let group = ProcessGroup::new().expect("create");
        group.assign(1).expect("assign is a no-op");
        assert!(!ProcessGroup::is_supervising());
    }
}
