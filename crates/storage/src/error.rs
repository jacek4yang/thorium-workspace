//! Typed storage errors with stable diagnostic codes.
//!
//! Error text never contains secret material: storage only handles
//! non-secret metadata, and paths are shown because they are needed to
//! diagnose portable-workspace problems.

use std::path::PathBuf;

use thorium_workspace_domain::DiagnosticCode;

/// Error type for all persistence failures.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// The database file could not be opened or created.
    #[error("failed to open workspace database at {path}: {source}")]
    Open {
        /// Database file path.
        path: PathBuf,
        /// Underlying SQLite error.
        source: rusqlite::Error,
    },

    /// A parent directory of the database file could not be created.
    #[error("failed to create directory {path}: {source}")]
    CreateDir {
        /// Directory path.
        path: PathBuf,
        /// Underlying filesystem error.
        source: std::io::Error,
    },

    /// A schema migration failed.
    #[error("database migration to version {version} failed: {source}")]
    Migration {
        /// Target schema version.
        version: i64,
        /// Underlying SQLite error.
        source: rusqlite::Error,
    },

    /// The database was written by a newer application version.
    #[error(
        "database schema version {database} is newer than this application supports ({application})"
    )]
    SchemaTooNew {
        /// Schema version found in the database.
        database: i64,
        /// Highest schema version this application understands.
        application: i64,
    },

    /// A SQLite operation failed.
    #[error("database operation failed: {0}")]
    Sql(#[from] rusqlite::Error),

    /// A stored row violated the record format.
    #[error("stored record is corrupt ({column}): {detail}")]
    Corrupt {
        /// Table.column that failed to decode.
        column: &'static str,
        /// Why the value was rejected.
        detail: String,
    },

    /// A referenced row did not exist.
    #[error("{entity} not found")]
    NotFound {
        /// Human name of the missing entity (e.g. "profile").
        entity: &'static str,
    },

    /// A uniqueness constraint was violated.
    #[error("{entity} {field} already exists")]
    Conflict {
        /// Human name of the entity being written.
        entity: &'static str,
        /// Human name of the conflicting field.
        field: &'static str,
    },

    /// Serializing a persisted value failed.
    #[error("serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl DiagnosticCode for StorageError {
    fn diagnostic_code(&self) -> &'static str {
        match self {
            Self::Open { .. } => "STORAGE_OPEN_FAILED",
            Self::CreateDir { .. } => "STORAGE_CREATE_DIR_FAILED",
            Self::Migration { .. } => "STORAGE_MIGRATION_FAILED",
            Self::SchemaTooNew { .. } => "STORAGE_SCHEMA_TOO_NEW",
            Self::Sql(_) => "STORAGE_SQL_ERROR",
            Self::Corrupt { .. } => "STORAGE_CORRUPT",
            Self::NotFound { .. } => "STORAGE_NOT_FOUND",
            Self::Conflict { .. } => "STORAGE_CONFLICT",
            Self::Serialization(_) => "STORAGE_SERIALIZATION",
        }
    }
}

/// Maps a failed INSERT/UPDATE to a storage error, translating SQLite
/// uniqueness violations into [`StorageError::Conflict`] where the column
/// can be identified.
pub(crate) fn map_write_error(err: rusqlite::Error, entity: &'static str) -> StorageError {
    if let rusqlite::Error::SqliteFailure(failure, Some(message)) = &err {
        if failure.code == rusqlite::ErrorCode::ConstraintViolation {
            if let Some(columns) = message.split("UNIQUE constraint failed: ").nth(1) {
                // A violation may span several columns (e.g. the
                // (account_id, position) pair); pick the most specific
                // user-facing field among them.
                let fields: Vec<&str> = columns
                    .split(',')
                    .map(str::trim)
                    .map(|column| column.rsplit('.').next().unwrap_or(column))
                    .collect();
                let pick = |wanted: &str| fields.contains(&wanted);
                let field = if pick("name") {
                    "name"
                } else if pick("user_data_rel_path") {
                    "user_data_rel_path"
                } else if pick("position") {
                    "position"
                } else {
                    "value"
                };
                return StorageError::Conflict { entity, field };
            }
            return StorageError::Conflict {
                entity,
                field: "value",
            };
        }
    }
    StorageError::Sql(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_violation(message: &str) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
            Some(message.to_owned()),
        )
    }

    #[test]
    fn unique_violations_map_to_conflict_by_column() {
        let single = map_write_error(
            unique_violation("UNIQUE constraint failed: profiles.name"),
            "profile",
        );
        assert!(matches!(
            single,
            StorageError::Conflict { field: "name", .. }
        ));

        let composite = map_write_error(
            unique_violation(
                "UNIQUE constraint failed: recovery_codes.account_id, recovery_codes.position",
            ),
            "recovery code",
        );
        assert!(matches!(
            composite,
            StorageError::Conflict {
                field: "position",
                ..
            }
        ));
    }

    #[test]
    fn diagnostic_codes_are_stable() {
        assert_eq!(
            StorageError::NotFound { entity: "profile" }.diagnostic_code(),
            "STORAGE_NOT_FOUND"
        );
        assert_eq!(
            StorageError::SchemaTooNew {
                database: 99,
                application: 1
            }
            .diagnostic_code(),
            "STORAGE_SCHEMA_TOO_NEW"
        );
    }

    #[test]
    fn conflict_errors_do_not_embed_values() {
        let error = StorageError::Conflict {
            entity: "profile",
            field: "name",
        };
        let rendered = format!("{error}");
        assert_eq!(rendered, "profile name already exists");
    }
}
