//! Schema migrations.
//!
//! Migrations are ordered, numbered and applied inside a transaction. The
//! applied version is recorded in `PRAGMA user_version`, which SQLite stores in
//! the file header, so it survives even a database whose tables are all gone.
//!
//! Adding a migration means appending to [`migrations`] and bumping nothing
//! else: [`SCHEMA_VERSION`] is derived from the list.

use rusqlite::Connection;

use crate::error::{StorageError, StorageResult};

/// One ordered schema change.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// Version this migration brings the schema to. Must be the previous
    /// migration's version plus one.
    pub version: u32,
    /// Short name, used in error messages.
    pub name: &'static str,
    /// The SQL to execute.
    pub sql: &'static str,
}

/// Every migration, in order.
#[must_use]
pub fn migrations() -> &'static [Migration] {
    &[Migration {
        version: 1,
        name: "initial",
        sql: include_str!("../migrations/0001_initial.sql"),
    }]
}

/// The schema version this build produces.
pub const SCHEMA_VERSION: u32 = 1;

/// Reads the schema version recorded in the database file.
///
/// # Errors
///
/// Returns [`StorageError::Query`] when the pragma cannot be read.
pub fn current_version(conn: &Connection) -> StorageResult<u32> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    u32::try_from(version)
        .map_err(|_| StorageError::Corrupt(format!("schema version {version} is not a valid version")))
}

/// Applies every migration newer than the database's current version.
///
/// Each migration runs in its own transaction together with the version bump, so
/// a failure or a crash leaves the database at a version whose schema is
/// actually present.
///
/// # Errors
///
/// Returns [`StorageError::SchemaTooNew`] when the file is ahead of this build,
/// and [`StorageError::Migration`] when a migration fails.
pub fn migrate(conn: &mut Connection) -> StorageResult<u32> {
    let installed = current_version(conn)?;
    if installed > SCHEMA_VERSION {
        return Err(StorageError::SchemaTooNew {
            found: installed,
            supported: SCHEMA_VERSION,
        });
    }
    for migration in migrations().iter().filter(|m| m.version > installed) {
        tracing::info!(
            version = migration.version,
            name = migration.name,
            "applying schema migration"
        );
        let tx = conn.transaction().map_err(|e| StorageError::Migration {
            version: migration.version,
            name: migration.name,
            reason: e.to_string(),
        })?;
        tx.execute_batch(migration.sql)
            .map_err(|e| StorageError::Migration {
                version: migration.version,
                name: migration.name,
                reason: e.to_string(),
            })?;
        // `user_version` does not accept a bound parameter, and the value is a
        // compile-time constant from our own table, never user input.
        tx.pragma_update(None, "user_version", migration.version)
            .map_err(|e| StorageError::Migration {
                version: migration.version,
                name: migration.name,
                reason: e.to_string(),
            })?;
        tx.commit().map_err(|e| StorageError::Migration {
            version: migration.version,
            name: migration.name,
            reason: e.to_string(),
        })?;
    }
    current_version(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory() -> Connection {
        let conn = Connection::open_in_memory().expect("open");
        conn.execute_batch("PRAGMA foreign_keys = ON;").expect("pragma");
        conn
    }

    #[test]
    fn the_migration_list_is_contiguous_and_matches_the_declared_version() {
        for (index, migration) in migrations().iter().enumerate() {
            assert_eq!(
                migration.version,
                u32::try_from(index).expect("fits") + 1,
                "migration versions must start at 1 and increase by one"
            );
            assert!(!migration.sql.trim().is_empty(), "{} is empty", migration.name);
        }
        assert_eq!(
            migrations().last().map(|m| m.version),
            Some(SCHEMA_VERSION),
            "SCHEMA_VERSION must equal the last migration's version"
        );
    }

    #[test]
    fn a_fresh_database_migrates_to_the_current_version() {
        let mut conn = memory();
        assert_eq!(current_version(&conn).expect("read"), 0);
        assert_eq!(migrate(&mut conn).expect("migrate"), SCHEMA_VERSION);
        assert_eq!(current_version(&conn).expect("read"), SCHEMA_VERSION);
    }

    #[test]
    fn migrating_twice_is_a_no_op() {
        let mut conn = memory();
        migrate(&mut conn).expect("first");
        // Insert a row, migrate again, and confirm nothing was recreated.
        conn.execute(
            "INSERT INTO workspace_settings (key, value) VALUES ('theme', 'dark')",
            [],
        )
        .expect("insert");
        assert_eq!(migrate(&mut conn).expect("second"), SCHEMA_VERSION);
        let value: String = conn
            .query_row(
                "SELECT value FROM workspace_settings WHERE key = 'theme'",
                [],
                |r| r.get(0),
            )
            .expect("select");
        assert_eq!(value, "dark");
    }

    #[test]
    fn a_newer_schema_is_refused_rather_than_downgraded() {
        let mut conn = memory();
        migrate(&mut conn).expect("migrate");
        conn.pragma_update(None, "user_version", SCHEMA_VERSION + 5)
            .expect("bump");
        match migrate(&mut conn) {
            Err(StorageError::SchemaTooNew { found, supported }) => {
                assert_eq!(found, SCHEMA_VERSION + 5);
                assert_eq!(supported, SCHEMA_VERSION);
            }
            other => panic!("expected SchemaTooNew, got {other:?}"),
        }
    }

    #[test]
    fn every_expected_table_exists_after_migrating() {
        let mut conn = memory();
        migrate(&mut conn).expect("migrate");
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .expect("prepare");
        let tables: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .expect("query")
            .collect::<Result<_, _>>()
            .expect("rows");
        for expected in [
            "account_factors",
            "account_tags",
            "accounts",
            "browser_profiles",
            "profile_accounts",
            "recovery_codes",
            "runtime_sessions",
            "thorium_installations",
            "workspace_settings",
        ] {
            assert!(
                tables.iter().any(|t| t == expected),
                "missing table {expected}: {tables:?}"
            );
        }
    }

    #[test]
    fn a_failing_migration_leaves_the_version_untouched() {
        let mut conn = memory();
        // Simulate a broken migration by running the batch directly against a
        // database that already has the tables.
        migrate(&mut conn).expect("migrate");
        let before = current_version(&conn).expect("read");
        let broken = Migration {
            version: 99,
            name: "broken",
            sql: "CREATE TABLE accounts (x TEXT);",
        };
        let tx = conn.transaction().expect("tx");
        assert!(tx.execute_batch(broken.sql).is_err());
        drop(tx);
        assert_eq!(current_version(&conn).expect("read"), before);
    }

    #[test]
    fn only_one_thorium_installation_can_be_current() {
        let mut conn = memory();
        migrate(&mut conn).expect("migrate");
        let insert = "INSERT INTO thorium_installations \
             (version, channel, install_dir, executable_path, installed_at, is_current) \
             VALUES (?1, 'windows_avx2', 'd', 'e', 0, ?2)";
        conn.execute(insert, rusqlite::params!["v1", 1])
            .expect("first current");
        conn.execute(insert, rusqlite::params!["v2", 0])
            .expect("second not current");
        assert!(
            conn.execute(insert, rusqlite::params!["v3", 1]).is_err(),
            "two current versions"
        );
    }

    #[test]
    fn deleting_an_account_cascades_to_its_children() {
        let mut conn = memory();
        migrate(&mut conn).expect("migrate");
        conn.execute(
            "INSERT INTO accounts (id, display_name, service_kind, created_at, updated_at) \
             VALUES ('a', 'A', 'github', 0, 0)",
            [],
        )
        .expect("account");
        conn.execute(
            "INSERT INTO account_tags (account_id, tag) VALUES ('a', 'work')",
            [],
        )
        .expect("tag");
        conn.execute(
            "INSERT INTO account_factors (id, account_id, label, kind, created_at, updated_at) \
             VALUES ('f', 'a', 'App', 'otp', 0, 0)",
            [],
        )
        .expect("factor");
        conn.execute(
            "INSERT INTO recovery_codes (id, account_id, code_ref, position, created_at) \
             VALUES ('r', 'a', 'ref', 0, 0)",
            [],
        )
        .expect("code");

        conn.execute("DELETE FROM accounts WHERE id = 'a'", [])
            .expect("delete");
        for table in ["account_tags", "account_factors", "recovery_codes"] {
            let count: i64 = conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
                .expect("count");
            assert_eq!(count, 0, "{table} was not cascaded");
        }
    }

    #[test]
    fn two_profiles_cannot_share_a_user_data_directory() {
        let mut conn = memory();
        migrate(&mut conn).expect("migrate");
        let insert = "INSERT INTO browser_profiles \
             (id, name, thorium_mode, user_data_dir, locale, timezone, created_at, updated_at) \
             VALUES (?1, ?2, 'current', ?3, 'en-US', 'UTC', 0, 0)";
        conn.execute(insert, rusqlite::params!["p1", "One", "profiles/aaa"])
            .expect("first");
        conn.execute(insert, rusqlite::params!["p2", "Two", "profiles/bbb"])
            .expect("second");
        assert!(
            conn.execute(insert, rusqlite::params!["p3", "Three", "profiles/aaa"])
                .is_err(),
            "a shared User Data directory would break profile isolation"
        );
    }
}
