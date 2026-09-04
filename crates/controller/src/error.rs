//! Typed controller errors with stable diagnostic codes.
//!
//! The controller composes subsystem errors; no variant carries secret
//! material.

use thorium_workspace_domain::DiagnosticCode;

/// Error type for all application-service operations.
#[derive(Debug, thiserror::Error)]
pub enum ControllerError {
    /// A platform-layer operation failed.
    #[error("{0}")]
    Platform(#[from] thorium_workspace_windows_platform::PlatformError),

    /// A persistence operation failed.
    #[error("{0}")]
    Storage(#[from] thorium_workspace_storage::StorageError),

    /// A vault operation failed.
    #[error("{0}")]
    Vault(#[from] thorium_workspace_vault::VaultError),

    /// A browser-profile operation failed.
    #[error("{0}")]
    Profile(#[from] thorium_workspace_browser_profile::ProfileError),

    /// A Thorium manager operation failed.
    #[error("{0}")]
    Thorium(#[from] thorium_workspace_thorium::ThoriumError),

    /// OTP computation or parsing failed.
    #[error("{0}")]
    Otp(#[from] thorium_workspace_otp::OtpError),

    /// QR decoding failed.
    #[error("{0}")]
    Qr(#[from] thorium_workspace_qr::QrError),

    /// Domain validation failed.
    #[error("{0}")]
    Domain(#[from] thorium_workspace_domain::DomainError),

    /// The workspace is already open in another process.
    #[error("the workspace is already open in another Thorium Workspace instance")]
    WorkspaceInUse,

    /// A referenced entity does not exist.
    #[error("{entity} not found")]
    NotFound {
        /// Human entity name ("profile", "account", "factor").
        entity: &'static str,
    },

    /// No Thorium version is selected as current.
    #[error("no Thorium version is installed or selected; install one from the Browser page")]
    NoCurrentThorium,
}

impl DiagnosticCode for ControllerError {
    fn diagnostic_code(&self) -> &'static str {
        match self {
            Self::Platform(error) => error.diagnostic_code(),
            Self::Storage(error) => error.diagnostic_code(),
            Self::Vault(error) => error.diagnostic_code(),
            Self::Profile(error) => error.diagnostic_code(),
            Self::Thorium(error) => error.diagnostic_code(),
            Self::Otp(error) => error.diagnostic_code(),
            Self::Qr(error) => error.diagnostic_code(),
            Self::Domain(error) => error.diagnostic_code(),
            Self::WorkspaceInUse => "CONTROLLER_WORKSPACE_IN_USE",
            Self::NotFound { .. } => "CONTROLLER_NOT_FOUND",
            Self::NoCurrentThorium => "CONTROLLER_NO_CURRENT_THORIUM",
        }
    }
}

impl ControllerError {
    /// Message safe for frontend display (never contains secrets; this
    /// error tree never embeds secret material by construction).
    pub fn user_message(&self) -> String {
        self.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_codes_delegate_and_are_stable() {
        assert_eq!(
            ControllerError::WorkspaceInUse.diagnostic_code(),
            "CONTROLLER_WORKSPACE_IN_USE"
        );
        let wrapped = ControllerError::Vault(thorium_workspace_vault::VaultError::Locked);
        assert_eq!(wrapped.diagnostic_code(), "VAULT_LOCKED");
        let rendered = format!("{wrapped}");
        assert!(
            !rendered.contains("password"),
            "no secret material expected"
        );
    }
}
