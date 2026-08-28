//! Storage errors.

use tw_domain::{DiagnosticCode, DomainError};

/// Storage result alias.
pub type StorageResult<T> = Result<T, StorageError>;

/// Failures raised by the metadata database.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StorageError {
    /// The database file could not be opened or configured.
    #[error("the workspace database could not be opened: {0}")]
    Open(String),
    /// A migration failed.
    #[error("schema migration {version} ({name}) failed: {reason}")]
    Migration {
        /// Which migration failed.
        version: u32,
        /// Its name.
        name: &'static str,
        /// Why it failed.
        reason: String,
    },
    /// The database was written by a newer build.
    #[error("this workspace database uses schema version {found}, but this build understands {supported}")]
    SchemaTooNew {
        /// The version found on disk.
        found: u32,
        /// The newest version this build knows.
        supported: u32,
    },
    /// A query or statement failed.
    #[error("a database operation failed: {0}")]
    Query(String),
    /// The requested record does not exist.
    #[error("{entity} {id} does not exist")]
    NotFound {
        /// What kind of record was requested.
        entity: &'static str,
        /// Its identifier.
        id: String,
    },
    /// A uniqueness or referential rule was violated.
    #[error("{0}")]
    Conflict(String),
    /// A stored row could not be mapped back into a domain type.
    #[error("a stored record is not valid: {0}")]
    Corrupt(String),
}

impl StorageError {
    /// The stable diagnostic code for this error.
    #[must_use]
    pub const fn code(&self) -> DiagnosticCode {
        match self {
            Self::Open(_) => DiagnosticCode::StorageOpenFailed,
            Self::Migration { .. } => DiagnosticCode::StorageMigrationFailed,
            Self::SchemaTooNew { .. } => DiagnosticCode::StorageSchemaTooNew,
            Self::Query(_) | Self::Corrupt(_) => DiagnosticCode::StorageQueryFailed,
            Self::NotFound { .. } => DiagnosticCode::RecordNotFound,
            Self::Conflict(_) => DiagnosticCode::RecordConflict,
        }
    }

    pub(crate) fn not_found(entity: &'static str, id: impl std::fmt::Display) -> Self {
        Self::NotFound {
            entity,
            id: id.to_string(),
        }
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(value: rusqlite::Error) -> Self {
        // Constraint violations are the caller's problem to report differently
        // from a genuine failure, so they are separated here rather than at
        // every call site.
        match &value {
            rusqlite::Error::SqliteFailure(e, message)
                if e.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                Self::Conflict(
                    message
                        .clone()
                        .unwrap_or_else(|| "a uniqueness rule was violated".to_owned()),
                )
            }
            _ => Self::Query(value.to_string()),
        }
    }
}

impl From<DomainError> for StorageError {
    fn from(value: DomainError) -> Self {
        Self::Corrupt(value.to_string())
    }
}
