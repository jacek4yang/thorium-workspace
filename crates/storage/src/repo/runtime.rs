//! Observed browser runtime state.
//!
//! Everything in this table is *observed*, never authoritative: it is a cache of
//! what was running when the manager last looked. Startup recovery reconciles it
//! against the real process table, so a stale row from a crash never becomes the
//! only record of anything the user configured.

use rusqlite::{Connection, OptionalExtension, Row, params};
use tw_domain::{ProfileId, ProfileRuntimeStatus, Timestamp};

use crate::error::StorageResult;

/// What was last observed about a profile's browser session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSession {
    /// Which profile.
    pub profile_id: ProfileId,
    /// Last observed status.
    pub status: ProfileRuntimeStatus,
    /// The browser process id, when one was launched.
    pub pid: Option<u32>,
    /// The loopback DevTools port, when one was opened.
    pub cdp_port: Option<u16>,
    /// Which Thorium version was launched.
    pub thorium_version: Option<String>,
    /// When the session started.
    pub started_at: Option<Timestamp>,
    /// When this row was last written.
    pub updated_at: Timestamp,
}

/// Reads and writes observed runtime state.
pub struct RuntimeRepo;

const SELECT: &str = "SELECT profile_id, status, pid, cdp_port, thorium_version, started_at, updated_at \
                      FROM runtime_sessions";

impl RuntimeRepo {
    /// Writes the observed state for a profile.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Conflict`] when the profile does not exist.
    pub fn upsert(conn: &Connection, session: &RuntimeSession) -> StorageResult<()> {
        conn.execute(
            "INSERT INTO runtime_sessions \
             (profile_id, status, pid, cdp_port, thorium_version, started_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
             ON CONFLICT (profile_id) DO UPDATE SET status = excluded.status, pid = excluded.pid, \
             cdp_port = excluded.cdp_port, thorium_version = excluded.thorium_version, \
             started_at = excluded.started_at, updated_at = excluded.updated_at",
            params![
                session.profile_id.to_string(),
                session.status.as_str(),
                session.pid,
                session.cdp_port,
                session.thorium_version,
                session.started_at.map(Timestamp::as_unix_seconds),
                session.updated_at.as_unix_seconds(),
            ],
        )?;
        Ok(())
    }

    /// Reads the observed state for a profile.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn get(conn: &Connection, profile_id: ProfileId) -> StorageResult<Option<RuntimeSession>> {
        Ok(conn
            .query_row(
                &format!("{SELECT} WHERE profile_id = ?1"),
                params![profile_id.to_string()],
                map_session,
            )
            .optional()?)
    }

    /// Lists every observed session.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn list(conn: &Connection) -> StorageResult<Vec<RuntimeSession>> {
        let mut stmt = conn.prepare(SELECT)?;
        let rows = stmt.query_map([], map_session)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Removes the observed state for a profile.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a write failure.
    pub fn clear(conn: &Connection, profile_id: ProfileId) -> StorageResult<()> {
        conn.execute(
            "DELETE FROM runtime_sessions WHERE profile_id = ?1",
            params![profile_id.to_string()],
        )?;
        Ok(())
    }

    /// Marks every session stopped.
    ///
    /// Called at startup before reconciliation: any row that survived a crash
    /// describes a process this manager no longer supervises.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a write failure.
    pub fn clear_all(conn: &Connection) -> StorageResult<usize> {
        Ok(conn.execute("DELETE FROM runtime_sessions", [])?)
    }

    /// The Thorium versions currently in use by an active session.
    ///
    /// Used to refuse deleting a version a running profile depends on.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn versions_in_use(conn: &Connection) -> StorageResult<Vec<String>> {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT thorium_version FROM runtime_sessions \
             WHERE thorium_version IS NOT NULL AND status IN ('starting', 'running', 'stopping')",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

fn map_session(row: &Row<'_>) -> rusqlite::Result<RuntimeSession> {
    let profile_id: String = row.get(0)?;
    let status: String = row.get(1)?;
    let started_at: Option<i64> = row.get(5)?;
    Ok(RuntimeSession {
        profile_id: profile_id.parse().map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                "profile id is not a UUID".into(),
            )
        })?,
        status: status.parse().map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                1,
                rusqlite::types::Type::Text,
                "unknown runtime status".into(),
            )
        })?,
        pid: row.get(2)?,
        cdp_port: row.get(3)?,
        thorium_version: row.get(4)?,
        started_at: started_at.map(Timestamp::from_unix_seconds),
        updated_at: Timestamp::from_unix_seconds(row.get(6)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::profiles::tests::sample_profile;
    use crate::{Database, ProfileRepo};

    fn profile(db: &Database, name: &str) -> ProfileId {
        let profile = sample_profile(ProfileId::new(), name);
        ProfileRepo::insert(db.connection(), &profile).expect("insert profile");
        profile.id
    }

    fn session(profile_id: ProfileId, status: ProfileRuntimeStatus) -> RuntimeSession {
        RuntimeSession {
            profile_id,
            status,
            pid: Some(4242),
            cdp_port: Some(51_234),
            thorium_version: Some("M152.0.7977.55".to_owned()),
            started_at: Some(Timestamp::from_unix_seconds(1_700_000_000)),
            updated_at: Timestamp::from_unix_seconds(1_700_000_005),
        }
    }

    #[test]
    fn sessions_round_trip() {
        let db = Database::open_in_memory().expect("open");
        let id = profile(&db, "Work");
        let s = session(id, ProfileRuntimeStatus::Running);
        RuntimeRepo::upsert(db.connection(), &s).expect("upsert");
        assert_eq!(RuntimeRepo::get(db.connection(), id).expect("get"), Some(s));
    }

    #[test]
    fn upserting_replaces_rather_than_duplicating() {
        let db = Database::open_in_memory().expect("open");
        let id = profile(&db, "Work");
        RuntimeRepo::upsert(db.connection(), &session(id, ProfileRuntimeStatus::Starting)).expect("upsert");
        RuntimeRepo::upsert(db.connection(), &session(id, ProfileRuntimeStatus::Running)).expect("upsert");
        assert_eq!(RuntimeRepo::list(db.connection()).expect("list").len(), 1);
        assert_eq!(
            RuntimeRepo::get(db.connection(), id)
                .expect("get")
                .map(|s| s.status),
            Some(ProfileRuntimeStatus::Running)
        );
    }

    #[test]
    fn startup_recovery_clears_every_stale_session() {
        let db = Database::open_in_memory().expect("open");
        let a = profile(&db, "A");
        let b = profile(&db, "B");
        RuntimeRepo::upsert(db.connection(), &session(a, ProfileRuntimeStatus::Running)).expect("upsert");
        RuntimeRepo::upsert(db.connection(), &session(b, ProfileRuntimeStatus::Starting)).expect("upsert");
        assert_eq!(RuntimeRepo::clear_all(db.connection()).expect("clear"), 2);
        assert!(RuntimeRepo::list(db.connection()).expect("list").is_empty());
    }

    #[test]
    fn only_active_sessions_pin_a_thorium_version() {
        let db = Database::open_in_memory().expect("open");
        let running = profile(&db, "Running");
        let stopped = profile(&db, "Stopped");
        RuntimeRepo::upsert(db.connection(), &session(running, ProfileRuntimeStatus::Running))
            .expect("upsert");
        let mut stopped_session = session(stopped, ProfileRuntimeStatus::Stopped);
        stopped_session.thorium_version = Some("M151.0.7922.72".to_owned());
        RuntimeRepo::upsert(db.connection(), &stopped_session).expect("upsert");

        assert_eq!(
            RuntimeRepo::versions_in_use(db.connection()).expect("in use"),
            vec!["M152.0.7977.55".to_owned()]
        );
    }

    #[test]
    fn deleting_a_profile_removes_its_session() {
        let db = Database::open_in_memory().expect("open");
        let id = profile(&db, "Work");
        RuntimeRepo::upsert(db.connection(), &session(id, ProfileRuntimeStatus::Running)).expect("upsert");
        ProfileRepo::delete(db.connection(), id).expect("delete");
        assert_eq!(RuntimeRepo::get(db.connection(), id).expect("get"), None);
    }

    #[test]
    fn a_session_for_an_unknown_profile_is_refused() {
        let db = Database::open_in_memory().expect("open");
        let orphan = session(ProfileId::new(), ProfileRuntimeStatus::Running);
        assert!(RuntimeRepo::upsert(db.connection(), &orphan).is_err());
    }
}
