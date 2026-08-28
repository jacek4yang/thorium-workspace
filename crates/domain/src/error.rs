//! The domain error type.
//!
//! Every error carries a [`DiagnosticCode`] so the UI, logs and copied
//! diagnostic reports can refer to the same stable identifier. Error messages
//! are written for the person who has to fix the problem and must never embed
//! secret material.

use crate::diagnostics::DiagnosticCode;

/// Result alias used throughout the workspace.
pub type DomainResult<T> = Result<T, DomainError>;

/// A validation or invariant failure raised by the domain model.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {message}")]
pub struct DomainError {
    /// Stable diagnostic identifier.
    pub code: DiagnosticCode,
    /// Human-readable, secret-free explanation.
    pub message: String,
    /// Optional actionable remediation shown beneath the message in the UI.
    pub remedy: Option<String>,
}

impl DomainError {
    /// Creates an error with the given code and message.
    #[must_use]
    pub fn new(code: DiagnosticCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            remedy: None,
        }
    }

    /// Attaches a remediation hint.
    #[must_use]
    pub fn with_remedy(mut self, remedy: impl Into<String>) -> Self {
        self.remedy = Some(remedy.into());
        self
    }

    /// Shorthand for [`DiagnosticCode::InvalidInput`].
    #[must_use]
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(DiagnosticCode::InvalidInput, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_includes_the_code() {
        let err = DomainError::invalid("name must not be empty");
        assert_eq!(err.to_string(), "TW-0901: name must not be empty");
    }

    #[test]
    fn remedy_is_optional_and_attachable() {
        let err = DomainError::new(
            DiagnosticCode::WorkspaceNotWritable,
            "cannot write beside the executable",
        )
        .with_remedy("Move ThoriumWorkspace.exe to a writable folder.");
        assert_eq!(
            err.remedy.as_deref(),
            Some("Move ThoriumWorkspace.exe to a writable folder.")
        );
    }
}
