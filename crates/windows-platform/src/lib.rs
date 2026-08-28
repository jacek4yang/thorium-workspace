//! The Windows platform layer.
//!
//! This is the only crate in the workspace permitted to use `unsafe`, and it is
//! kept deliberately small: a single-instance mutex, a Job Object for the
//! browser process tree, console-free process spawning, process liveness,
//! window activation and a screen capture used by the QR scanner. Everything
//! else is safe Rust in other crates.
//!
//! # Unsafe policy
//!
//! Every `unsafe` block below states why it is necessary, what it assumes about
//! the pointers it passes, who owns each handle and when that handle is closed.
//! Raw handles are never exposed: each is owned by a wrapper whose `Drop`
//! releases it exactly once.
//!
//! # Non-Windows builds
//!
//! The shipped product is Windows-only. The `#[cfg(not(windows))]` bodies exist
//! so the workspace can be compiled and tested on a developer or CI machine
//! without a Windows host; they either provide a portable equivalent (an
//! exclusive file lock in place of a named mutex) or return
//! [`PlatformError::Unsupported`]. They are never compiled into a release
//! artefact, which is always built for `x86_64-pc-windows-msvc`.
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![cfg_attr(not(windows), forbid(unsafe_code))]

mod instance;
mod job;
mod process;
mod screen;
mod window;

pub use instance::{SingleInstanceGuard, instance_name_for};
pub use job::ProcessGroup;
pub use process::{ChildProcess, SpawnOptions, process_is_running, spawn};
pub use screen::{ScreenCapture, capture_virtual_screen};
pub use window::focus_window_of_process;

use tw_domain::DiagnosticCode;

/// Failures raised by the platform layer.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PlatformError {
    /// Another instance already owns this workspace.
    #[error("another Thorium Workspace instance is already using this folder")]
    AlreadyRunning,
    /// A Windows API call failed.
    #[error("{operation} failed: {detail}")]
    Api {
        /// What was being attempted.
        operation: &'static str,
        /// The system's description of the failure.
        detail: String,
    },
    /// The operation is only available on Windows.
    #[error("{0} is only available on Windows")]
    Unsupported(&'static str),
    /// An I/O operation failed.
    #[error("{operation} failed: {source}")]
    Io {
        /// What was being attempted.
        operation: &'static str,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
}

impl PlatformError {
    /// The stable diagnostic code for this error.
    #[must_use]
    pub const fn code(&self) -> DiagnosticCode {
        match self {
            Self::AlreadyRunning => DiagnosticCode::WorkspaceAlreadyRunning,
            Self::Api { .. } => DiagnosticCode::WindowsApiFailed,
            Self::Unsupported(_) => DiagnosticCode::UnsupportedPlatform,
            Self::Io { .. } => DiagnosticCode::IoFailed,
        }
    }

    pub(crate) fn io(operation: &'static str, source: std::io::Error) -> Self {
        Self::Io { operation, source }
    }
}

/// Platform result alias.
pub type PlatformResult<T> = Result<T, PlatformError>;

/// Whether this build runs on Windows.
///
/// Used by diagnostics to state plainly which platform behaviour is active.
#[must_use]
pub const fn is_windows() -> bool {
    cfg!(windows)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_map_to_stable_diagnostic_codes() {
        assert_eq!(
            PlatformError::AlreadyRunning.code(),
            DiagnosticCode::WorkspaceAlreadyRunning
        );
        assert_eq!(
            PlatformError::Api {
                operation: "CreateMutexW",
                detail: "x".into()
            }
            .code(),
            DiagnosticCode::WindowsApiFailed
        );
        assert_eq!(
            PlatformError::Unsupported("job objects").code(),
            DiagnosticCode::UnsupportedPlatform
        );
    }
}
