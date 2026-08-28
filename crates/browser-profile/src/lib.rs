//! Launching and supervising isolated Thorium browser profiles.
//!
//! The product invariant this crate exists to uphold is that **one browser
//! profile owns exactly one Thorium `User Data` directory, and two conflicting
//! processes never run against the same one**. Chromium does not defend itself
//! against that: pointing two browsers at one profile directory corrupts
//! preferences, cookies and the history database. The per-profile lock here is
//! what prevents it.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod cdp;
mod launch;
mod lock;
mod session;

pub use cdp::{CdpEndpoint, EmulationSettings, apply_emulation, read_devtools_endpoint};
pub use launch::{LaunchPlan, ProfileLayout, build_launch_plan};
pub use lock::{LockHolder, ProfileLock};
pub use session::{BrowserSession, SessionHandle, SessionState};

use tw_domain::DiagnosticCode;

/// Failures raised while launching or supervising a profile.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProfileError {
    /// The profile already has a running browser session.
    #[error("this profile is already running")]
    AlreadyRunning {
        /// The process that holds the lock, when it could be read.
        holder: Option<LockHolder>,
    },
    /// The profile is not running.
    #[error("this profile is not running")]
    NotRunning,
    /// The `User Data` directory could not be prepared.
    #[error("the profile's browser data folder could not be prepared: {0}")]
    UserData(String),
    /// The per-profile lock could not be taken.
    #[error("the profile lock could not be acquired: {0}")]
    Lock(String),
    /// No usable Thorium installation is selected.
    #[error("no Thorium version is installed; install one from the Browser page first")]
    NoBrowser,
    /// The browser process could not be started.
    #[error("Thorium could not be started: {0}")]
    Launch(String),
    /// The DevTools control channel could not be established.
    #[error("the browser control channel could not be opened: {0}")]
    Cdp(String),
    /// Timezone or locale could not be applied.
    #[error("the profile's timezone or locale could not be applied: {0}")]
    Emulation(String),
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

impl ProfileError {
    /// The stable diagnostic code for this error.
    #[must_use]
    pub const fn code(&self) -> DiagnosticCode {
        match self {
            Self::AlreadyRunning { .. } => DiagnosticCode::ProfileAlreadyRunning,
            Self::NotRunning => DiagnosticCode::ProfileNotRunning,
            Self::UserData(_) => DiagnosticCode::ProfileUserDataFailed,
            Self::Lock(_) => DiagnosticCode::ProfileLockFailed,
            Self::NoBrowser => DiagnosticCode::ThoriumNotInstalled,
            Self::Launch(_) => DiagnosticCode::ProfileLaunchFailed,
            Self::Cdp(_) => DiagnosticCode::CdpUnavailable,
            Self::Emulation(_) => DiagnosticCode::EmulationFailed,
            Self::Io { .. } => DiagnosticCode::IoFailed,
        }
    }

    pub(crate) fn io(operation: &'static str, source: std::io::Error) -> Self {
        Self::Io { operation, source }
    }
}

/// Profile result alias.
pub type ProfileResult<T> = Result<T, ProfileError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_map_to_stable_diagnostic_codes() {
        assert_eq!(
            ProfileError::AlreadyRunning { holder: None }.code(),
            DiagnosticCode::ProfileAlreadyRunning
        );
        assert_eq!(
            ProfileError::NoBrowser.code(),
            DiagnosticCode::ThoriumNotInstalled
        );
        assert_eq!(
            ProfileError::Cdp("x".into()).code(),
            DiagnosticCode::CdpUnavailable
        );
    }
}
