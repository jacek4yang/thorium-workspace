//! Second-factor metadata.
//!
//! The shared OTP secret is *not* here: only `seed_ref`, which points into the
//! vault.

use rusqlite::{Connection, OptionalExtension, Row, params};
use tw_domain::{
    AccountId, FactorId, OtpAlgorithm, OtpDigits, OtpKind, OtpParameters, SecondFactor, SecondFactorKind,
    SecretRef, Timestamp,
};

use crate::error::{StorageError, StorageResult};

/// Reads and writes second factors.
pub struct SecondFactorRepo;

const SELECT: &str = "SELECT id, account_id, label, kind, otp_kind, algorithm, digits, period_seconds, \
                      counter, issuer, account_label, seed_ref, created_at, updated_at \
                      FROM account_factors";

impl SecondFactorRepo {
    /// Inserts a factor.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Conflict`] when the factor is internally
    /// inconsistent or its account does not exist.
    pub fn insert(conn: &Connection, factor: &SecondFactor) -> StorageResult<()> {
        factor
            .validate()
            .map_err(|e| StorageError::Conflict(e.to_string()))?;
        let otp = factor.otp.as_ref();
        conn.execute(
            "INSERT INTO account_factors (id, account_id, label, kind, otp_kind, algorithm, digits, \
             period_seconds, counter, issuer, account_label, seed_ref, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                factor.id.to_string(),
                factor.account_id.to_string(),
                factor.label,
                factor.kind.as_str(),
                otp.map(|o| o.kind.as_uri_type()),
                otp.map(|o| o.algorithm.as_uri_value()),
                otp.map(|o| i64::from(o.digits.count())),
                otp.map(|o| i64::from(o.period_seconds)),
                otp.map(|o| i64::try_from(o.counter).unwrap_or(i64::MAX)),
                otp.and_then(|o| o.issuer.clone()),
                otp.and_then(|o| o.account_label.clone()),
                factor.seed_ref.map(|r| r.to_string()),
                factor.created_at.as_unix_seconds(),
                factor.updated_at.as_unix_seconds(),
            ],
        )?;
        Ok(())
    }

    /// Updates a factor.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] when no such factor exists and
    /// [`StorageError::Conflict`] when it is internally inconsistent.
    pub fn update(conn: &Connection, factor: &SecondFactor) -> StorageResult<()> {
        factor
            .validate()
            .map_err(|e| StorageError::Conflict(e.to_string()))?;
        let otp = factor.otp.as_ref();
        let changed = conn.execute(
            "UPDATE account_factors SET label = ?2, kind = ?3, otp_kind = ?4, algorithm = ?5, \
             digits = ?6, period_seconds = ?7, counter = ?8, issuer = ?9, account_label = ?10, \
             seed_ref = ?11, updated_at = ?12 WHERE id = ?1",
            params![
                factor.id.to_string(),
                factor.label,
                factor.kind.as_str(),
                otp.map(|o| o.kind.as_uri_type()),
                otp.map(|o| o.algorithm.as_uri_value()),
                otp.map(|o| i64::from(o.digits.count())),
                otp.map(|o| i64::from(o.period_seconds)),
                otp.map(|o| i64::try_from(o.counter).unwrap_or(i64::MAX)),
                otp.and_then(|o| o.issuer.clone()),
                otp.and_then(|o| o.account_label.clone()),
                factor.seed_ref.map(|r| r.to_string()),
                factor.updated_at.as_unix_seconds(),
            ],
        )?;
        if changed == 0 {
            return Err(StorageError::not_found("second factor", factor.id));
        }
        Ok(())
    }

    /// Advances a HOTP factor's counter.
    ///
    /// Separated from [`Self::update`] because generating a HOTP code must
    /// persist the new counter immediately, without rewriting anything else.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] when no such factor exists.
    pub fn set_counter(conn: &Connection, id: FactorId, counter: u64) -> StorageResult<()> {
        let changed = conn.execute(
            "UPDATE account_factors SET counter = ?2, updated_at = ?3 WHERE id = ?1",
            params![
                id.to_string(),
                i64::try_from(counter).unwrap_or(i64::MAX),
                Timestamp::now().as_unix_seconds()
            ],
        )?;
        if changed == 0 {
            return Err(StorageError::not_found("second factor", id));
        }
        Ok(())
    }

    /// Deletes a factor.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] when no such factor exists.
    pub fn delete(conn: &Connection, id: FactorId) -> StorageResult<()> {
        let changed = conn.execute(
            "DELETE FROM account_factors WHERE id = ?1",
            params![id.to_string()],
        )?;
        if changed == 0 {
            return Err(StorageError::not_found("second factor", id));
        }
        Ok(())
    }

    /// Fetches one factor.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] when no such factor exists.
    pub fn get(conn: &Connection, id: FactorId) -> StorageResult<SecondFactor> {
        conn.query_row(
            &format!("{SELECT} WHERE id = ?1"),
            params![id.to_string()],
            map_factor,
        )
        .optional()?
        .ok_or_else(|| StorageError::not_found("second factor", id))
    }

    /// Lists an account's factors, oldest first.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn list_for_account(conn: &Connection, account_id: AccountId) -> StorageResult<Vec<SecondFactor>> {
        let mut stmt = conn.prepare(&format!(
            "{SELECT} WHERE account_id = ?1 ORDER BY created_at, label"
        ))?;
        let rows = stmt.query_map(params![account_id.to_string()], map_factor)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Every vault reference held by a factor seed.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn seed_refs(conn: &Connection) -> StorageResult<Vec<SecretRef>> {
        let mut stmt = conn.prepare("SELECT seed_ref FROM account_factors WHERE seed_ref IS NOT NULL")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut refs = Vec::new();
        for value in rows {
            refs.push(
                value?
                    .parse()
                    .map_err(|_| StorageError::Corrupt("a stored seed reference is not a UUID".to_owned()))?,
            );
        }
        Ok(refs)
    }
}

fn map_factor(row: &Row<'_>) -> rusqlite::Result<SecondFactor> {
    let id: String = row.get(0)?;
    let account_id: String = row.get(1)?;
    let kind_text: String = row.get(3)?;
    let kind: SecondFactorKind = kind_text
        .parse()
        .map_err(|_| bad_column(3, "unknown second factor kind"))?;
    let otp_kind: Option<String> = row.get(4)?;
    let seed_ref: Option<String> = row.get(11)?;

    let otp = match (kind, otp_kind) {
        (SecondFactorKind::Otp, Some(kind_text)) => {
            let algorithm: Option<String> = row.get(5)?;
            let digits: Option<i64> = row.get(6)?;
            let period: Option<i64> = row.get(7)?;
            let counter: Option<i64> = row.get(8)?;
            Some(OtpParameters {
                kind: kind_text
                    .parse::<OtpKind>()
                    .map_err(|_| bad_column(4, "unknown OTP kind"))?,
                algorithm: algorithm
                    .as_deref()
                    .unwrap_or("SHA1")
                    .parse::<OtpAlgorithm>()
                    .map_err(|_| bad_column(5, "unknown OTP algorithm"))?,
                digits: OtpDigits::try_from(u8::try_from(digits.unwrap_or(6)).unwrap_or(6))
                    .map_err(|_| bad_column(6, "unsupported digit count"))?,
                period_seconds: u32::try_from(period.unwrap_or(30)).unwrap_or(30),
                counter: u64::try_from(counter.unwrap_or(0)).unwrap_or(0),
                issuer: row.get(9)?,
                account_label: row.get(10)?,
            })
        }
        _ => None,
    };

    Ok(SecondFactor {
        id: id.parse().map_err(|_| bad_column(0, "factor id is not a UUID"))?,
        account_id: account_id
            .parse()
            .map_err(|_| bad_column(1, "account id is not a UUID"))?,
        label: row.get(2)?,
        kind,
        otp,
        seed_ref: match seed_ref {
            Some(raw) => Some(
                raw.parse()
                    .map_err(|_| bad_column(11, "seed reference is not a UUID"))?,
            ),
            None => None,
        },
        created_at: Timestamp::from_unix_seconds(row.get(12)?),
        updated_at: Timestamp::from_unix_seconds(row.get(13)?),
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

    fn otp_factor(account_id: AccountId) -> SecondFactor {
        let now = Timestamp::from_unix_seconds(1_700_000_000);
        SecondFactor {
            id: FactorId::new(),
            account_id,
            label: "Authenticator".to_owned(),
            kind: SecondFactorKind::Otp,
            otp: Some(OtpParameters {
                kind: OtpKind::Totp,
                algorithm: OtpAlgorithm::Sha512,
                digits: OtpDigits::Eight,
                period_seconds: 45,
                counter: 0,
                issuer: Some("Example Co".to_owned()),
                account_label: Some("alice@example.test".to_owned()),
            }),
            seed_ref: Some(SecretRef::new()),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn otp_factors_round_trip_with_every_parameter() {
        let db = Database::open_in_memory().expect("open");
        let account_id = account(&db);
        let factor = otp_factor(account_id);
        SecondFactorRepo::insert(db.connection(), &factor).expect("insert");
        assert_eq!(
            SecondFactorRepo::get(db.connection(), factor.id).expect("get"),
            factor
        );
    }

    #[test]
    fn external_authenticator_factors_carry_no_otp_state() {
        let db = Database::open_in_memory().expect("open");
        let account_id = account(&db);
        let now = Timestamp::from_unix_seconds(1_700_000_000);
        let factor = SecondFactor {
            id: FactorId::new(),
            account_id,
            label: "Microsoft Authenticator push".to_owned(),
            kind: SecondFactorKind::ExternalAuthenticator,
            otp: None,
            seed_ref: None,
            created_at: now,
            updated_at: now,
        };
        SecondFactorRepo::insert(db.connection(), &factor).expect("insert");
        let loaded = SecondFactorRepo::get(db.connection(), factor.id).expect("get");
        assert_eq!(loaded, factor);
        assert!(!loaded.generates_codes());
    }

    #[test]
    fn an_inconsistent_factor_is_refused() {
        let db = Database::open_in_memory().expect("open");
        let account_id = account(&db);
        let mut broken = otp_factor(account_id);
        broken.seed_ref = None;
        assert!(matches!(
            SecondFactorRepo::insert(db.connection(), &broken),
            Err(StorageError::Conflict(_))
        ));
    }

    #[test]
    fn a_hotp_counter_can_be_advanced_on_its_own() {
        let db = Database::open_in_memory().expect("open");
        let account_id = account(&db);
        let mut factor = otp_factor(account_id);
        if let Some(otp) = factor.otp.as_mut() {
            otp.kind = OtpKind::Hotp;
            otp.counter = 4;
        }
        SecondFactorRepo::insert(db.connection(), &factor).expect("insert");
        SecondFactorRepo::set_counter(db.connection(), factor.id, 5).expect("advance");
        let loaded = SecondFactorRepo::get(db.connection(), factor.id).expect("get");
        assert_eq!(loaded.otp.expect("otp").counter, 5);
        assert_eq!(
            loaded.label, factor.label,
            "advancing the counter changes nothing else"
        );
    }

    #[test]
    fn factors_are_listed_per_account_and_cascade_on_delete() {
        let db = Database::open_in_memory().expect("open");
        let account_id = account(&db);
        let other_account = account(&db);
        SecondFactorRepo::insert(db.connection(), &otp_factor(account_id)).expect("insert");
        SecondFactorRepo::insert(db.connection(), &otp_factor(account_id)).expect("insert");
        SecondFactorRepo::insert(db.connection(), &otp_factor(other_account)).expect("insert");

        assert_eq!(
            SecondFactorRepo::list_for_account(db.connection(), account_id)
                .expect("list")
                .len(),
            2
        );
        AccountRepo::delete(db.connection(), account_id).expect("delete account");
        assert!(
            SecondFactorRepo::list_for_account(db.connection(), account_id)
                .expect("list")
                .is_empty()
        );
        assert_eq!(
            SecondFactorRepo::list_for_account(db.connection(), other_account)
                .expect("list")
                .len(),
            1
        );
    }

    #[test]
    fn missing_factors_are_reported() {
        let db = Database::open_in_memory().expect("open");
        let id = FactorId::new();
        assert!(matches!(
            SecondFactorRepo::get(db.connection(), id),
            Err(StorageError::NotFound { .. })
        ));
        assert!(matches!(
            SecondFactorRepo::delete(db.connection(), id),
            Err(StorageError::NotFound { .. })
        ));
        assert!(matches!(
            SecondFactorRepo::set_counter(db.connection(), id, 1),
            Err(StorageError::NotFound { .. })
        ));
    }

    #[test]
    fn seed_references_can_be_enumerated_for_orphan_collection() {
        let db = Database::open_in_memory().expect("open");
        let account_id = account(&db);
        let factor = otp_factor(account_id);
        SecondFactorRepo::insert(db.connection(), &factor).expect("insert");
        assert_eq!(
            SecondFactorRepo::seed_refs(db.connection()).expect("refs"),
            vec![factor.seed_ref.expect("set")]
        );
    }

    #[test]
    fn no_secret_material_is_stored_in_the_factor_table() {
        let db = Database::open_in_memory().expect("open");
        let account_id = account(&db);
        SecondFactorRepo::insert(db.connection(), &otp_factor(account_id)).expect("insert");
        let stmt = db
            .connection()
            .prepare("SELECT * FROM account_factors")
            .expect("prepare");
        let columns: Vec<String> = stmt.column_names().into_iter().map(str::to_owned).collect();
        assert!(
            !columns.iter().any(|c| c == "secret" || c == "seed"),
            "{columns:?}"
        );
        assert!(columns.iter().any(|c| c == "seed_ref"));
    }
}
