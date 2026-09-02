//! Typed platform errors with stable diagnostic codes.
//!
//! Error text never contains secret material. Paths appear because they
//! are essential for diagnosing portable-workspace problems.

use std::path::PathBuf;

use thorium_workspace_domain::DiagnosticCode;

/// Error type for Windows platform operations.
#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    /// The executable path could not be resolved.
    #[error("failed to resolve the executable path: {source}")]
    ExePath {
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// The portable workspace root is not writable.
    #[error(
        "the application directory is not writable: {path}. Move the application to a writable folder (for example a folder under your user profile) and run it again"
    )]
    NotWritable {
        /// Directory that failed the write probe.
        path: PathBuf,
    },

    /// A filesystem operation failed.
    #[error("filesystem operation failed for {path}: {source}")]
    Io {
        /// Target path.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// A Win32 call failed.
    #[error("Windows API call failed: {operation} (system error {code})")]
    Win32 {
        /// Human name of the failing call.
        operation: &'static str,
        /// GetLastError() value.
        code: u32,
    },

    /// A Win32 call returned a value outside the expected set.
    #[error("Windows API call returned an unexpected result: {operation}")]
    Win32Unexpected {
        /// Human name of the failing call.
        operation: &'static str,
    },

    /// The clipboard operation failed.
    #[error("clipboard operation failed: {0}")]
    Clipboard(String),

    /// A path could not be converted to UTF-16 (rare; implies invalid
    /// surrogate data).
    #[error("path contains invalid Unicode: {path}")]
    WideConversion {
        /// Offending path.
        path: PathBuf,
    },
}

impl PlatformError {
    /// Maps a raw Win32 error code from `GetLastError`.
    pub fn win32(operation: &'static str, code: u32) -> Self {
        Self::Win32 { operation, code }
    }
}

impl DiagnosticCode for PlatformError {
    fn diagnostic_code(&self) -> &'static str {
        match self {
            Self::ExePath { .. } => "PLATFORM_EXE_PATH_FAILED",
            Self::NotWritable { .. } => "PLATFORM_NOT_WRITABLE",
            Self::Io { .. } => "PLATFORM_IO_FAILED",
            Self::Win32 { .. } => "PLATFORM_WIN32_FAILED",
            Self::Win32Unexpected { .. } => "PLATFORM_WIN32_UNEXPECTED",
            Self::Clipboard(_) => "PLATFORM_CLIPBOARD_FAILED",
            Self::WideConversion { .. } => "PLATFORM_WIDE_CONVERSION",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_codes_are_stable() {
        assert_eq!(
            PlatformError::win32("CreateMutexW", 183).diagnostic_code(),
            "PLATFORM_WIN32_FAILED"
        );
        let error = PlatformError::NotWritable {
            path: PathBuf::from("C:/not-writable"),
        };
        assert_eq!(error.diagnostic_code(), "PLATFORM_NOT_WRITABLE");
        let rendered = format!("{error}");
        assert!(rendered.contains("writable"), "actionable hint expected");
    }
}
