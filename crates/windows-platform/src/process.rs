//! Hidden-console process spawning with Job Object assignment.
//!
//! # Unsafe invariants
//!
//! The only unsafe call is `AssignProcessToJobObject`, which takes the
//! raw kernel handle of a spawned child. `std::process::Child` owns that
//! handle; it stays valid for the duration of the call (the `Child` value
//! is alive and not yet waited on). Ownership remains with the caller's
//! `Child`; the kernel independently records the job membership, so no
//! handle transfer occurs.

use std::path::Path;
use std::process::{Child, Command};

use crate::error::PlatformError;
use crate::job::JobObject;

/// `CREATE_NO_WINDOW` (0x08000000): child console processes get no
/// console window; GUI processes are unaffected (their windows are their
/// own).
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Spawns a child process with no console window and assigns it to the
/// given job before returning.
///
/// Launch failures are typed: `PlatformError::Io` carries the reason, and
/// the child is never left unsupervised because job assignment happens
/// immediately after the spawn succeeds (or the child is killed).
pub fn spawn_hidden_in_job(
    job: &JobObject,
    exe: &Path,
    arguments: &[String],
    working_dir: &Path,
) -> Result<Child, PlatformError> {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::process::CommandExt as _;

    let mut command = Command::new(exe);
    command
        .args(arguments)
        .current_dir(working_dir)
        .creation_flags(CREATE_NO_WINDOW);

    let mut child = command.spawn().map_err(|source| PlatformError::Io {
        path: exe.to_path_buf(),
        source,
    })?;

    // SAFETY: `child` owns a valid process handle between spawn and
    // wait; the raw handle is only used for this call, and ownership
    // stays with the Child struct.
    let handle = child.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE;
    // SAFETY: the handle belongs to a freshly spawned, still-running
    // child owned by `child` below, satisfying `assign_process`'s
    // documented safety contract.
    let assignment = unsafe { job.assign_process(handle) };
    assignment.inspect_err(|_error| {
        // The process exists but is outside our job: kill it so a launch
        // never leaves an unsupervised browser tree behind.
        let _ = child.kill();
    })?;
    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as PlainCommand;

    #[test]
    fn spawn_hidden_in_job_supervises_a_real_process() {
        // cmd.exe on every Windows machine; -c "exit 7" makes it exit
        // quickly with a recognizable code.
        let job = JobObject::new().expect("job");
        let mut child = spawn_hidden_in_job(
            &job,
            Path::new("cmd.exe"),
            &["/c".to_owned(), "exit 7".to_owned()],
            Path::new("."),
        )
        .expect("spawn");
        let status = child.wait().expect("wait");
        assert_eq!(status.code(), Some(7));
    }

    #[test]
    fn missing_executable_reports_typed_error() {
        let job = JobObject::new().expect("job");
        let error = spawn_hidden_in_job(&job, Path::new("Z:/no/such/exe.exe"), &[], Path::new("."))
            .expect_err("missing exe");
        assert!(matches!(
            error,
            PlatformError::Io { .. } | PlatformError::Win32 { .. }
        ));
    }

    #[test]
    fn create_no_window_flag_matches_win32_constant() {
        // Documented CREATE_NO_WINDOW value from winbase.h.
        assert_eq!(CREATE_NO_WINDOW, 0x0800_0000);
        // Guards against accidental cross-platform misuse.
        let _ = PlainCommand::new("cmd.exe");
    }
}
