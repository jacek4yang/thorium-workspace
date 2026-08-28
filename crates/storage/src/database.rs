//! The database handle.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::error::{StorageError, StorageResult};
use crate::migrations;

/// How to open a database.
#[derive(Debug, Clone)]
pub struct DatabaseOptions {
    /// Run migrations on open.
    pub migrate: bool,
    /// How long a busy database is waited on before giving up.
    pub busy_timeout: std::time::Duration,
}

impl Default for DatabaseOptions {
    fn default() -> Self {
        Self {
            migrate: true,
            busy_timeout: std::time::Duration::from_secs(5),
        }
    }
}

/// An open metadata database.
///
/// Wraps a single connection. The workspace is single-process (enforced by the
/// workspace mutex), so a connection pool would add contention without buying
/// anything.
pub struct Database {
    conn: Connection,
    path: PathBuf,
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Database {
    /// Opens (creating if needed) the database at `path` and migrates it.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Open`] when the file cannot be opened or
    /// configured, and propagates migration failures.
    pub fn open(path: impl AsRef<Path>, options: &DatabaseOptions) -> StorageResult<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| StorageError::Open(format!("{} could not be created: {e}", parent.display())))?;
        }
        let conn = Connection::open(&path).map_err(|e| StorageError::Open(e.to_string()))?;
        let mut db = Self { conn, path };
        db.configure(options)?;
        if options.migrate {
            migrations::migrate(&mut db.conn)?;
        }
        Ok(db)
    }

    /// Opens an in-memory database. Tests only; nothing persists.
    ///
    /// # Errors
    ///
    /// Propagates configuration and migration failures.
    pub fn open_in_memory() -> StorageResult<Self> {
        let conn = Connection::open_in_memory().map_err(|e| StorageError::Open(e.to_string()))?;
        let mut db = Self {
            conn,
            path: PathBuf::from(":memory:"),
        };
        db.configure(&DatabaseOptions::default())?;
        migrations::migrate(&mut db.conn)?;
        Ok(db)
    }

    fn configure(&mut self, options: &DatabaseOptions) -> StorageResult<()> {
        self.conn
            .busy_timeout(options.busy_timeout)
            .map_err(|e| StorageError::Open(format!("busy timeout could not be set: {e}")))?;
        // WAL survives an abrupt process exit far better than the rollback
        // journal, which matters for a portable app users close by killing it.
        // NORMAL synchronous is safe under WAL: a crash cannot corrupt the
        // database, only lose the most recent transaction, and every write here
        // is idempotent from the user's point of view.
        self.conn
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA foreign_keys = ON;
                 PRAGMA temp_store = MEMORY;",
            )
            .map_err(|e| StorageError::Open(format!("connection pragmas could not be applied: {e}")))?;
        Ok(())
    }

    /// The database file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The applied schema version.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] when the pragma cannot be read.
    pub fn schema_version(&self) -> StorageResult<u32> {
        migrations::current_version(&self.conn)
    }

    /// Borrows the underlying connection.
    #[must_use]
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Mutably borrows the underlying connection, for transactions.
    #[must_use]
    pub fn connection_mut(&mut self) -> &mut Connection {
        &mut self.conn
    }

    /// Runs `f` inside a transaction, committing on `Ok` and rolling back on
    /// `Err`.
    ///
    /// # Errors
    ///
    /// Propagates whatever `f` returns, plus transaction failures.
    pub fn transaction<T>(
        &mut self,
        f: impl FnOnce(&rusqlite::Transaction<'_>) -> StorageResult<T>,
    ) -> StorageResult<T> {
        let tx = self.conn.transaction()?;
        let value = f(&tx)?;
        tx.commit()?;
        Ok(value)
    }

    /// Runs SQLite's own integrity check.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] when the check cannot run.
    pub fn integrity_check(&self) -> StorageResult<String> {
        let result: String = self
            .conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        Ok(result)
    }

    /// Writes a consistent snapshot of the database to `destination`.
    ///
    /// Uses SQLite's online backup API rather than copying the file, which would
    /// race with the WAL.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] when the backup fails.
    pub fn backup_to(&self, destination: &Path) -> StorageResult<()> {
        if let Some(parent) = destination.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| {
                StorageError::Query(format!("{} could not be created: {e}", parent.display()))
            })?;
        }
        self.conn
            .backup(rusqlite::MAIN_DB, destination, None)
            .map_err(|e| StorageError::Query(format!("the database backup failed: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_creates_the_file_and_its_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("workspace.db");
        let db = Database::open(&path, &DatabaseOptions::default()).expect("open");
        assert!(path.is_file());
        assert_eq!(db.schema_version().expect("version"), migrations::SCHEMA_VERSION);
    }

    #[test]
    fn state_survives_a_close_and_reopen() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("workspace.db");
        {
            let db = Database::open(&path, &DatabaseOptions::default()).expect("open");
            db.connection()
                .execute(
                    "INSERT INTO workspace_settings (key, value) VALUES ('theme', 'dark')",
                    [],
                )
                .expect("insert");
        }
        let db = Database::open(&path, &DatabaseOptions::default()).expect("reopen");
        let value: String = db
            .connection()
            .query_row(
                "SELECT value FROM workspace_settings WHERE key = 'theme'",
                [],
                |r| r.get(0),
            )
            .expect("select");
        assert_eq!(value, "dark");
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let db = Database::open_in_memory().expect("open");
        let err = db.connection().execute(
            "INSERT INTO account_tags (account_id, tag) VALUES ('missing', 'x')",
            [],
        );
        assert!(err.is_err(), "a dangling foreign key must be rejected");
    }

    #[test]
    fn a_failing_transaction_rolls_back() {
        let mut db = Database::open_in_memory().expect("open");
        let result: StorageResult<()> = db.transaction(|tx| {
            tx.execute(
                "INSERT INTO workspace_settings (key, value) VALUES ('a', '1')",
                [],
            )?;
            Err(StorageError::Conflict("deliberate".to_owned()))
        });
        assert!(result.is_err());
        let count: i64 = db
            .connection()
            .query_row("SELECT COUNT(*) FROM workspace_settings", [], |r| r.get(0))
            .expect("count");
        assert_eq!(count, 0);
    }

    #[test]
    fn the_integrity_check_passes_on_a_fresh_database() {
        let db = Database::open_in_memory().expect("open");
        assert_eq!(db.integrity_check().expect("check"), "ok");
    }

    #[test]
    fn a_backup_is_a_readable_database_with_the_same_contents() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = Database::open(dir.path().join("workspace.db"), &DatabaseOptions::default()).expect("open");
        db.connection()
            .execute(
                "INSERT INTO workspace_settings (key, value) VALUES ('theme', 'light')",
                [],
            )
            .expect("insert");

        let backup = dir.path().join("backups").join("workspace.db");
        db.backup_to(&backup).expect("backup");

        let restored = Database::open(
            &backup,
            &DatabaseOptions {
                migrate: false,
                ..Default::default()
            },
        )
        .expect("open backup");
        let value: String = restored
            .connection()
            .query_row(
                "SELECT value FROM workspace_settings WHERE key = 'theme'",
                [],
                |r| r.get(0),
            )
            .expect("select");
        assert_eq!(value, "light");
        assert_eq!(
            restored.schema_version().expect("version"),
            migrations::SCHEMA_VERSION
        );
    }

    #[test]
    fn the_debug_output_shows_the_path_and_nothing_else() {
        let db = Database::open_in_memory().expect("open");
        let rendered = format!("{db:?}");
        assert!(rendered.contains(":memory:"));
    }
}
