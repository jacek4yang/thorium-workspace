//! Installed Thorium versions.

use rusqlite::{Connection, OptionalExtension, Row, params};
use tw_domain::{ThoriumChannel, ThoriumInstallation, Timestamp};

use crate::error::{StorageError, StorageResult};

/// Reads and writes Thorium installation records.
pub struct ThoriumRepo;

const SELECT: &str = "SELECT version, channel, install_dir, executable_path, installed_at, source_url, \
                      archive_sha256, is_current FROM thorium_installations";

impl ThoriumRepo {
    /// Records an installed version, replacing any record with the same version.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a write failure.
    pub fn upsert(conn: &Connection, install: &ThoriumInstallation) -> StorageResult<()> {
        conn.execute(
            "INSERT INTO thorium_installations \
             (version, channel, install_dir, executable_path, installed_at, source_url, archive_sha256, is_current) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
             ON CONFLICT (version) DO UPDATE SET channel = excluded.channel, \
             install_dir = excluded.install_dir, executable_path = excluded.executable_path, \
             installed_at = excluded.installed_at, source_url = excluded.source_url, \
             archive_sha256 = excluded.archive_sha256",
            params![
                install.version,
                install.channel.as_str(),
                install.install_dir,
                install.executable_path,
                install.installed_at.as_unix_seconds(),
                install.source_url,
                install.archive_sha256,
                i64::from(install.is_current),
            ],
        )?;
        Ok(())
    }

    /// Removes an installation record. Does not touch files on disk.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] when no such version is recorded.
    pub fn delete(conn: &Connection, version: &str) -> StorageResult<()> {
        let changed = conn.execute(
            "DELETE FROM thorium_installations WHERE version = ?1",
            params![version],
        )?;
        if changed == 0 {
            return Err(StorageError::not_found("Thorium installation", version));
        }
        Ok(())
    }

    /// Fetches one installation record.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] when no such version is recorded.
    pub fn get(conn: &Connection, version: &str) -> StorageResult<ThoriumInstallation> {
        conn.query_row(
            &format!("{SELECT} WHERE version = ?1"),
            params![version],
            map_install,
        )
        .optional()?
        .ok_or_else(|| StorageError::not_found("Thorium installation", version))
    }

    /// Lists installations, newest install first.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn list(conn: &Connection) -> StorageResult<Vec<ThoriumInstallation>> {
        let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY installed_at DESC, version DESC"))?;
        let rows = stmt.query_map([], map_install)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// The version currently promoted to `current`, if any.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn current(conn: &Connection) -> StorageResult<Option<ThoriumInstallation>> {
        Ok(conn
            .query_row(&format!("{SELECT} WHERE is_current = 1"), [], map_install)
            .optional()?)
    }

    /// Promotes `version` to `current`, demoting whatever held it.
    ///
    /// Both statements run in one transaction: a partial application would leave
    /// either no current version or two, and the schema forbids the latter.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] when no such version is recorded.
    pub fn set_current(conn: &mut Connection, version: &str) -> StorageResult<()> {
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE thorium_installations SET is_current = 0 WHERE is_current = 1",
            [],
        )?;
        let changed = tx.execute(
            "UPDATE thorium_installations SET is_current = 1 WHERE version = ?1",
            params![version],
        )?;
        if changed == 0 {
            return Err(StorageError::not_found("Thorium installation", version));
        }
        tx.commit()?;
        Ok(())
    }

    /// The version installed most recently that is not `current`.
    ///
    /// This is the rollback target: the previous known-good build.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn previous_known_good(conn: &Connection) -> StorageResult<Option<ThoriumInstallation>> {
        Ok(conn
            .query_row(
                &format!("{SELECT} WHERE is_current = 0 ORDER BY installed_at DESC, version DESC LIMIT 1"),
                [],
                map_install,
            )
            .optional()?)
    }
}

fn map_install(row: &Row<'_>) -> rusqlite::Result<ThoriumInstallation> {
    let channel: String = row.get(1)?;
    let is_current: i64 = row.get(7)?;
    Ok(ThoriumInstallation {
        version: row.get(0)?,
        channel: ThoriumChannel::parse(&channel).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                "unknown Thorium channel".into(),
            )
        })?,
        install_dir: row.get(2)?,
        executable_path: row.get(3)?,
        installed_at: Timestamp::from_unix_seconds(row.get(4)?),
        source_url: row.get(5)?,
        archive_sha256: row.get(6)?,
        is_current: is_current != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    fn install(version: &str, installed_at: i64, is_current: bool) -> ThoriumInstallation {
        ThoriumInstallation {
            version: version.to_owned(),
            channel: ThoriumChannel::WindowsAvx2,
            install_dir: format!("browsers/thorium/versions/{version}"),
            executable_path: format!("browsers/thorium/versions/{version}/BIN/thorium.exe"),
            installed_at: Timestamp::from_unix_seconds(installed_at),
            source_url: format!("https://example.test/{version}.zip"),
            archive_sha256: "0".repeat(64),
            is_current,
        }
    }

    #[test]
    fn installations_round_trip() {
        let db = Database::open_in_memory().expect("open");
        let original = install("M152.0.7977.55", 1_700_000_000, true);
        ThoriumRepo::upsert(db.connection(), &original).expect("upsert");
        assert_eq!(
            ThoriumRepo::get(db.connection(), &original.version).expect("get"),
            original
        );
    }

    #[test]
    fn promotion_demotes_the_previous_current_version() {
        let mut db = Database::open_in_memory().expect("open");
        ThoriumRepo::upsert(db.connection(), &install("v1", 100, true)).expect("upsert");
        ThoriumRepo::upsert(db.connection(), &install("v2", 200, false)).expect("upsert");
        assert_eq!(
            ThoriumRepo::current(db.connection())
                .expect("current")
                .map(|i| i.version),
            Some("v1".into())
        );

        ThoriumRepo::set_current(db.connection_mut(), "v2").expect("promote");
        assert_eq!(
            ThoriumRepo::current(db.connection())
                .expect("current")
                .map(|i| i.version),
            Some("v2".into())
        );
        assert!(!ThoriumRepo::get(db.connection(), "v1").expect("get").is_current);
    }

    #[test]
    fn promoting_a_missing_version_is_refused_and_leaves_the_current_one_alone() {
        let mut db = Database::open_in_memory().expect("open");
        ThoriumRepo::upsert(db.connection(), &install("v1", 100, true)).expect("upsert");
        assert!(matches!(
            ThoriumRepo::set_current(db.connection_mut(), "v9"),
            Err(StorageError::NotFound { .. })
        ));
        assert_eq!(
            ThoriumRepo::current(db.connection())
                .expect("current")
                .map(|i| i.version),
            Some("v1".into()),
            "a refused promotion must not leave the workspace with no current version"
        );
    }

    #[test]
    fn the_previous_known_good_version_is_the_newest_non_current_one() {
        let mut db = Database::open_in_memory().expect("open");
        ThoriumRepo::upsert(db.connection(), &install("v1", 100, false)).expect("upsert");
        ThoriumRepo::upsert(db.connection(), &install("v2", 200, false)).expect("upsert");
        ThoriumRepo::upsert(db.connection(), &install("v3", 300, false)).expect("upsert");
        ThoriumRepo::set_current(db.connection_mut(), "v3").expect("promote");
        assert_eq!(
            ThoriumRepo::previous_known_good(db.connection())
                .expect("previous")
                .map(|i| i.version),
            Some("v2".into())
        );
    }

    #[test]
    fn a_reinstall_updates_in_place_without_changing_which_version_is_current() {
        let mut db = Database::open_in_memory().expect("open");
        ThoriumRepo::upsert(db.connection(), &install("v1", 100, false)).expect("upsert");
        ThoriumRepo::set_current(db.connection_mut(), "v1").expect("promote");
        let mut updated = install("v1", 500, false);
        updated.archive_sha256 = "a".repeat(64);
        ThoriumRepo::upsert(db.connection(), &updated).expect("re-upsert");
        let loaded = ThoriumRepo::get(db.connection(), "v1").expect("get");
        assert_eq!(loaded.archive_sha256, "a".repeat(64));
        assert!(
            loaded.is_current,
            "an in-place reinstall must not demote the current version"
        );
        assert_eq!(ThoriumRepo::list(db.connection()).expect("list").len(), 1);
    }

    #[test]
    fn a_fresh_workspace_has_no_current_version() {
        let db = Database::open_in_memory().expect("open");
        assert!(ThoriumRepo::current(db.connection()).expect("current").is_none());
        assert!(
            ThoriumRepo::previous_known_good(db.connection())
                .expect("previous")
                .is_none()
        );
        assert!(ThoriumRepo::list(db.connection()).expect("list").is_empty());
    }

    #[test]
    fn missing_installations_are_reported() {
        let db = Database::open_in_memory().expect("open");
        assert!(matches!(
            ThoriumRepo::get(db.connection(), "v9"),
            Err(StorageError::NotFound { .. })
        ));
        assert!(matches!(
            ThoriumRepo::delete(db.connection(), "v9"),
            Err(StorageError::NotFound { .. })
        ));
    }
}
