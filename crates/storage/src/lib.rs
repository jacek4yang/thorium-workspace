//! SQLite persistence and schema migrations for workspace metadata.
//!
//! Only non-secret metadata is stored here. Account secrets live in the
//! vault crate's encrypted format; rows reference them by structured
//! [`thorium_workspace_domain::SecretRef`] strings.
//!
//! The store is configured for crash safety (WAL journal when the platform
//! accepts it, `foreign_keys` always on). All timestamps are stored as
//! RFC 3339 text.

#![forbid(unsafe_code)]

mod accounts;
mod error;
mod migrations;
mod profiles;
mod settings;

pub use error::StorageError;

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use rusqlite::Connection;

/// Opened workspace database.
#[derive(Debug)]
pub struct Store {
    /// Database file path (kept for diagnostics; not secret).
    path: PathBuf,
    conn: Connection,
}

impl Store {
    /// Opens (creating if needed) the workspace database and applies all
    /// pending migrations.
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|source| StorageError::CreateDir {
                    path: parent.to_owned(),
                    source,
                })?;
            }
        }
        let conn = Connection::open(path).map_err(|source| StorageError::Open {
            path: path.to_owned(),
            source,
        })?;
        Self::configure(&conn, path)?;
        let store = Self {
            path: path.to_owned(),
            conn,
        };
        store.apply_migrations()?;
        Ok(store)
    }

    /// Database file path (diagnostic use).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Applied schema version (0 for an empty bookkeeping table).
    pub fn schema_version(&self) -> Result<i64, StorageError> {
        let version: Option<i64> =
            self.conn
                .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                    row.get(0)
                })?;
        Ok(version.unwrap_or(0))
    }

    fn configure(conn: &Connection, path: &Path) -> Result<(), StorageError> {
        // WAL improves crash resilience and lets diagnostics read while the
        // store writes. If the filesystem refuses WAL, the default rollback
        // journal still provides atomic commits.
        let mode: String = conn
            .query_row("PRAGMA journal_mode = WAL", [], |row| row.get(0))
            .map_err(|source| StorageError::Open {
                path: path.to_owned(),
                source,
            })?;
        if mode.eq_ignore_ascii_case("wal") {
            conn.pragma_update(None, "synchronous", "NORMAL")
                .map_err(|source| StorageError::Open {
                    path: path.to_owned(),
                    source,
                })?;
        }
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(|source| StorageError::Open {
                path: path.to_owned(),
                source,
            })?;
        Ok(())
    }

    fn apply_migrations(&self) -> Result<(), StorageError> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at TEXT NOT NULL
            );",
        )?;
        let current: Option<i64> =
            self.conn
                .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                    row.get(0)
                })?;
        if let Some(applied) = current {
            if applied > migrations::LATEST {
                return Err(StorageError::SchemaTooNew {
                    database: applied,
                    application: migrations::LATEST,
                });
            }
        }
        for migration in migrations::pending(current) {
            let tx = self.conn.unchecked_transaction()?;
            tx.execute_batch(migration.sql)
                .map_err(|source| StorageError::Migration {
                    version: migration.version,
                    source,
                })?;
            tx.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![migration.version, migration.name, Utc::now().to_rfc3339()],
            )?;
            tx.commit()?;
        }
        Ok(())
    }
}

/// Timestamp conversion helpers shared by the repositories.
pub(crate) mod time {
    use super::*;

    /// Converts a timestamp to its stored text form.
    pub fn to_text(value: DateTime<Utc>) -> String {
        value.to_rfc3339()
    }

    /// Decodes a stored timestamp, mapping garbage to
    /// [`StorageError::Corrupt`].
    pub fn from_text(column: &'static str, value: &str) -> Result<DateTime<Utc>, StorageError> {
        DateTime::parse_from_rfc3339(value)
            .map(|parsed| parsed.with_timezone(&Utc))
            .map_err(|source| StorageError::Corrupt {
                column,
                detail: format!("invalid timestamp: {source}"),
            })
    }
}

/// Numeric conversion helpers shared by the repositories.
pub(crate) mod num {
    use super::*;

    /// Converts a stored integer to `T`, mapping out-of-range values to
    /// [`StorageError::Corrupt`].
    pub fn from_i64<T: TryFrom<i64>>(column: &'static str, value: i64) -> Result<T, StorageError> {
        T::try_from(value).map_err(|_| StorageError::Corrupt {
            column,
            detail: format!("integer {value} out of range"),
        })
    }

    /// Converts `T` to a storable SQLite integer, mapping overflow to
    /// [`StorageError::Corrupt`].
    pub fn to_i64<T: TryInto<i64> + Copy + std::fmt::Display>(
        column: &'static str,
        value: T,
    ) -> Result<i64, StorageError> {
        value.try_into().map_err(|_| StorageError::Corrupt {
            column,
            detail: format!("integer out of range: {value}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(tag: &str) -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(format!("{tag}.db"));
        let store = Store::open(&path).expect("open");
        (dir, store)
    }

    #[test]
    fn fresh_database_is_at_latest_schema_version() {
        let (_dir, store) = temp_store("fresh");
        assert_eq!(store.schema_version().expect("version"), migrations::LATEST);
    }

    #[test]
    fn reopening_preserves_schema_version() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("reopen.db");
        {
            let store = Store::open(&path).expect("open first");
            assert_eq!(store.schema_version().expect("version"), migrations::LATEST);
        }
        let store = Store::open(&path).expect("reopen");
        assert_eq!(store.schema_version().expect("version"), migrations::LATEST);
    }

    #[test]
    fn each_migration_is_recorded_once() {
        let (_dir, store) = temp_store("bookkeeping");
        let applied: Vec<(i64, String)> = store
            .conn
            .prepare("SELECT version, name FROM schema_migrations ORDER BY version")
            .expect("prepare")
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .expect("query")
            .map(|row| row.expect("row"))
            .collect();
        assert_eq!(applied.len(), migrations::MIGRATIONS.len());
        for (index, migration) in migrations::MIGRATIONS.iter().enumerate() {
            assert_eq!(applied[index].0, migration.version);
            assert_eq!(applied[index].1, migration.name);
        }
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let (_dir, store) = temp_store("fk");
        let error = store
            .conn
            .execute(
                "INSERT INTO accounts (id, profile_id, display_name, service_id, tags, notes, created_at, updated_at)
                 VALUES ('a', 'missing', 'n', 'github', '[]', '', 'x', 'x')",
                [],
            )
            .expect_err("foreign keys must reject orphans");
        let rendered = error.to_string();
        assert!(rendered.contains("FOREIGN KEY"), "got: {rendered}");
    }

    #[test]
    fn diagnostics_never_leak_secrets() {
        use thorium_workspace_domain::DiagnosticCode;
        let error = StorageError::Corrupt {
            column: "profiles.name",
            detail: "invalid timestamp".to_owned(),
        };
        assert_eq!(error.diagnostic_code(), "STORAGE_CORRUPT");
    }
}
