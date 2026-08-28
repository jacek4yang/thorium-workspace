//! Recovery code status.
//!
//! The code text lives in the vault; this table holds only its reference, its
//! position and whether the user has spent it.

use rusqlite::{Connection, OptionalExtension, Row, params};
use tw_domain::{AccountId, RecoveryCode, RecoveryCodeId, SecretRef, Timestamp};

use crate::error::{StorageError, StorageResult};

/// Reads and writes recovery codes.
pub struct RecoveryCodeRepo;

const SELECT: &str =
    "SELECT id, account_id, code_ref, position, used, used_at, created_at FROM recovery_codes";

impl RecoveryCodeRepo {
    /// Inserts one recovery code.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Conflict`] when the account does not exist.
    pub fn insert(conn: &Connection, code: &RecoveryCode) -> StorageResult<()> {
        conn.execute(
            "INSERT INTO recovery_codes (id, account_id, code_ref, position, used, used_at, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                code.id.to_string(),
                code.account_id.to_string(),
                code.code_ref.to_string(),
                i64::from(code.position),
                i64::from(code.used),
                code.used_at.map(Timestamp::as_unix_seconds),
                code.created_at.as_unix_seconds(),
            ],
        )?;
        Ok(())
    }

    /// Marks a code used or unused, recording the time it was marked used.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] when no such code exists.
    pub fn set_used(conn: &Connection, id: RecoveryCodeId, used: bool) -> StorageResult<()> {
        let used_at = used.then(|| Timestamp::now().as_unix_seconds());
        let changed = conn.execute(
            "UPDATE recovery_codes SET used = ?2, used_at = ?3 WHERE id = ?1",
            params![id.to_string(), i64::from(used), used_at],
        )?;
        if changed == 0 {
            return Err(StorageError::not_found("recovery code", id));
        }
        Ok(())
    }

    /// Deletes one recovery code.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] when no such code exists.
    pub fn delete(conn: &Connection, id: RecoveryCodeId) -> StorageResult<()> {
        let changed = conn.execute(
            "DELETE FROM recovery_codes WHERE id = ?1",
            params![id.to_string()],
        )?;
        if changed == 0 {
            return Err(StorageError::not_found("recovery code", id));
        }
        Ok(())
    }

    /// Deletes every recovery code for an account, returning the vault
    /// references that are now orphaned.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a failure.
    pub fn delete_for_account(conn: &Connection, account_id: AccountId) -> StorageResult<Vec<SecretRef>> {
        let existing = Self::list_for_account(conn, account_id)?;
        conn.execute(
            "DELETE FROM recovery_codes WHERE account_id = ?1",
            params![account_id.to_string()],
        )?;
        Ok(existing.into_iter().map(|c| c.code_ref).collect())
    }

    /// Fetches one code.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] when no such code exists.
    pub fn get(conn: &Connection, id: RecoveryCodeId) -> StorageResult<RecoveryCode> {
        conn.query_row(
            &format!("{SELECT} WHERE id = ?1"),
            params![id.to_string()],
            map_code,
        )
        .optional()?
        .ok_or_else(|| StorageError::not_found("recovery code", id))
    }

    /// Lists an account's codes in their stored order.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn list_for_account(conn: &Connection, account_id: AccountId) -> StorageResult<Vec<RecoveryCode>> {
        let mut stmt = conn.prepare(&format!("{SELECT} WHERE account_id = ?1 ORDER BY position"))?;
        let rows = stmt.query_map(params![account_id.to_string()], map_code)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// The next free position for an account's code list.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn next_position(conn: &Connection, account_id: AccountId) -> StorageResult<u32> {
        let max: Option<i64> = conn.query_row(
            "SELECT MAX(position) FROM recovery_codes WHERE account_id = ?1",
            params![account_id.to_string()],
            |row| row.get(0),
        )?;
        Ok(max.map_or(0, |m| u32::try_from(m + 1).unwrap_or(0)))
    }

    /// Counts an account's unused codes, so the UI can warn when they run low.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn unused_count(conn: &Connection, account_id: AccountId) -> StorageResult<u32> {
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM recovery_codes WHERE account_id = ?1 AND used = 0",
            params![account_id.to_string()],
            |row| row.get(0),
        )?;
        Ok(u32::try_from(count).unwrap_or(u32::MAX))
    }

    /// Every vault reference held by a recovery code.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn code_refs(conn: &Connection) -> StorageResult<Vec<SecretRef>> {
        let mut stmt = conn.prepare("SELECT code_ref FROM recovery_codes")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut refs = Vec::new();
        for value in rows {
            refs.push(
                value?
                    .parse()
                    .map_err(|_| StorageError::Corrupt("a stored code reference is not a UUID".to_owned()))?,
            );
        }
        Ok(refs)
    }
}

fn map_code(row: &Row<'_>) -> rusqlite::Result<RecoveryCode> {
    let id: String = row.get(0)?;
    let account_id: String = row.get(1)?;
    let code_ref: String = row.get(2)?;
    let position: i64 = row.get(3)?;
    let used: i64 = row.get(4)?;
    let used_at: Option<i64> = row.get(5)?;
    Ok(RecoveryCode {
        id: id
            .parse()
            .map_err(|_| bad_column(0, "recovery code id is not a UUID"))?,
        account_id: account_id
            .parse()
            .map_err(|_| bad_column(1, "account id is not a UUID"))?,
        code_ref: code_ref
            .parse()
            .map_err(|_| bad_column(2, "code reference is not a UUID"))?,
        position: u32::try_from(position).unwrap_or(0),
        used: used != 0,
        used_at: used_at.map(Timestamp::from_unix_seconds),
        created_at: Timestamp::from_unix_seconds(row.get(6)?),
    })
}

fn bad_column(index: usize, message: &'static str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AccountRepo, Database};

    fn account(db: &Database) -> AccountId {
        let now = Timestamp::from_unix_seconds(1_700_000_000);
        let account = tw_domain::Account {
            id: AccountId::new(),
            display_name: "Acct".to_owned(),
            service: tw_domain::ServiceKind::GitHub,
            username: None,
            email: None,
            login_url: None,
            tags: Vec::new(),
            notes: String::new(),
            password_ref: None,
            created_at: now,
            updated_at: now,
        };
        AccountRepo::insert(db.connection(), &account).expect("insert account");
        account.id
    }

    fn code(account_id: AccountId, position: u32) -> RecoveryCode {
        RecoveryCode {
            id: RecoveryCodeId::new(),
            account_id,
            code_ref: SecretRef::new(),
            position,
            used: false,
            used_at: None,
            created_at: Timestamp::from_unix_seconds(1_700_000_000),
        }
    }

    #[test]
    fn codes_round_trip_and_keep_their_order() {
        let db = Database::open_in_memory().expect("open");
        let account_id = account(&db);
        for position in [2u32, 0, 1] {
            RecoveryCodeRepo::insert(db.connection(), &code(account_id, position)).expect("insert");
        }
        let listed = RecoveryCodeRepo::list_for_account(db.connection(), account_id).expect("list");
        assert_eq!(
            listed.iter().map(|c| c.position).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
    }

    #[test]
    fn marking_a_code_used_records_when() {
        let db = Database::open_in_memory().expect("open");
        let account_id = account(&db);
        let c = code(account_id, 0);
        RecoveryCodeRepo::insert(db.connection(), &c).expect("insert");
        assert_eq!(
            RecoveryCodeRepo::unused_count(db.connection(), account_id).expect("count"),
            1
        );

        RecoveryCodeRepo::set_used(db.connection(), c.id, true).expect("mark used");
        let loaded = RecoveryCodeRepo::get(db.connection(), c.id).expect("get");
        assert!(loaded.used);
        assert!(
            loaded.used_at.is_some(),
            "the time a code was spent must be recorded"
        );
        assert_eq!(
            RecoveryCodeRepo::unused_count(db.connection(), account_id).expect("count"),
            0
        );
    }

    #[test]
    fn a_code_can_be_marked_unused_again_and_the_timestamp_is_cleared() {
        let db = Database::open_in_memory().expect("open");
        let account_id = account(&db);
        let c = code(account_id, 0);
        RecoveryCodeRepo::insert(db.connection(), &c).expect("insert");
        RecoveryCodeRepo::set_used(db.connection(), c.id, true).expect("mark used");
        RecoveryCodeRepo::set_used(db.connection(), c.id, false).expect("mark unused");
        let loaded = RecoveryCodeRepo::get(db.connection(), c.id).expect("get");
        assert!(!loaded.used);
        assert_eq!(loaded.used_at, None);
    }

    #[test]
    fn positions_are_allocated_after_the_existing_highest() {
        let db = Database::open_in_memory().expect("open");
        let account_id = account(&db);
        assert_eq!(
            RecoveryCodeRepo::next_position(db.connection(), account_id).expect("next"),
            0
        );
        RecoveryCodeRepo::insert(db.connection(), &code(account_id, 0)).expect("insert");
        RecoveryCodeRepo::insert(db.connection(), &code(account_id, 7)).expect("insert");
        assert_eq!(
            RecoveryCodeRepo::next_position(db.connection(), account_id).expect("next"),
            8
        );
    }

    #[test]
    fn deleting_an_account_removes_its_codes() {
        let db = Database::open_in_memory().expect("open");
        let account_id = account(&db);
        RecoveryCodeRepo::insert(db.connection(), &code(account_id, 0)).expect("insert");
        AccountRepo::delete(db.connection(), account_id).expect("delete");
        assert!(
            RecoveryCodeRepo::list_for_account(db.connection(), account_id)
                .expect("list")
                .is_empty()
        );
    }

    #[test]
    fn bulk_delete_returns_the_now_orphaned_vault_references() {
        let db = Database::open_in_memory().expect("open");
        let account_id = account(&db);
        let a = code(account_id, 0);
        let b = code(account_id, 1);
        RecoveryCodeRepo::insert(db.connection(), &a).expect("insert");
        RecoveryCodeRepo::insert(db.connection(), &b).expect("insert");
        let mut orphans = RecoveryCodeRepo::delete_for_account(db.connection(), account_id).expect("delete");
        orphans.sort();
        let mut expected = vec![a.code_ref, b.code_ref];
        expected.sort();
        assert_eq!(orphans, expected);
        assert!(
            RecoveryCodeRepo::list_for_account(db.connection(), account_id)
                .expect("list")
                .is_empty()
        );
    }

    #[test]
    fn missing_codes_are_reported() {
        let db = Database::open_in_memory().expect("open");
        let id = RecoveryCodeId::new();
        assert!(matches!(
            RecoveryCodeRepo::get(db.connection(), id),
            Err(StorageError::NotFound { .. })
        ));
        assert!(matches!(
            RecoveryCodeRepo::set_used(db.connection(), id, true),
            Err(StorageError::NotFound { .. })
        ));
        assert!(matches!(
            RecoveryCodeRepo::delete(db.connection(), id),
            Err(StorageError::NotFound { .. })
        ));
    }

    #[test]
    fn no_code_text_is_stored_in_the_table() {
        let db = Database::open_in_memory().expect("open");
        let account_id = account(&db);
        RecoveryCodeRepo::insert(db.connection(), &code(account_id, 0)).expect("insert");
        let stmt = db
            .connection()
            .prepare("SELECT * FROM recovery_codes")
            .expect("prepare");
        let columns: Vec<String> = stmt.column_names().into_iter().map(str::to_owned).collect();
        assert!(!columns.iter().any(|c| c == "code"), "{columns:?}");
        assert!(columns.iter().any(|c| c == "code_ref"));
    }
}
