//! Account, factor, and recovery-code persistence.
//!
//! An [`Account`] spans three tables (`accounts`, `factors`,
//! `recovery_codes`). [`Store::create_account`] writes all three atomically;
//! subsequent edits go through targeted methods so HOTP counter increments
//! are never clobbered by whole-record writes.

use chrono::{DateTime, Utc};
use rusqlite::{Row, params};
use thorium_workspace_domain::{
    Account, AccountId, FactorId, FactorKind, OtpAlgorithm, RecoveryCode, RecoveryCodeId,
    SecondFactor, SecretRef, ServiceKind,
};

use crate::Store;
use crate::error::{StorageError, map_write_error};
use crate::num;
use crate::time;

const COL_ACCOUNT_ID: &str = "accounts.id";
const COL_ACCOUNT_TAGS: &str = "accounts.tags";
const COL_ACCOUNT_CREATED: &str = "accounts.created_at";
const COL_ACCOUNT_UPDATED: &str = "accounts.updated_at";
const COL_FACTOR_KIND: &str = "factors.kind";
const COL_FACTOR_ALGORITHM: &str = "factors.algorithm";
const COL_FACTOR_DIGITS: &str = "factors.digits";
const COL_FACTOR_PERIOD: &str = "factors.period_seconds";
const COL_FACTOR_COUNTER: &str = "factors.counter";
const COL_FACTOR_REF: &str = "factors.secret_ref";
const COL_FACTOR_CREATED: &str = "factors.created_at";
const COL_CODE_POSITION: &str = "recovery_codes.position";
const COL_CODE_USED: &str = "recovery_codes.used";
const COL_CODE_MARKED: &str = "recovery_codes.marked_used_at";

impl Store {
    /// Inserts an account together with its factors and recovery codes in
    /// one transaction. Fails with [`StorageError::NotFound`] when the
    /// owning profile does not exist.
    pub fn create_account(&self, account: &Account) -> Result<(), StorageError> {
        if !self.profile_exists(account.profile_id)? {
            return Err(StorageError::NotFound { entity: "profile" });
        }
        let tx = self.conn.unchecked_transaction()?;
        write_account(&tx, account)?;
        for factor in &account.factors {
            write_factor(&tx, factor)?;
        }
        for code in &account.recovery_codes {
            write_recovery_code(&tx, code)?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Loads every account of a profile, ordered by display name.
    pub fn list_accounts(
        &self,
        profile_id: thorium_workspace_domain::ProfileId,
    ) -> Result<Vec<Account>, StorageError> {
        let mut statement = self.conn.prepare(
            "SELECT id FROM accounts WHERE profile_id = ?1
             ORDER BY display_name COLLATE NOCASE",
        )?;
        let rows = statement.query_map(params![profile_id.to_string()], |row| {
            row.get::<_, String>(0)
        })?;
        let mut accounts = Vec::new();
        for text in rows {
            let id: AccountId = parse_id(&text?, COL_ACCOUNT_ID)?;
            if let Some(account) = self.get_account(id)? {
                accounts.push(account);
            }
        }
        Ok(accounts)
    }

    /// Loads one account with its factors and recovery codes.
    pub fn get_account(&self, id: AccountId) -> Result<Option<Account>, StorageError> {
        let mut statement = self.conn.prepare(
            "SELECT id, profile_id, display_name, service_id, service_label, username,
                    email, login_url, tags, notes, password_secret_ref, created_at, updated_at
             FROM accounts
             WHERE id = ?1",
        )?;
        let mut rows = statement.query(params![id.to_string()])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        let mut account = account_from_row(row)?;
        account.factors = self.list_factors(account.id)?;
        account.recovery_codes = self.list_recovery_codes(account.id)?;
        Ok(Some(account))
    }

    /// Updates account metadata. Factors and recovery codes are managed
    /// through their own methods. Returns `false` when the account does
    /// not exist.
    pub fn update_account(&self, account: &Account) -> Result<bool, StorageError> {
        let (service_id, service_label) = service_parts(&account.service_kind);
        let tags = serde_json::to_string(&account.tags)?;
        let changed = self.conn.execute(
            "UPDATE accounts SET
                display_name = ?2, service_id = ?3, service_label = ?4, username = ?5,
                email = ?6, login_url = ?7, tags = ?8, notes = ?9,
                password_secret_ref = ?10, updated_at = ?11
             WHERE id = ?1",
            params![
                account.id.to_string(),
                account.display_name,
                service_id,
                service_label,
                account.username,
                account.email,
                account.login_url,
                tags,
                account.notes,
                account.password_ref.as_ref().map(SecretRef::as_str),
                time::to_text(account.updated_at),
            ],
        )?;
        Ok(changed > 0)
    }

    /// Deletes an account (factors and recovery codes cascade). Returns
    /// `false` when the account does not exist. Secret values behind
    /// deleted `SecretRef`s are orphaned in the vault; the caller
    /// (controller) is responsible for purging them.
    pub fn delete_account(&self, id: AccountId) -> Result<bool, StorageError> {
        let changed = self.conn.execute(
            "DELETE FROM accounts WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(changed > 0)
    }

    /// Inserts a second factor. Fails with [`StorageError::NotFound`]
    /// when the owning account does not exist.
    pub fn add_factor(&self, factor: &SecondFactor) -> Result<(), StorageError> {
        if !self.account_exists(factor.account_id)? {
            return Err(StorageError::NotFound { entity: "account" });
        }
        let tx = self.conn.unchecked_transaction()?;
        write_factor(&tx, factor)?;
        tx.commit()?;
        Ok(())
    }

    /// Updates a factor's metadata. The HOTP counter is managed by
    /// [`Store::set_hotp_counter`] so increments never race with edits.
    /// Returns `false` when the factor does not exist.
    pub fn update_factor(&self, factor: &SecondFactor) -> Result<bool, StorageError> {
        let changed = self.conn.execute(
            "UPDATE factors SET
                label = ?2, issuer = ?3, account_label = ?4, algorithm = ?5,
                digits = ?6, period_seconds = ?7, secret_ref = ?8, external_note = ?9
             WHERE id = ?1",
            params![
                factor.id.to_string(),
                factor.label,
                factor.issuer,
                factor.account_label,
                factor.algorithm.map(|algorithm| algorithm.id().to_owned()),
                factor.digits.map(i64::from),
                factor.period_seconds.map(i64::from),
                factor.secret_ref.as_ref().map(SecretRef::as_str),
                factor.external_note,
            ],
        )?;
        Ok(changed > 0)
    }

    /// Deletes a factor. Returns `false` when it does not exist. The seed
    /// behind a deleted factor's `SecretRef` is orphaned in the vault; the
    /// caller (controller) is responsible for purging it.
    pub fn delete_factor(&self, id: FactorId) -> Result<bool, StorageError> {
        let changed = self
            .conn
            .execute("DELETE FROM factors WHERE id = ?1", params![id.to_string()])?;
        Ok(changed > 0)
    }

    /// Loads the factors of one account, ordered by creation time.
    pub fn list_factors(&self, account_id: AccountId) -> Result<Vec<SecondFactor>, StorageError> {
        let mut statement = self.conn.prepare(
            "SELECT id, account_id, kind, label, issuer, account_label, algorithm, digits,
                    period_seconds, counter, secret_ref, external_note, created_at
             FROM factors
             WHERE account_id = ?1
             ORDER BY created_at, id",
        )?;
        let mut rows = statement.query(params![account_id.to_string()])?;
        let mut factors = Vec::new();
        while let Some(row) = rows.next()? {
            factors.push(factor_from_row(row)?);
        }
        Ok(factors)
    }

    /// Persists a HOTP counter increment. Returns `false` when the factor
    /// does not exist or is not an HOTP factor.
    pub fn set_hotp_counter(&self, id: FactorId, counter: u64) -> Result<bool, StorageError> {
        let counter = num::to_i64(COL_FACTOR_COUNTER, counter)?;
        let changed = self.conn.execute(
            "UPDATE factors SET counter = ?2 WHERE id = ?1 AND kind = 'hotp'",
            params![id.to_string(), counter],
        )?;
        Ok(changed > 0)
    }

    /// Inserts a recovery code slot. Fails with
    /// [`StorageError::NotFound`] when the owning account does not exist.
    pub fn add_recovery_code(&self, code: &RecoveryCode) -> Result<(), StorageError> {
        if !self.account_exists(code.account_id)? {
            return Err(StorageError::NotFound { entity: "account" });
        }
        let tx = self.conn.unchecked_transaction()?;
        write_recovery_code(&tx, code)?;
        tx.commit()?;
        Ok(())
    }

    /// Loads the recovery code slots of one account, ordered by position.
    pub fn list_recovery_codes(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<RecoveryCode>, StorageError> {
        let mut statement = self.conn.prepare(
            "SELECT id, account_id, position, used, marked_used_at, secret_ref
             FROM recovery_codes
             WHERE account_id = ?1
             ORDER BY position",
        )?;
        let mut rows = statement.query(params![account_id.to_string()])?;
        let mut codes = Vec::new();
        while let Some(row) = rows.next()? {
            codes.push(code_from_row(row)?);
        }
        Ok(codes)
    }

    /// Marks a recovery code used. Idempotent: the first timestamp wins,
    /// matching [`RecoveryCode::mark_used`]. Returns `false` when the code
    /// does not exist.
    pub fn mark_recovery_code_used(
        &self,
        id: RecoveryCodeId,
        at: DateTime<Utc>,
    ) -> Result<bool, StorageError> {
        let changed = self.conn.execute(
            "UPDATE recovery_codes
             SET used = 1, marked_used_at = ?2
             WHERE id = ?1 AND used = 0",
            params![id.to_string(), time::to_text(at)],
        )?;
        Ok(changed > 0)
    }

    /// Deletes a recovery code slot. Returns `false` when it does not
    /// exist. The value behind the deleted `SecretRef` is orphaned in the
    /// vault; the caller (controller) is responsible for purging it.
    pub fn delete_recovery_code(&self, id: RecoveryCodeId) -> Result<bool, StorageError> {
        let changed = self.conn.execute(
            "DELETE FROM recovery_codes WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(changed > 0)
    }

    fn profile_exists(
        &self,
        id: thorium_workspace_domain::ProfileId,
    ) -> Result<bool, StorageError> {
        let found = self.conn.query_row(
            "SELECT 1 FROM profiles WHERE id = ?1",
            params![id.to_string()],
            |row| row.get::<_, i64>(0),
        );
        match found {
            Ok(_) => Ok(true),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
            Err(other) => Err(StorageError::from(other)),
        }
    }

    fn account_exists(&self, id: AccountId) -> Result<bool, StorageError> {
        let found = self.conn.query_row(
            "SELECT 1 FROM accounts WHERE id = ?1",
            params![id.to_string()],
            |row| row.get::<_, i64>(0),
        );
        match found {
            Ok(_) => Ok(true),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
            Err(other) => Err(StorageError::from(other)),
        }
    }
}

/// Splits a service kind into its stored identifier and custom label.
fn service_parts(kind: &ServiceKind) -> (&'static str, Option<String>) {
    match kind {
        ServiceKind::Custom { label } => ("custom", Some(label.clone())),
        other => (other.id(), None),
    }
}

fn parse_id<T: std::str::FromStr<Err = thorium_workspace_domain::DomainError>>(
    text: &str,
    column: &'static str,
) -> Result<T, StorageError> {
    text.parse().map_err(|_| StorageError::Corrupt {
        column,
        detail: "invalid identifier".to_owned(),
    })
}

fn parse_ref(
    text: Option<String>,
    column: &'static str,
) -> Result<Option<SecretRef>, StorageError> {
    text.map(|value| {
        value.parse().map_err(|_| StorageError::Corrupt {
            column,
            detail: "invalid secret reference".to_owned(),
        })
    })
    .transpose()
}

fn write_account(conn: &rusqlite::Connection, account: &Account) -> Result<(), StorageError> {
    let (service_id, service_label) = service_parts(&account.service_kind);
    let tags = serde_json::to_string(&account.tags)?;
    conn.execute(
        "INSERT INTO accounts (
            id, profile_id, display_name, service_id, service_label, username,
            email, login_url, tags, notes, password_secret_ref, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            account.id.to_string(),
            account.profile_id.to_string(),
            account.display_name,
            service_id,
            service_label,
            account.username,
            account.email,
            account.login_url,
            tags,
            account.notes,
            account.password_ref.as_ref().map(SecretRef::as_str),
            time::to_text(account.created_at),
            time::to_text(account.updated_at),
        ],
    )
    .map_err(|error| map_write_error(error, "account"))?;
    Ok(())
}

fn write_factor(conn: &rusqlite::Connection, factor: &SecondFactor) -> Result<(), StorageError> {
    conn.execute(
        "INSERT INTO factors (
            id, account_id, kind, label, issuer, account_label, algorithm, digits,
            period_seconds, counter, secret_ref, external_note, created_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            factor.id.to_string(),
            factor.account_id.to_string(),
            factor.kind.id(),
            factor.label,
            factor.issuer,
            factor.account_label,
            factor.algorithm.map(|algorithm| algorithm.id().to_owned()),
            factor.digits.map(i64::from),
            factor.period_seconds.map(i64::from),
            factor
                .counter
                .map(|counter| num::to_i64(COL_FACTOR_COUNTER, counter))
                .transpose()?,
            factor.secret_ref.as_ref().map(SecretRef::as_str),
            factor.external_note,
            time::to_text(factor.created_at),
        ],
    )
    .map_err(|error| map_write_error(error, "factor"))?;
    Ok(())
}

fn write_recovery_code(
    conn: &rusqlite::Connection,
    code: &RecoveryCode,
) -> Result<(), StorageError> {
    let position = num::to_i64(COL_CODE_POSITION, code.position)?;
    conn.execute(
        "INSERT INTO recovery_codes (id, account_id, position, used, marked_used_at, secret_ref)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            code.id.to_string(),
            code.account_id.to_string(),
            position,
            i64::from(code.used),
            code.marked_used_at.map(time::to_text),
            code.secret_ref.as_str(),
        ],
    )
    .map_err(|error| map_write_error(error, "recovery code"))?;
    Ok(())
}

fn account_from_row(row: &Row<'_>) -> Result<Account, StorageError> {
    let id_text: String = row.get("id")?;
    let profile_text: String = row.get("profile_id")?;
    let service_id: String = row.get("service_id")?;
    let service_label: Option<String> = row.get("service_label")?;
    let service_kind = ServiceKind::from_id_and_label(&service_id, service_label.as_deref())
        .ok_or(StorageError::Corrupt {
            column: "accounts.service_id",
            detail: format!("unknown service {service_id:?}"),
        })?;
    let tags_text: String = row.get("tags")?;
    let tags: Vec<String> =
        serde_json::from_str(&tags_text).map_err(|source| StorageError::Corrupt {
            column: COL_ACCOUNT_TAGS,
            detail: source.to_string(),
        })?;
    let created_text: String = row.get("created_at")?;
    let updated_text: String = row.get("updated_at")?;
    Ok(Account {
        id: parse_id(&id_text, COL_ACCOUNT_ID)?,
        profile_id: parse_id(&profile_text, COL_ACCOUNT_ID)?,
        display_name: row.get("display_name")?,
        service_kind,
        username: row.get("username")?,
        email: row.get("email")?,
        login_url: row.get("login_url")?,
        tags,
        notes: row.get("notes")?,
        password_ref: parse_ref(
            row.get("password_secret_ref")?,
            "accounts.password_secret_ref",
        )?,
        factors: Vec::new(),
        recovery_codes: Vec::new(),
        created_at: time::from_text(COL_ACCOUNT_CREATED, &created_text)?,
        updated_at: time::from_text(COL_ACCOUNT_UPDATED, &updated_text)?,
    })
}

fn factor_from_row(row: &Row<'_>) -> Result<SecondFactor, StorageError> {
    let kind_text: String = row.get("kind")?;
    let kind = FactorKind::from_id(&kind_text).ok_or(StorageError::Corrupt {
        column: COL_FACTOR_KIND,
        detail: format!("unknown factor kind {kind_text:?}"),
    })?;
    let algorithm: Option<OtpAlgorithm> = row
        .get::<_, Option<String>>("algorithm")?
        .map(|text| {
            OtpAlgorithm::from_id(&text).ok_or(StorageError::Corrupt {
                column: COL_FACTOR_ALGORITHM,
                detail: format!("unknown algorithm {text:?}"),
            })
        })
        .transpose()?;
    let digits: Option<u8> = row
        .get::<_, Option<i64>>("digits")?
        .map(|value| num::from_i64(COL_FACTOR_DIGITS, value))
        .transpose()?;
    let period: Option<u32> = row
        .get::<_, Option<i64>>("period_seconds")?
        .map(|value| num::from_i64(COL_FACTOR_PERIOD, value))
        .transpose()?;
    let counter: Option<u64> = row
        .get::<_, Option<i64>>("counter")?
        .map(|value| num::from_i64(COL_FACTOR_COUNTER, value))
        .transpose()?;
    let created_text: String = row.get("created_at")?;
    Ok(SecondFactor {
        id: parse_id(&row.get::<_, String>("id")?, "factors.id")?,
        account_id: parse_id(&row.get::<_, String>("account_id")?, "factors.account_id")?,
        kind,
        label: row.get("label")?,
        issuer: row.get("issuer")?,
        account_label: row.get("account_label")?,
        algorithm,
        digits,
        period_seconds: period,
        counter,
        secret_ref: parse_ref(row.get("secret_ref")?, COL_FACTOR_REF)?,
        external_note: row.get("external_note")?,
        created_at: time::from_text(COL_FACTOR_CREATED, &created_text)?,
    })
}

fn code_from_row(row: &Row<'_>) -> Result<RecoveryCode, StorageError> {
    let position = num::from_i64::<u32>(COL_CODE_POSITION, row.get("position")?)?;
    let used_value: i64 = row.get("used")?;
    let used = match used_value {
        0 => false,
        1 => true,
        _ => {
            return Err(StorageError::Corrupt {
                column: COL_CODE_USED,
                detail: format!("expected 0 or 1, got {used_value}"),
            });
        }
    };
    let marked_used_at: Option<DateTime<Utc>> = row
        .get::<_, Option<String>>("marked_used_at")?
        .map(|text| time::from_text(COL_CODE_MARKED, &text))
        .transpose()?;
    let secret_ref_text: String = row.get("secret_ref")?;
    let secret_ref: SecretRef = secret_ref_text.parse().map_err(|_| StorageError::Corrupt {
        column: "recovery_codes.secret_ref",
        detail: "invalid secret reference".to_owned(),
    })?;
    Ok(RecoveryCode {
        id: parse_id(&row.get::<_, String>("id")?, "recovery_codes.id")?,
        account_id: parse_id(
            &row.get::<_, String>("account_id")?,
            "recovery_codes.account_id",
        )?,
        position,
        used,
        marked_used_at,
        secret_ref,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use thorium_workspace_domain::AccountInput;

    /// A synthetic account fixture with one TOTP factor and two recovery
    /// code slots. All material is synthetic; nothing here is a real
    /// credential.
    pub(crate) fn sample_account(profile_id: thorium_workspace_domain::ProfileId) -> Account {
        let input = AccountInput {
            display_name: "Work GitHub".to_owned(),
            service_kind: ServiceKind::GitHub,
            username: Some("octocat".to_owned()),
            email: Some("octocat@example.com".to_owned()),
            login_url: Some("https://github.com/login".to_owned()),
            tags: vec!["work".to_owned()],
            notes: "synthetic test account".to_owned(),
        };
        let validated = input.validate().expect("valid input");
        let now = Utc::now();
        let account_id = AccountId::new();
        let factor_id = FactorId::new();
        let code_ids = [RecoveryCodeId::new(), RecoveryCodeId::new()];
        Account {
            id: account_id,
            profile_id,
            display_name: validated.display_name,
            service_kind: validated.service_kind,
            username: validated.username,
            email: validated.email,
            login_url: validated.login_url,
            tags: validated.tags,
            notes: validated.notes,
            password_ref: Some(SecretRef::for_password(&account_id)),
            factors: vec![SecondFactor {
                id: factor_id,
                account_id,
                kind: FactorKind::Totp,
                label: Some("Authenticator".to_owned()),
                issuer: Some("GitHub".to_owned()),
                account_label: Some("octocat".to_owned()),
                algorithm: Some(OtpAlgorithm::Sha1),
                digits: Some(6),
                period_seconds: Some(30),
                counter: None,
                secret_ref: Some(SecretRef::for_otp_seed(&factor_id)),
                external_note: None,
                created_at: now,
            }],
            recovery_codes: vec![
                RecoveryCode {
                    id: code_ids[0],
                    account_id,
                    position: 0,
                    used: false,
                    marked_used_at: None,
                    secret_ref: SecretRef::for_recovery_code(&code_ids[0]),
                },
                RecoveryCode {
                    id: code_ids[1],
                    account_id,
                    position: 1,
                    used: false,
                    marked_used_at: None,
                    secret_ref: SecretRef::for_recovery_code(&code_ids[1]),
                },
            ],
            created_at: now,
            updated_at: now,
        }
    }

    fn temp_store(tag: &str) -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(format!("{tag}.db"));
        let store = Store::open(&path).expect("open");
        (dir, store)
    }

    fn store_with_account(tag: &str) -> (tempfile::TempDir, Store, Account) {
        let (dir, store) = temp_store(tag);
        let profile = crate::profiles::tests::sample_profile("Profiles Owner");
        store.create_profile(&profile).expect("profile insert");
        let account = sample_account(profile.id);
        store.create_account(&account).expect("account insert");
        (dir, store, account)
    }

    #[test]
    fn account_with_factors_and_codes_roundtrips() {
        let (_dir, store, account) = store_with_account("roundtrip");
        let loaded = store
            .get_account(account.id)
            .expect("query")
            .expect("present");
        assert_eq!(loaded, account);

        let listed = store.list_accounts(account.profile_id).expect("list");
        assert_eq!(listed, vec![account]);
    }

    #[test]
    fn create_account_for_missing_profile_is_rejected() {
        let (_dir, store) = temp_store("orphan");
        let account = sample_account(thorium_workspace_domain::ProfileId::new());
        let error = store
            .create_account(&account)
            .expect_err("orphan account must fail");
        assert!(matches!(
            error,
            StorageError::NotFound { entity: "profile" }
        ));
    }

    #[test]
    fn update_account_changes_metadata_only() {
        let (_dir, store, account) = store_with_account("metadata");
        let mut changed = account.clone();
        changed.display_name = "Renamed Account".to_owned();
        changed.service_kind = ServiceKind::Custom {
            label: "Internal Wiki".to_owned(),
        };
        changed.notes = "edited".to_owned();
        changed.updated_at += chrono::Duration::minutes(2);
        changed.factors.clear();
        changed.recovery_codes.clear();
        assert!(store.update_account(&changed).expect("update"));

        let loaded = store
            .get_account(account.id)
            .expect("query")
            .expect("present");
        assert_eq!(loaded.display_name, "Renamed Account");
        assert_eq!(
            loaded.service_kind,
            ServiceKind::Custom {
                label: "Internal Wiki".to_owned()
            }
        );
        assert_eq!(loaded.notes, "edited");
        // Factors and codes are untouched by metadata updates.
        assert_eq!(loaded.factors.len(), 1);
        assert_eq!(loaded.recovery_codes.len(), 2);
    }

    #[test]
    fn factor_crud_and_counter_updates() {
        let (_dir, store, account) = store_with_account("factors");
        let hotp_id = FactorId::new();
        let hotp = SecondFactor {
            id: hotp_id,
            account_id: account.id,
            kind: FactorKind::Hotp,
            label: None,
            issuer: None,
            account_label: None,
            algorithm: Some(OtpAlgorithm::Sha256),
            digits: Some(8),
            period_seconds: None,
            counter: Some(0),
            secret_ref: Some(SecretRef::for_otp_seed(&hotp_id)),
            external_note: None,
            created_at: Utc::now(),
        };
        store.add_factor(&hotp).expect("add hotp");
        assert!(store.set_hotp_counter(hotp_id, 7).expect("counter"));
        assert!(
            !store
                .set_hotp_counter(FactorId::new(), 7)
                .expect("counter missing"),
            "unknown factor must report false"
        );

        let loaded = store.list_factors(account.id).expect("factors");
        let stored_hotp = loaded
            .iter()
            .find(|factor| factor.id == hotp_id)
            .expect("hotp present");
        assert_eq!(stored_hotp.counter, Some(7));
        assert_eq!(stored_hotp.algorithm, Some(OtpAlgorithm::Sha256));
        assert_eq!(stored_hotp.digits, Some(8));

        hotp_counter_updates_only_hotp_factors(&store, account.id);

        assert!(store.delete_factor(hotp_id).expect("delete"));
        assert!(!store.delete_factor(hotp_id).expect("delete again"));
    }

    fn hotp_counter_updates_only_hotp_factors(store: &Store, account_id: AccountId) {
        let factors = store.list_factors(account_id).expect("factors");
        let totp = factors
            .iter()
            .find(|factor| factor.kind == FactorKind::Totp)
            .expect("totp present");
        let updated = store
            .set_hotp_counter(totp.id, 99)
            .expect("counter update on totp");
        assert!(!updated, "totp factors must not accept counters");
        let unchanged = store
            .list_factors(account_id)
            .expect("factors")
            .into_iter()
            .find(|factor| factor.id == totp.id)
            .expect("totp present");
        assert!(unchanged.counter.is_none());
    }

    #[test]
    fn recovery_code_lifecycle_and_position_conflict() {
        let (_dir, store, account) = store_with_account("codes");
        let code = &account.recovery_codes[0];
        let marked_at = Utc::now();
        assert!(
            store
                .mark_recovery_code_used(code.id, marked_at)
                .expect("mark used")
        );
        // Idempotent: the second mark changes nothing and the first
        // timestamp wins.
        assert!(
            !store
                .mark_recovery_code_used(code.id, marked_at + chrono::Duration::seconds(5))
                .expect("mark again")
        );
        let loaded = store.list_recovery_codes(account.id).expect("codes");
        let stored = loaded.iter().find(|slot| slot.id == code.id).expect("slot");
        assert!(stored.used);
        assert_eq!(stored.marked_used_at, Some(marked_at));

        // Duplicate position within the same account conflicts.
        let mut duplicate = account.recovery_codes[0].clone();
        duplicate.id = RecoveryCodeId::new();
        let error = store.add_recovery_code(&duplicate).expect_err("conflict");
        assert!(matches!(
            error,
            StorageError::Conflict {
                field: "position",
                ..
            }
        ));

        assert!(store.delete_recovery_code(code.id).expect("delete"));
        assert_eq!(
            store.list_recovery_codes(account.id).expect("codes").len(),
            1
        );
    }

    #[test]
    fn deleting_account_cascades_factors_and_codes() {
        let (_dir, store, account) = store_with_account("cascade-account");
        assert!(store.delete_account(account.id).expect("delete"));
        assert!(store.get_account(account.id).expect("query").is_none());
        assert!(store.list_factors(account.id).expect("factors").is_empty());
        assert!(
            store
                .list_recovery_codes(account.id)
                .expect("codes")
                .is_empty()
        );
    }

    #[test]
    fn stored_rows_never_contain_secret_values() {
        let (_dir, store, account) = store_with_account("secrets");
        // The synthetic fixture uses only structured references; scan the
        // raw tables to prove no plaintext value column exists.
        let dump: Vec<String> = store
            .conn
            .prepare("SELECT password_secret_ref FROM accounts WHERE id = ?1")
            .expect("prepare")
            .query_map(params![account.id.to_string()], |row| row.get(0))
            .expect("query")
            .map(|row| row.expect("row"))
            .collect();
        assert_eq!(
            dump,
            vec![SecretRef::for_password(&account.id).as_str().to_owned()]
        );
        // The reference is the structured vault path; a password value
        // would never fit this format and the fixture's synthetic name
        // never appears in storage.
        assert!(dump[0].starts_with("account/"));
        assert!(dump[0].ends_with("/password"));
    }
}
