//! Account, second-factor and recovery-code services.
//!
//! These are the operations that move secret material between the vault and the
//! metadata database. The rule they all follow: metadata is written only after
//! the secret it references exists, and a secret is removed only after nothing
//! references it.

use tw_domain::{
    Account, AccountDraft, AccountId, DiagnosticCode, FactorId, OtpKind, OtpParameters, RecoveryCode,
    RecoveryCodeId, SecondFactor, SecondFactorDraft, SecondFactorKind, SecretRef, Timestamp,
    validate_display_name,
};
use tw_otp::{OtpCode, OtpSecret};
use tw_secrets::SecretString;
use tw_storage::{AccountRepo, RecoveryCodeRepo, SecondFactorRepo};
use tw_vault::SecretKind;

use crate::error::{AppError, AppResult};
use crate::vault::VaultService;

/// Creates an account, storing its password in the vault first.
///
/// # Errors
///
/// Returns [`DiagnosticCode::InvalidInput`] for a bad draft,
/// [`DiagnosticCode::VaultLocked`] when a password is supplied and the vault is
/// locked, and a storage error on a write failure.
pub fn create_account(
    conn: &rusqlite_conn::Connection,
    vault: &mut VaultService,
    draft: &AccountDraft,
    password: Option<SecretString>,
) -> AppResult<Account> {
    let normalized = draft.normalize()?;
    let password_ref = match password {
        Some(value) if !value.is_empty() => {
            vault.require_unlocked()?;
            Some(vault.store(SecretKind::Password, value)?)
        }
        _ => None,
    };
    let now = Timestamp::now();
    let account = Account {
        id: AccountId::new(),
        display_name: normalized.display_name,
        service: normalized.service,
        username: normalized.username,
        email: normalized.email,
        login_url: normalized.login_url,
        tags: normalized.tags,
        notes: normalized.notes,
        password_ref,
        created_at: now,
        updated_at: now,
    };
    // If this fails, the stored secret is orphaned rather than lost; the next
    // orphan collection removes it.
    AccountRepo::insert(conn, &account)?;
    Ok(account)
}

/// Updates an account's metadata. Does not touch its password.
///
/// # Errors
///
/// Returns [`DiagnosticCode::InvalidInput`] or a storage error.
pub fn update_account(
    conn: &rusqlite_conn::Connection,
    id: AccountId,
    draft: &AccountDraft,
) -> AppResult<Account> {
    let existing = AccountRepo::get(conn, id)?;
    let normalized = draft.normalize()?;
    let updated = Account {
        display_name: normalized.display_name,
        service: normalized.service,
        username: normalized.username,
        email: normalized.email,
        login_url: normalized.login_url,
        tags: normalized.tags,
        notes: normalized.notes,
        updated_at: Timestamp::now(),
        ..existing
    };
    AccountRepo::update(conn, &updated)?;
    Ok(updated)
}

/// Sets or clears an account's password.
///
/// Replaces the value behind the existing reference where possible, so nothing
/// else has to be rewritten.
///
/// # Errors
///
/// Returns [`DiagnosticCode::VaultLocked`] or a storage error.
pub fn set_account_password(
    conn: &rusqlite_conn::Connection,
    vault: &mut VaultService,
    id: AccountId,
    password: Option<SecretString>,
) -> AppResult<Account> {
    vault.require_unlocked()?;
    let mut account = AccountRepo::get(conn, id)?;
    match (account.password_ref, password) {
        (Some(reference), Some(value)) if !value.is_empty() => {
            vault.replace(reference, value)?;
        }
        (None, Some(value)) if !value.is_empty() => {
            account.password_ref = Some(vault.store(SecretKind::Password, value)?);
        }
        (Some(reference), _) => {
            // Clear: drop the metadata reference first, so a failure to remove
            // the secret leaves an orphan rather than a dangling reference.
            account.password_ref = None;
            account.updated_at = Timestamp::now();
            AccountRepo::update(conn, &account)?;
            vault.forget(reference)?;
            return Ok(account);
        }
        (None, _) => return Ok(account),
    }
    account.updated_at = Timestamp::now();
    AccountRepo::update(conn, &account)?;
    Ok(account)
}

/// Reveals an account's password.
///
/// # Errors
///
/// Returns [`DiagnosticCode::VaultLocked`] or
/// [`DiagnosticCode::SecretNotFound`].
pub fn reveal_account_password(
    conn: &rusqlite_conn::Connection,
    vault: &mut VaultService,
    id: AccountId,
) -> AppResult<SecretString> {
    let account = AccountRepo::get(conn, id)?;
    let reference = account.password_ref.ok_or_else(|| {
        AppError::new(
            DiagnosticCode::SecretNotFound,
            "this account has no stored password",
        )
    })?;
    vault.reveal(reference)
}

/// Deletes an account and every secret only it referenced.
///
/// # Errors
///
/// Returns a storage error, or [`DiagnosticCode::VaultLocked`] when the vault
/// is locked and there are secrets to remove.
pub fn delete_account(
    conn: &rusqlite_conn::Connection,
    vault: &mut VaultService,
    id: AccountId,
) -> AppResult<()> {
    let account = AccountRepo::get(conn, id)?;
    let factors = SecondFactorRepo::list_for_account(conn, id)?;
    let codes = RecoveryCodeRepo::list_for_account(conn, id)?;

    let mut orphaned: Vec<SecretRef> = Vec::new();
    orphaned.extend(account.password_ref);
    orphaned.extend(factors.iter().filter_map(|f| f.seed_ref));
    orphaned.extend(codes.iter().map(|c| c.code_ref));

    // Metadata first: a crash between the two leaves orphaned secrets, which
    // orphan collection cleans up. The reverse order would leave metadata
    // pointing at secrets that no longer exist.
    AccountRepo::delete(conn, id)?;

    if !orphaned.is_empty() && vault.is_unlocked() {
        for reference in orphaned {
            if let Err(error) = vault.forget(reference) {
                tracing::warn!(code = %error.code, "a secret could not be removed and was left orphaned");
            }
        }
    }
    Ok(())
}

/// Adds an OTP second factor, storing the shared secret in the vault.
///
/// # Errors
///
/// Returns [`DiagnosticCode::VaultLocked`], [`DiagnosticCode::InvalidInput`] or
/// a storage error.
pub fn add_otp_factor(
    conn: &rusqlite_conn::Connection,
    vault: &mut VaultService,
    account_id: AccountId,
    label: &str,
    parameters: &OtpParameters,
    secret: &OtpSecret,
) -> AppResult<SecondFactor> {
    vault.require_unlocked()?;
    parameters.validate()?;
    let label = validate_display_name(label)?;
    // Verify the account exists before writing a secret for it.
    let _ = AccountRepo::get(conn, account_id)?;

    let seed_ref = vault.store(SecretKind::OtpSeed, secret.to_base32())?;
    let now = Timestamp::now();
    let factor = SecondFactor {
        id: FactorId::new(),
        account_id,
        label,
        kind: SecondFactorKind::Otp,
        otp: Some(parameters.clone()),
        seed_ref: Some(seed_ref),
        created_at: now,
        updated_at: now,
    };
    SecondFactorRepo::insert(conn, &factor)?;
    Ok(factor)
}

/// Records a factor handled entirely by another application or device.
///
/// Vendor push approvals, number matching, passwordless sign-in and hardware
/// keys are **not** TOTP and are never emulated. They are recorded so the user
/// knows the factor exists.
///
/// # Errors
///
/// Returns [`DiagnosticCode::InvalidInput`] or a storage error.
pub fn add_external_factor(
    conn: &rusqlite_conn::Connection,
    account_id: AccountId,
    draft: &SecondFactorDraft,
) -> AppResult<SecondFactor> {
    let label = validate_display_name(&draft.label)?;
    let _ = AccountRepo::get(conn, account_id)?;
    let now = Timestamp::now();
    let factor = SecondFactor {
        id: FactorId::new(),
        account_id,
        label,
        kind: SecondFactorKind::ExternalAuthenticator,
        otp: None,
        seed_ref: None,
        created_at: now,
        updated_at: now,
    };
    SecondFactorRepo::insert(conn, &factor)?;
    Ok(factor)
}

/// Generates the current code for a factor.
///
/// For HOTP this advances and persists the counter, because a counter-based code
/// is single use: handing out the same code twice would be worse than losing
/// one.
///
/// # Errors
///
/// Returns [`DiagnosticCode::VaultLocked`], [`DiagnosticCode::RecordNotFound`]
/// or [`DiagnosticCode::OtpSecretInvalid`].
pub fn generate_code(
    conn: &rusqlite_conn::Connection,
    vault: &mut VaultService,
    factor_id: FactorId,
) -> AppResult<OtpCode> {
    vault.require_unlocked()?;
    let factor = SecondFactorRepo::get(conn, factor_id)?;
    let parameters = factor.otp.clone().ok_or_else(|| {
        AppError::new(
            DiagnosticCode::OtpParametersInvalid,
            "this factor is handled by another application and produces no code here",
        )
    })?;
    let seed_ref = factor
        .seed_ref
        .ok_or_else(|| AppError::new(DiagnosticCode::SecretNotFound, "this factor has no stored secret"))?;
    let encoded = vault.reveal(seed_ref)?;
    let secret = OtpSecret::from_base32(encoded.expose()).map_err(|_| {
        AppError::new(
            DiagnosticCode::OtpSecretInvalid,
            "the stored secret for this factor is not usable",
        )
        .with_remedy("Remove the factor and import it again from the issuer.")
    })?;

    let code = tw_otp::generate(&secret, &parameters);
    if parameters.kind == OtpKind::Hotp {
        SecondFactorRepo::set_counter(conn, factor_id, parameters.counter.saturating_add(1))?;
    }
    Ok(code)
}

/// Deletes a factor and the secret it referenced.
///
/// # Errors
///
/// Returns a storage error.
pub fn delete_factor(
    conn: &rusqlite_conn::Connection,
    vault: &mut VaultService,
    factor_id: FactorId,
) -> AppResult<()> {
    let factor = SecondFactorRepo::get(conn, factor_id)?;
    SecondFactorRepo::delete(conn, factor_id)?;
    if let Some(reference) = factor.seed_ref
        && vault.is_unlocked()
        && let Err(error) = vault.forget(reference)
    {
        tracing::warn!(code = %error.code, "a factor secret could not be removed and was left orphaned");
    }
    Ok(())
}

/// Stores a batch of recovery codes.
///
/// # Errors
///
/// Returns [`DiagnosticCode::VaultLocked`] or a storage error.
pub fn add_recovery_codes(
    conn: &rusqlite_conn::Connection,
    vault: &mut VaultService,
    account_id: AccountId,
    codes: Vec<SecretString>,
) -> AppResult<Vec<RecoveryCode>> {
    vault.require_unlocked()?;
    let _ = AccountRepo::get(conn, account_id)?;
    let mut position = RecoveryCodeRepo::next_position(conn, account_id)?;
    let mut stored = Vec::new();
    for code in codes {
        if code.expose().trim().is_empty() {
            continue;
        }
        let code_ref = vault.store(SecretKind::RecoveryCode, code)?;
        let record = RecoveryCode {
            id: RecoveryCodeId::new(),
            account_id,
            code_ref,
            position,
            used: false,
            used_at: None,
            created_at: Timestamp::now(),
        };
        RecoveryCodeRepo::insert(conn, &record)?;
        stored.push(record);
        position += 1;
    }
    Ok(stored)
}

/// Splits pasted text into individual recovery codes.
///
/// Issuers hand out codes one per line, sometimes with a numbered prefix or
/// surrounding blank lines.
#[must_use]
pub fn split_recovery_codes(raw: &str) -> Vec<SecretString> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            // Strip a leading "1." or "1)" list marker.
            let stripped = line
                .split_once(['.', ')'])
                .filter(|(prefix, _)| !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()))
                .map_or(line, |(_, rest)| rest.trim());
            SecretString::new(stripped)
        })
        .filter(|code| !code.is_empty())
        .collect()
}

/// Marks a recovery code used or unused.
///
/// # Errors
///
/// Returns a storage error.
pub fn set_recovery_code_used(
    conn: &rusqlite_conn::Connection,
    id: RecoveryCodeId,
    used: bool,
) -> AppResult<RecoveryCode> {
    RecoveryCodeRepo::set_used(conn, id, used)?;
    Ok(RecoveryCodeRepo::get(conn, id)?)
}

/// Reveals a recovery code.
///
/// # Errors
///
/// Returns [`DiagnosticCode::VaultLocked`] or a storage error.
pub fn reveal_recovery_code(
    conn: &rusqlite_conn::Connection,
    vault: &mut VaultService,
    id: RecoveryCodeId,
) -> AppResult<SecretString> {
    let code = RecoveryCodeRepo::get(conn, id)?;
    vault.reveal(code.code_ref)
}

/// Deletes a recovery code and its stored value.
///
/// # Errors
///
/// Returns a storage error.
pub fn delete_recovery_code(
    conn: &rusqlite_conn::Connection,
    vault: &mut VaultService,
    id: RecoveryCodeId,
) -> AppResult<()> {
    let code = RecoveryCodeRepo::get(conn, id)?;
    RecoveryCodeRepo::delete(conn, id)?;
    if vault.is_unlocked()
        && let Err(error) = vault.forget(code.code_ref)
    {
        tracing::warn!(code = %error.code, "a recovery code could not be removed and was left orphaned");
    }
    Ok(())
}

/// Every vault reference any metadata record still points at.
///
/// # Errors
///
/// Returns a storage error.
pub fn live_secret_refs(
    conn: &rusqlite_conn::Connection,
) -> AppResult<std::collections::BTreeSet<SecretRef>> {
    let mut live = std::collections::BTreeSet::new();
    live.extend(AccountRepo::password_refs(conn)?);
    live.extend(SecondFactorRepo::seed_refs(conn)?);
    live.extend(RecoveryCodeRepo::code_refs(conn)?);
    Ok(live)
}

/// Re-exported so this module's signatures do not leak the SQLite crate name
/// into every call site's imports.
pub(crate) mod rusqlite_conn {
    pub use tw_storage::rusqlite::Connection;
}

#[cfg(test)]
mod tests {
    use tw_domain::{OtpAlgorithm, OtpDigits, ServiceKind};
    use tw_storage::Database;

    use super::*;
    use crate::vault::VaultService;

    fn setup() -> (tempfile::TempDir, Database, VaultService) {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = Database::open_in_memory().expect("db");
        let mut vault = VaultService::new(dir.path().join("v.twvault"), Default::default());
        vault
            .create(&SecretString::new("correct horse battery staple"))
            .expect("create vault");
        (dir, db, vault)
    }

    fn draft(name: &str) -> AccountDraft {
        AccountDraft {
            display_name: name.to_owned(),
            service: Some(ServiceKind::GitHub),
            ..Default::default()
        }
    }

    /// A synthetic secret. Never a real credential.
    fn synthetic_secret() -> OtpSecret {
        OtpSecret::from_base32("JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP").expect("decode")
    }

    #[test]
    fn an_account_password_is_stored_in_the_vault_not_the_database() {
        let (_dir, db, mut vault) = setup();
        let account = create_account(
            db.connection(),
            &mut vault,
            &draft("Bot"),
            Some(SecretString::new("hunter2")),
        )
        .expect("create");

        assert!(account.password_ref.is_some());
        assert_eq!(
            reveal_account_password(db.connection(), &mut vault, account.id)
                .expect("reveal")
                .expose(),
            "hunter2"
        );

        // Nothing in the database contains the plaintext.
        let mut stmt = db
            .connection()
            .prepare("SELECT * FROM accounts")
            .expect("prepare");
        let mut rows = stmt.query([]).expect("query");
        while let Some(row) = rows.next().expect("row") {
            for index in 0..row.as_ref().column_count() {
                if let Ok(text) = row.get::<_, String>(index) {
                    assert!(!text.contains("hunter2"), "column {index} leaked the password");
                }
            }
        }
    }

    #[test]
    fn an_account_can_be_created_without_a_password() {
        let (_dir, db, mut vault) = setup();
        let account =
            create_account(db.connection(), &mut vault, &draft("No password"), None).expect("create");
        assert_eq!(account.password_ref, None);
        assert!(reveal_account_password(db.connection(), &mut vault, account.id).is_err());
    }

    #[test]
    fn setting_a_password_reuses_the_existing_reference() {
        let (_dir, db, mut vault) = setup();
        let account = create_account(
            db.connection(),
            &mut vault,
            &draft("Bot"),
            Some(SecretString::new("first")),
        )
        .expect("create");
        let original_ref = account.password_ref.expect("set");

        let updated = set_account_password(
            db.connection(),
            &mut vault,
            account.id,
            Some(SecretString::new("second")),
        )
        .expect("update");
        assert_eq!(
            updated.password_ref,
            Some(original_ref),
            "the reference is stable across edits"
        );
        assert_eq!(
            reveal_account_password(db.connection(), &mut vault, account.id)
                .expect("reveal")
                .expose(),
            "second"
        );
    }

    #[test]
    fn clearing_a_password_removes_it_from_the_vault() {
        let (_dir, db, mut vault) = setup();
        let account = create_account(
            db.connection(),
            &mut vault,
            &draft("Bot"),
            Some(SecretString::new("hunter2")),
        )
        .expect("create");
        let reference = account.password_ref.expect("set");

        let cleared = set_account_password(db.connection(), &mut vault, account.id, None).expect("clear");
        assert_eq!(cleared.password_ref, None);
        assert!(
            vault.reveal(reference).is_err(),
            "the secret is gone from the vault too"
        );
    }

    #[test]
    fn deleting_an_account_removes_every_secret_it_owned() {
        let (_dir, db, mut vault) = setup();
        let account = create_account(
            db.connection(),
            &mut vault,
            &draft("Bot"),
            Some(SecretString::new("hunter2")),
        )
        .expect("create");
        let factor = add_otp_factor(
            db.connection(),
            &mut vault,
            account.id,
            "Authenticator",
            &OtpParameters::default(),
            &synthetic_secret(),
        )
        .expect("factor");
        let codes = add_recovery_codes(
            db.connection(),
            &mut vault,
            account.id,
            vec![SecretString::new("aaaa-bbbb")],
        )
        .expect("codes");

        let password_ref = account.password_ref.expect("set");
        let seed_ref = factor.seed_ref.expect("set");
        let code_ref = codes[0].code_ref;

        delete_account(db.connection(), &mut vault, account.id).expect("delete");

        for reference in [password_ref, seed_ref, code_ref] {
            assert!(
                vault.reveal(reference).is_err(),
                "{reference} survived the deletion"
            );
        }
        assert!(live_secret_refs(db.connection()).expect("refs").is_empty());
    }

    #[test]
    fn a_totp_factor_generates_a_code_and_does_not_advance_a_counter() {
        let (_dir, db, mut vault) = setup();
        let account = create_account(db.connection(), &mut vault, &draft("Bot"), None).expect("create");
        let factor = add_otp_factor(
            db.connection(),
            &mut vault,
            account.id,
            "Authenticator",
            &OtpParameters {
                algorithm: OtpAlgorithm::Sha1,
                digits: OtpDigits::Six,
                ..Default::default()
            },
            &synthetic_secret(),
        )
        .expect("factor");

        let first = generate_code(db.connection(), &mut vault, factor.id).expect("code");
        assert_eq!(first.code.len(), 6);
        assert!(first.valid_for_seconds.is_some());

        let second = generate_code(db.connection(), &mut vault, factor.id).expect("code");
        assert_eq!(first.code, second.code, "a TOTP code is stable within its step");
        assert_eq!(
            SecondFactorRepo::get(db.connection(), factor.id)
                .expect("get")
                .otp
                .expect("otp")
                .counter,
            0
        );
    }

    #[test]
    fn a_hotp_factor_advances_its_counter_so_a_code_is_never_reissued() {
        let (_dir, db, mut vault) = setup();
        let account = create_account(db.connection(), &mut vault, &draft("Bot"), None).expect("create");
        let factor = add_otp_factor(
            db.connection(),
            &mut vault,
            account.id,
            "Hardware token",
            &OtpParameters {
                kind: OtpKind::Hotp,
                counter: 0,
                ..Default::default()
            },
            &synthetic_secret(),
        )
        .expect("factor");

        let first = generate_code(db.connection(), &mut vault, factor.id).expect("code");
        assert_eq!(first.counter, 0);
        assert_eq!(first.valid_for_seconds, None);

        let second = generate_code(db.connection(), &mut vault, factor.id).expect("code");
        assert_eq!(second.counter, 1);
        assert_ne!(first.code, second.code, "a counter-based code must never repeat");
        assert_eq!(
            SecondFactorRepo::get(db.connection(), factor.id)
                .expect("get")
                .otp
                .expect("otp")
                .counter,
            2
        );
    }

    #[test]
    fn an_external_factor_is_recorded_but_produces_no_code() {
        let (_dir, db, mut vault) = setup();
        let account = create_account(db.connection(), &mut vault, &draft("Bot"), None).expect("create");
        let factor = add_external_factor(
            db.connection(),
            account.id,
            &SecondFactorDraft {
                label: "Microsoft Authenticator push".to_owned(),
                kind: SecondFactorKind::ExternalAuthenticator,
                otp: None,
            },
        )
        .expect("factor");

        assert!(!factor.generates_codes());
        let error = generate_code(db.connection(), &mut vault, factor.id).expect_err("no code");
        assert_eq!(error.code, DiagnosticCode::OtpParametersInvalid);
        assert!(error.message.contains("another application"), "{}", error.message);
    }

    #[test]
    fn recovery_codes_are_stored_encrypted_and_marked_used_with_a_timestamp() {
        let (_dir, db, mut vault) = setup();
        let account = create_account(db.connection(), &mut vault, &draft("Bot"), None).expect("create");
        let codes = add_recovery_codes(
            db.connection(),
            &mut vault,
            account.id,
            split_recovery_codes("1. aaaa-bbbb\n2) cccc-dddd\n\n   eeee-ffff  \n"),
        )
        .expect("codes");

        assert_eq!(codes.len(), 3);
        assert_eq!(
            reveal_recovery_code(db.connection(), &mut vault, codes[0].id)
                .expect("reveal")
                .expose(),
            "aaaa-bbbb"
        );
        assert_eq!(
            reveal_recovery_code(db.connection(), &mut vault, codes[1].id)
                .expect("reveal")
                .expose(),
            "cccc-dddd"
        );
        assert_eq!(
            RecoveryCodeRepo::unused_count(db.connection(), account.id).expect("count"),
            3
        );

        let used = set_recovery_code_used(db.connection(), codes[0].id, true).expect("mark");
        assert!(used.used);
        assert!(used.used_at.is_some());
        assert_eq!(
            RecoveryCodeRepo::unused_count(db.connection(), account.id).expect("count"),
            2
        );

        delete_recovery_code(db.connection(), &mut vault, codes[2].id).expect("delete");
        assert!(vault.reveal(codes[2].code_ref).is_err());
    }

    #[test]
    fn pasted_recovery_codes_are_split_and_cleaned() {
        let codes = split_recovery_codes("  \n1. AAAA-BBBB\n\n2. CCCC-DDDD\nEEEE-FFFF\n  \n");
        let values: Vec<&str> = codes.iter().map(tw_secrets::SecretString::expose).collect();
        assert_eq!(values, vec!["AAAA-BBBB", "CCCC-DDDD", "EEEE-FFFF"]);
        assert!(split_recovery_codes("").is_empty());
        assert!(split_recovery_codes("   \n\n  ").is_empty());
    }

    #[test]
    fn a_locked_vault_refuses_every_secret_operation() {
        let (_dir, db, mut vault) = setup();
        let account = create_account(
            db.connection(),
            &mut vault,
            &draft("Bot"),
            Some(SecretString::new("hunter2")),
        )
        .expect("create");
        vault.lock(tw_vault::LockReason::Manual);

        assert!(reveal_account_password(db.connection(), &mut vault, account.id).is_err());
        assert!(
            create_account(
                db.connection(),
                &mut vault,
                &draft("Another"),
                Some(SecretString::new("x"))
            )
            .is_err()
        );
        assert!(
            add_otp_factor(
                db.connection(),
                &mut vault,
                account.id,
                "A",
                &OtpParameters::default(),
                &synthetic_secret()
            )
            .is_err()
        );
        assert!(
            add_recovery_codes(
                db.connection(),
                &mut vault,
                account.id,
                vec![SecretString::new("x")]
            )
            .is_err()
        );

        // Metadata-only work still succeeds while locked.
        assert!(update_account(db.connection(), account.id, &draft("Renamed")).is_ok());
    }

    #[test]
    fn live_references_cover_every_kind_of_secret() {
        let (_dir, db, mut vault) = setup();
        let account = create_account(
            db.connection(),
            &mut vault,
            &draft("Bot"),
            Some(SecretString::new("hunter2")),
        )
        .expect("create");
        add_otp_factor(
            db.connection(),
            &mut vault,
            account.id,
            "Authenticator",
            &OtpParameters::default(),
            &synthetic_secret(),
        )
        .expect("factor");
        add_recovery_codes(
            db.connection(),
            &mut vault,
            account.id,
            vec![SecretString::new("aaaa")],
        )
        .expect("codes");

        assert_eq!(live_secret_refs(db.connection()).expect("refs").len(), 3);
        assert_eq!(
            vault
                .collect_orphans(&live_secret_refs(db.connection()).expect("refs"))
                .expect("collect"),
            0,
            "nothing referenced may be collected"
        );
    }
}
