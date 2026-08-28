//! Account metadata.

use rusqlite::{Connection, OptionalExtension, Row, params};
use tw_domain::{Account, AccountId, SecretRef, ServiceKind, Timestamp};

use crate::error::{StorageError, StorageResult};

/// An account plus the profiles it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRecord {
    /// The account itself.
    pub account: Account,
    /// Profiles this account is associated with.
    pub profile_ids: Vec<tw_domain::ProfileId>,
}

/// Reads and writes accounts.
pub struct AccountRepo;

const SELECT: &str = "SELECT id, display_name, service_kind, service_label, username, email, \
                      login_url, notes, password_ref, created_at, updated_at FROM accounts";

impl AccountRepo {
    /// Inserts an account and its tags.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Conflict`] on a duplicate id and
    /// [`StorageError::Query`] on a write failure.
    pub fn insert(conn: &Connection, account: &Account) -> StorageResult<()> {
        conn.execute(
            "INSERT INTO accounts (id, display_name, service_kind, service_label, username, email, \
             login_url, notes, password_ref, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                account.id.to_string(),
                account.display_name,
                account.service.discriminant(),
                service_label(&account.service),
                account.username,
                account.email,
                account.login_url,
                account.notes,
                account.password_ref.map(|r| r.to_string()),
                account.created_at.as_unix_seconds(),
                account.updated_at.as_unix_seconds(),
            ],
        )?;
        Self::replace_tags(conn, account.id, &account.tags)?;
        Ok(())
    }

    /// Updates an account and its tags.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] when no such account exists.
    pub fn update(conn: &Connection, account: &Account) -> StorageResult<()> {
        let changed = conn.execute(
            "UPDATE accounts SET display_name = ?2, service_kind = ?3, service_label = ?4, \
             username = ?5, email = ?6, login_url = ?7, notes = ?8, password_ref = ?9, updated_at = ?10 \
             WHERE id = ?1",
            params![
                account.id.to_string(),
                account.display_name,
                account.service.discriminant(),
                service_label(&account.service),
                account.username,
                account.email,
                account.login_url,
                account.notes,
                account.password_ref.map(|r| r.to_string()),
                account.updated_at.as_unix_seconds(),
            ],
        )?;
        if changed == 0 {
            return Err(StorageError::not_found("account", account.id));
        }
        Self::replace_tags(conn, account.id, &account.tags)?;
        Ok(())
    }

    /// Deletes an account. Cascades to tags, factors, recovery codes and profile
    /// associations.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] when no such account exists.
    pub fn delete(conn: &Connection, id: AccountId) -> StorageResult<()> {
        let changed = conn.execute("DELETE FROM accounts WHERE id = ?1", params![id.to_string()])?;
        if changed == 0 {
            return Err(StorageError::not_found("account", id));
        }
        Ok(())
    }

    /// Fetches one account.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] when no such account exists.
    pub fn get(conn: &Connection, id: AccountId) -> StorageResult<Account> {
        let row = conn
            .query_row(
                &format!("{SELECT} WHERE id = ?1"),
                params![id.to_string()],
                map_account,
            )
            .optional()?;
        let mut account = row.ok_or_else(|| StorageError::not_found("account", id))?;
        account.tags = Self::tags(conn, id)?;
        Ok(account)
    }

    /// Lists every account, newest name order, with tags loaded.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn list(conn: &Connection) -> StorageResult<Vec<Account>> {
        let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY display_name COLLATE NOCASE"))?;
        let rows = stmt.query_map([], map_account)?;
        let mut accounts = Vec::new();
        for account in rows {
            let mut account = account?;
            account.tags = Self::tags(conn, account.id)?;
            accounts.push(account);
        }
        Ok(accounts)
    }

    /// Lists the accounts associated with a profile, in their stored order.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn list_for_profile(
        conn: &Connection,
        profile_id: tw_domain::ProfileId,
    ) -> StorageResult<Vec<Account>> {
        let mut stmt = conn.prepare(
            "SELECT a.id, a.display_name, a.service_kind, a.service_label, a.username, a.email, \
             a.login_url, a.notes, a.password_ref, a.created_at, a.updated_at \
             FROM accounts a JOIN profile_accounts pa ON pa.account_id = a.id \
             WHERE pa.profile_id = ?1 ORDER BY pa.position, a.display_name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map(params![profile_id.to_string()], map_account)?;
        let mut accounts = Vec::new();
        for account in rows {
            let mut account = account?;
            account.tags = Self::tags(conn, account.id)?;
            accounts.push(account);
        }
        Ok(accounts)
    }

    /// Returns the tags on an account.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn tags(conn: &Connection, id: AccountId) -> StorageResult<Vec<String>> {
        let mut stmt = conn.prepare("SELECT tag FROM account_tags WHERE account_id = ?1 ORDER BY tag")?;
        let rows = stmt.query_map(params![id.to_string()], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Every distinct tag in use, for the filter UI.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn all_tags(conn: &Connection) -> StorageResult<Vec<String>> {
        let mut stmt = conn.prepare("SELECT DISTINCT tag FROM account_tags ORDER BY tag")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Every vault reference held by any account's password field.
    ///
    /// Used to find orphaned secrets.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn password_refs(conn: &Connection) -> StorageResult<Vec<SecretRef>> {
        let mut stmt = conn.prepare("SELECT password_ref FROM accounts WHERE password_ref IS NOT NULL")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut refs = Vec::new();
        for value in rows {
            let raw = value?;
            refs.push(
                raw.parse::<SecretRef>().map_err(|_| {
                    StorageError::Corrupt("a stored secret reference is not a UUID".to_owned())
                })?,
            );
        }
        Ok(refs)
    }

    fn replace_tags(conn: &Connection, id: AccountId, tags: &[String]) -> StorageResult<()> {
        conn.execute(
            "DELETE FROM account_tags WHERE account_id = ?1",
            params![id.to_string()],
        )?;
        let mut stmt = conn.prepare("INSERT INTO account_tags (account_id, tag) VALUES (?1, ?2)")?;
        for tag in tags {
            stmt.execute(params![id.to_string(), tag])?;
        }
        Ok(())
    }
}

fn service_label(kind: &ServiceKind) -> String {
    match kind {
        ServiceKind::Other(label) => label.clone(),
        other => other.label().to_owned(),
    }
}

fn map_account(row: &Row<'_>) -> rusqlite::Result<Account> {
    let id: String = row.get(0)?;
    let service_kind: String = row.get(2)?;
    let service_label: String = row.get(3)?;
    let password_ref: Option<String> = row.get(8)?;
    Ok(Account {
        id: id
            .parse()
            .map_err(|_| bad_column(0, "account id is not a UUID"))?,
        display_name: row.get(1)?,
        service: ServiceKind::from_parts(&service_kind, &service_label)
            .map_err(|_| bad_column(2, "unknown service kind"))?,
        username: row.get(4)?,
        email: row.get(5)?,
        login_url: row.get(6)?,
        tags: Vec::new(),
        notes: row.get(7)?,
        password_ref: match password_ref {
            Some(raw) => Some(
                raw.parse()
                    .map_err(|_| bad_column(8, "secret reference is not a UUID"))?,
            ),
            None => None,
        },
        created_at: Timestamp::from_unix_seconds(row.get(9)?),
        updated_at: Timestamp::from_unix_seconds(row.get(10)?),
    })
}

fn bad_column(index: usize, message: &'static str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, message.into())
}

#[cfg(test)]
mod tests {
    use tw_domain::ProfileId;

    use super::*;
    use crate::{Database, ProfileRepo};

    fn account(name: &str, service: ServiceKind) -> Account {
        let now = Timestamp::from_unix_seconds(1_700_000_000);
        Account {
            id: AccountId::new(),
            display_name: name.to_owned(),
            service,
            username: Some("user".to_owned()),
            email: Some("user@example.test".to_owned()),
            login_url: Some("https://example.test/login".to_owned()),
            tags: vec!["ci".to_owned(), "work".to_owned()],
            notes: "notes".to_owned(),
            password_ref: Some(SecretRef::new()),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn accounts_round_trip_with_every_field() {
        let db = Database::open_in_memory().expect("open");
        let original = account("Build bot", ServiceKind::GitHub);
        AccountRepo::insert(db.connection(), &original).expect("insert");
        assert_eq!(
            AccountRepo::get(db.connection(), original.id).expect("get"),
            original
        );
    }

    #[test]
    fn a_custom_service_label_survives_the_round_trip() {
        let db = Database::open_in_memory().expect("open");
        let original = account("Mail", ServiceKind::Other("Fastmail".to_owned()));
        AccountRepo::insert(db.connection(), &original).expect("insert");
        let loaded = AccountRepo::get(db.connection(), original.id).expect("get");
        assert_eq!(loaded.service, ServiceKind::Other("Fastmail".to_owned()));
    }

    #[test]
    fn optional_fields_may_be_absent() {
        let db = Database::open_in_memory().expect("open");
        let mut sparse = account("Bare", ServiceKind::Microsoft);
        sparse.username = None;
        sparse.email = None;
        sparse.login_url = None;
        sparse.password_ref = None;
        sparse.tags = Vec::new();
        AccountRepo::insert(db.connection(), &sparse).expect("insert");
        assert_eq!(AccountRepo::get(db.connection(), sparse.id).expect("get"), sparse);
    }

    #[test]
    fn updating_replaces_tags_rather_than_appending() {
        let db = Database::open_in_memory().expect("open");
        let mut a = account("A", ServiceKind::GitHub);
        AccountRepo::insert(db.connection(), &a).expect("insert");
        a.tags = vec!["personal".to_owned()];
        a.display_name = "Renamed".to_owned();
        AccountRepo::update(db.connection(), &a).expect("update");
        let loaded = AccountRepo::get(db.connection(), a.id).expect("get");
        assert_eq!(loaded.tags, vec!["personal".to_owned()]);
        assert_eq!(loaded.display_name, "Renamed");
    }

    #[test]
    fn missing_accounts_are_reported_not_silently_ignored() {
        let db = Database::open_in_memory().expect("open");
        let missing = AccountId::new();
        assert!(matches!(
            AccountRepo::get(db.connection(), missing),
            Err(StorageError::NotFound { .. })
        ));
        assert!(matches!(
            AccountRepo::delete(db.connection(), missing),
            Err(StorageError::NotFound { .. })
        ));
        let ghost = account("Ghost", ServiceKind::GitHub);
        assert!(matches!(
            AccountRepo::update(db.connection(), &ghost),
            Err(StorageError::NotFound { .. })
        ));
    }

    #[test]
    fn listing_is_ordered_case_insensitively_by_name() {
        let db = Database::open_in_memory().expect("open");
        for name in ["zeta", "Alpha", "beta"] {
            AccountRepo::insert(db.connection(), &account(name, ServiceKind::GitHub)).expect("insert");
        }
        let names: Vec<String> = AccountRepo::list(db.connection())
            .expect("list")
            .into_iter()
            .map(|a| a.display_name)
            .collect();
        assert_eq!(
            names,
            vec!["Alpha".to_owned(), "beta".to_owned(), "zeta".to_owned()]
        );
    }

    #[test]
    fn accounts_can_be_listed_per_profile() {
        let mut db = Database::open_in_memory().expect("open");
        let a1 = account("First", ServiceKind::GitHub);
        let a2 = account("Second", ServiceKind::Microsoft);
        let a3 = account("Unlinked", ServiceKind::GitHub);
        for a in [&a1, &a2, &a3] {
            AccountRepo::insert(db.connection(), a).expect("insert");
        }
        let profile = crate::repo::profiles::tests::sample_profile(ProfileId::new(), "Work");
        ProfileRepo::insert(db.connection(), &profile).expect("insert profile");
        ProfileRepo::set_accounts(db.connection_mut(), profile.id, &[a2.id, a1.id]).expect("link");

        let listed: Vec<String> = AccountRepo::list_for_profile(db.connection(), profile.id)
            .expect("list")
            .into_iter()
            .map(|a| a.display_name)
            .collect();
        assert_eq!(
            listed,
            vec!["Second".to_owned(), "First".to_owned()],
            "stored order is preserved"
        );
    }

    #[test]
    fn password_references_can_be_enumerated_for_orphan_collection() {
        let db = Database::open_in_memory().expect("open");
        let with = account("With", ServiceKind::GitHub);
        let mut without = account("Without", ServiceKind::GitHub);
        without.password_ref = None;
        AccountRepo::insert(db.connection(), &with).expect("insert");
        AccountRepo::insert(db.connection(), &without).expect("insert");
        let refs = AccountRepo::password_refs(db.connection()).expect("refs");
        assert_eq!(refs, vec![with.password_ref.expect("set")]);
    }

    #[test]
    fn tags_are_deduplicated_across_accounts_in_the_filter_list() {
        let db = Database::open_in_memory().expect("open");
        AccountRepo::insert(db.connection(), &account("A", ServiceKind::GitHub)).expect("insert");
        AccountRepo::insert(db.connection(), &account("B", ServiceKind::GitHub)).expect("insert");
        assert_eq!(
            AccountRepo::all_tags(db.connection()).expect("tags"),
            vec!["ci".to_owned(), "work".to_owned()]
        );
    }
}
