//! The single error type crossing the Tauri boundary.
//!
//! Every underlying error is mapped into this shape so the frontend receives a
//! stable code, a message written for a person, and an optional remedy.
//! Nothing here can carry secret material: each `From` impl goes through the
//! source error's own `Display`, and every one of those is secret-free by
//! construction.

use serde::{Deserialize, Serialize};
use tw_domain::{DiagnosticCode, DomainError};

/// Controller result alias.
pub type AppResult<T> = Result<T, AppError>;

/// An error the frontend can render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code}: {message}")]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    /// Stable diagnostic identifier.
    pub code: DiagnosticCode,
    /// Explanation written for the person who has to act on it.
    pub message: String,
    /// What to do about it, when there is a clear answer.
    pub remedy: Option<String>,
}

impl AppError {
    /// Builds an error.
    #[must_use]
    pub fn new(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            remedy: None,
        }
    }

    /// Attaches a remedy.
    #[must_use]
    pub fn with_remedy(mut self, remedy: impl Into<String>) -> Self {
        self.remedy = Some(remedy.into());
        self
    }

    /// Shorthand for an internal invariant failure.
    #[must_use]
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(DiagnosticCode::Internal, message)
    }

    /// Shorthand for a caller-input failure.
    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(DiagnosticCode::InvalidInput, message)
    }
}

impl From<DomainError> for AppError {
    fn from(value: DomainError) -> Self {
        Self {
            code: value.code,
            message: value.message,
            remedy: value.remedy,
        }
    }
}

impl From<tw_storage::StorageError> for AppError {
    fn from(value: tw_storage::StorageError) -> Self {
        Self::new(value.code(), value.to_string())
    }
}

impl From<tw_vault::VaultError> for AppError {
    fn from(value: tw_vault::VaultError) -> Self {
        let code = value.code();
        let error = Self::new(code, value.to_string());
        match code {
            DiagnosticCode::VaultLocked => error.with_remedy("Unlock the vault and try again."),
            DiagnosticCode::VaultCorrupt => {
                error.with_remedy("Restore the vault from a backup, or from the .bak file beside it.")
            }
            _ => error,
        }
    }
}

impl From<tw_otp::OtpUriError> for AppError {
    fn from(value: tw_otp::OtpUriError) -> Self {
        Self::new(DiagnosticCode::OtpUriInvalid, value.to_string())
    }
}

impl From<tw_qr::QrError> for AppError {
    fn from(value: tw_qr::QrError) -> Self {
        Self::new(value.code(), value.to_string())
    }
}

impl From<tw_thorium::ThoriumError> for AppError {
    fn from(value: tw_thorium::ThoriumError) -> Self {
        Self::new(value.code(), value.to_string())
    }
}

impl From<tw_browser_profile::ProfileError> for AppError {
    fn from(value: tw_browser_profile::ProfileError) -> Self {
        let code = value.code();
        let error = Self::new(code, value.to_string());
        match code {
            DiagnosticCode::ThoriumNotInstalled => {
                error.with_remedy("Install a Thorium version from the Browser page first.")
            }
            DiagnosticCode::ProfileAlreadyRunning => {
                error.with_remedy("Stop the running session before launching it again.")
            }
            _ => error,
        }
    }
}

impl From<tw_windows_platform::PlatformError> for AppError {
    fn from(value: tw_windows_platform::PlatformError) -> Self {
        Self::new(value.code(), value.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        Self::new(DiagnosticCode::IoFailed, value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_serialize_in_the_shape_the_frontend_expects() {
        let error =
            AppError::new(DiagnosticCode::VaultLocked, "the vault is locked").with_remedy("Unlock it.");
        let json = serde_json::to_value(&error).expect("serialize");
        assert_eq!(json["code"], "VAULT_LOCKED");
        assert_eq!(json["message"], "the vault is locked");
        assert_eq!(json["remedy"], "Unlock it.");
    }

    #[test]
    fn a_locked_vault_error_carries_an_actionable_remedy() {
        let error: AppError = tw_vault::VaultError::Locked.into();
        assert_eq!(error.code, DiagnosticCode::VaultLocked);
        assert!(error.remedy.is_some());
    }

    #[test]
    fn a_bad_password_error_never_reveals_anything_about_the_password() {
        let error: AppError = tw_vault::VaultError::BadPassword.into();
        assert_eq!(error.code, DiagnosticCode::VaultBadPassword);
        assert_eq!(error.message, "the master password is incorrect");
        assert_eq!(error.remedy, None);
    }

    #[test]
    fn domain_errors_keep_their_code_and_remedy() {
        let domain = DomainError::new(DiagnosticCode::WorkspaceNotWritable, "cannot write")
            .with_remedy("Move the app.");
        let error: AppError = domain.into();
        assert_eq!(error.code, DiagnosticCode::WorkspaceNotWritable);
        assert_eq!(error.remedy.as_deref(), Some("Move the app."));
    }

    #[test]
    fn every_source_error_maps_to_its_own_code() {
        let cases: Vec<(AppError, DiagnosticCode)> = vec![
            (
                tw_storage::StorageError::Conflict("x".into()).into(),
                DiagnosticCode::RecordConflict,
            ),
            (tw_qr::QrError::NoQrCode.into(), DiagnosticCode::QrNotFound),
            (
                tw_thorium::ThoriumError::VersionMissing("v".into()).into(),
                DiagnosticCode::ThoriumVersionMissing,
            ),
            (
                tw_browser_profile::ProfileError::NotRunning.into(),
                DiagnosticCode::ProfileNotRunning,
            ),
            (
                tw_windows_platform::PlatformError::AlreadyRunning.into(),
                DiagnosticCode::WorkspaceAlreadyRunning,
            ),
        ];
        for (error, expected) in cases {
            assert_eq!(error.code, expected, "{error:?}");
            assert!(!error.message.is_empty());
        }
    }
}
