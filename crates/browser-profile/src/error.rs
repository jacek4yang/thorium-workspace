//! Typed browser-profile errors with stable diagnostic codes.

use thorium_workspace_domain::DiagnosticCode as DiagnosticCodeTrait;
#[allow(unused_imports)]
use thorium_workspace_domain::DiagnosticCode as _;
use thorium_workspace_windows_platform::PlatformError;

/// Error type for profile launching and supervision.
#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    /// The browser executable could not be spawned.
    #[error("failed to launch the browser executable: {source}")]
    Spawn {
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// The profile is already running (its lock is held).
    #[error(
        "this profile is already running; switch to the existing window instead of launching a second instance"
    )]
    AlreadyRunning,

    /// A required profile path could not be resolved or created.
    #[error("profile user data directory could not be prepared: {source}")]
    UserDataDir {
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// A platform-layer operation failed (Win32 or clipboard).
    #[error("{0}")]
    Platform(#[from] PlatformError),

    /// The launch specification was invalid.
    #[error("invalid launch specification: {detail}")]
    InvalidSpec {
        /// Why the spec was rejected.
        detail: String,
    },

    /// A launch argument outside the explicit allowlist was requested.
    #[error("launch argument not allowed: {argument}")]
    DisallowedArgument {
        /// The rejected argument.
        argument: String,
    },

    /// The executable path is not a file.
    #[error("browser executable not found at {path}")]
    MissingExecutable {
        /// Configured executable path.
        path: std::path::PathBuf,
    },
}

impl DiagnosticCodeTrait for ProfileError {
    fn diagnostic_code(&self) -> &'static str {
        if let Self::Platform(inner) = self {
            return inner.diagnostic_code();
        }
        self._diagnostic_code()
    }
}

impl ProfileError {
    fn _diagnostic_code(&self) -> &'static str {
        match self {
            Self::Spawn { .. } => "PROFILE_SPAWN_FAILED",
            Self::AlreadyRunning => "PROFILE_ALREADY_RUNNING",
            Self::UserDataDir { .. } => "PROFILE_USER_DATA_DIR_FAILED",
            Self::InvalidSpec { .. } => "PROFILE_INVALID_SPEC",
            Self::DisallowedArgument { .. } => "PROFILE_DISALLOWED_ARGUMENT",
            Self::MissingExecutable { .. } => "PROFILE_MISSING_EXECUTABLE",
            Self::Platform(_) => "PLATFORM_WIN32_FAILED",
        }
    }
}
