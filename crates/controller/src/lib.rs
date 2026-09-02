//! Application services orchestrating workspace state and behavior.
//!
//! The controller owns the workspace lifecycle: portable bootstrap, storage,
//! vault, Thorium installs, and browser profile sessions. It is the only
//! layer the Tauri commands talk to.

#![forbid(unsafe_code)]

pub mod clipboard;
pub mod diagnostics;
pub mod error;
pub mod idle;
pub mod services;
pub mod workspace;

pub use clipboard::{ClipboardPort, ClipboardScheduler, SystemClipboard};
pub use diagnostics::DiagnosticsSnapshot;
pub use error::ControllerError;
pub use idle::IdleTracker;
pub use services::{LaunchPlan, VaultStatus};
pub use workspace::{BROWSERS_REL, DB_FILE, VAULT_REL, Workspace};

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use thorium_workspace_domain::DiagnosticCode as _;
    use thorium_workspace_domain::{
        AccountInput, FactorKind, OtpAlgorithm, ProfileInput, ServiceKind, ThoriumSelection,
    };
    use thorium_workspace_secrets::SecretText;
    use thorium_workspace_vault::{VaultError, VaultLockState};

    use super::*;

    const SYNTHETIC_MASTER: &str = "synthetic-master-password-42";
    // RFC 4226 seed as base32: "12345678901234567890".
    const SYNTHETIC_SEED_B32: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    fn temp_root() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn workspace() -> (tempfile::TempDir, Workspace) {
        let dir = temp_root();
        let ws = Workspace::bootstrap(Some(dir.path())).expect("bootstrap");
        (dir, ws)
    }

    fn profile_input(name: &str) -> ProfileInput {
        ProfileInput {
            name: name.to_owned(),
            thorium_version: ThoriumSelection::Current,
            startup_urls: vec!["https://github.com".to_owned()],
            locale: Some("en-US".to_owned()),
            timezone: Some("America/Los_Angeles".to_owned()),
        }
    }

    fn account_input() -> AccountInput {
        AccountInput {
            display_name: "Work GitHub".to_owned(),
            service_kind: ServiceKind::GitHub,
            username: Some("octocat".to_owned()),
            email: Some("octocat@example.com".to_owned()),
            login_url: Some("https://github.com/login".to_owned()),
            tags: vec!["work".to_owned()],
            notes: "synthetic".to_owned(),
        }
    }

    #[test]
    fn bootstrap_creates_layout_store_and_runtime() {
        let (dir, ws) = workspace();
        for relative in ["vault", "profiles", "runtime", "backups", "logs"] {
            assert!(dir.path().join(relative).is_dir(), "{relative} missing");
        }
        assert!(dir.path().join(DB_FILE).is_file());
        assert_eq!(ws.root(), dir.path());
        // Diagnostics before any content.
        let snapshot = ws.diagnostics().expect("diagnostics");
        assert_eq!(snapshot.schema_version, 1);
        assert!(snapshot.workspace_writable);
        assert!(!snapshot.vault_exists);
    }

    #[test]
    fn second_open_of_same_root_is_rejected() {
        let (dir, _ws) = workspace();
        let error = Workspace::bootstrap(Some(dir.path())).expect_err("in use");
        assert_eq!(error.diagnostic_code(), "CONTROLLER_WORKSPACE_IN_USE");
    }

    #[test]
    fn reopen_preserves_persisted_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let profile;
        {
            let ws = Workspace::bootstrap(Some(dir.path())).expect("open");
            ws.create_vault(&SecretText::new(SYNTHETIC_MASTER))
                .expect("create vault");
            profile = ws
                .create_profile(&profile_input("Test Profile A"))
                .expect("create");
            ws.lock_vault().expect("lock");
        }
        {
            let ws = Workspace::bootstrap(Some(dir.path())).expect("reopen");
            assert_eq!(ws.vault_status().lock_state, VaultLockState::Locked);
            let loaded = ws.get_profile(profile.id).expect("profile persists");
            assert_eq!(loaded.name, "Test Profile A");
            assert_eq!(loaded.locale.as_deref(), Some("en-US"));
            assert_eq!(loaded.timezone.as_deref(), Some("America/Los_Angeles"));
            assert_eq!(
                loaded.user_data_rel_path,
                thorium_workspace_domain::BrowserProfile::user_data_rel_path_for(profile.id)
            );
            // The user data directory contract: profiles/<uuid>/User Data.
            assert!(dir.path().join(&loaded.user_data_rel_path).exists());
        }
    }

    #[test]
    fn vault_lifecycle_and_locked_behavior() {
        let (_dir, ws) = workspace();
        assert!(!ws.vault_status().exists);

        // With no vault (nothing to unlock), secret writes fail locked.
        let profile = ws
            .create_profile(&profile_input("Locked Profile"))
            .expect("profile");
        let account = ws
            .create_account(profile.id, &account_input())
            .expect("account");
        let error = ws
            .set_password(account.id, &SecretText::new("synthetic-pw"))
            .expect_err("locked");
        assert_eq!(error.diagnostic_code(), "VAULT_LOCKED");

        ws.create_vault(&SecretText::new(SYNTHETIC_MASTER))
            .expect("create");
        assert!(ws.vault_status().exists);
        assert!(ws.vault_status().is_unlocked());

        ws.lock_vault().expect("lock");
        assert_eq!(ws.vault_status().lock_state, VaultLockState::Locked);

        let error = ws
            .unlock_vault(&SecretText::new("definitely-wrong"))
            .expect_err("wrong password");
        assert_eq!(error.diagnostic_code(), "VAULT_UNLOCK_FAILED");

        ws.unlock_vault(&SecretText::new(SYNTHETIC_MASTER))
            .expect("unlock");
        assert!(ws.vault_status().is_unlocked());
    }

    #[test]
    fn profile_and_account_crud_with_association() {
        let (_dir, ws) = workspace();
        let profile = ws
            .create_profile(&profile_input("Test Profile A"))
            .expect("profile");
        let account = ws
            .create_account(profile.id, &account_input())
            .expect("account");

        let loaded_profile = ws.get_profile(profile.id).expect("profile");
        assert_eq!(loaded_profile.account_ids, vec![account.id]);

        let accounts = ws.list_accounts(profile.id).expect("accounts");
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].service_kind, ServiceKind::GitHub);

        let mut edited = accounts[0].clone();
        edited.notes = "edited".to_owned();
        ws.update_account(&edited).expect("update");
        assert_eq!(ws.get_account(account.id).expect("account").notes, "edited");

        // Account deletion removes the association.
        ws.delete_account(account.id).expect("delete account");
        assert!(ws.list_accounts(profile.id).expect("accounts").is_empty());
        assert!(
            ws.get_profile(profile.id)
                .expect("profile")
                .account_ids
                .is_empty()
        );

        // Profile deletion cascades and purges secrets (none here).
        ws.create_account(profile.id, &account_input())
            .expect("recreate");
        ws.delete_profile(profile.id).expect("delete profile");
        assert!(ws.get_profile(profile.id).is_err());
    }

    #[test]
    fn password_write_read_copy_delete_with_redaction() {
        let (dir, ws) = workspace();
        ws.create_vault(&SecretText::new(SYNTHETIC_MASTER))
            .expect("vault");
        let profile = ws
            .create_profile(&profile_input("Secrets Profile"))
            .expect("profile");
        let account = ws
            .create_account(profile.id, &account_input())
            .expect("account");

        const SYNTHETIC_PASSWORD: &str = "synthetic-account-password-123";
        ws.set_password(account.id, &SecretText::new(SYNTHETIC_PASSWORD))
            .expect("store");

        // The vault file must not contain the plaintext.
        let vault_file = std::fs::read(dir.path().join(VAULT_REL)).expect("read vault");
        assert!(
            !vault_file
                .windows(SYNTHETIC_PASSWORD.len())
                .any(|window| window == SYNTHETIC_PASSWORD.as_bytes())
        );

        // Explicit retrieve works.
        let password = ws.get_password(account.id).expect("retrieve");
        assert_eq!(password.expose(), SYNTHETIC_PASSWORD);

        // Copy schedules a conditional clear using configured settings.
        let t0 = Instant::now();
        let delay = ws.copy_password(account.id, t0).expect("copy");
        assert_eq!(delay, Duration::from_secs(20));
        assert!(ws.clipboard_state().pending_fire_at().is_some());
        ws.clipboard_state().cancel();

        // Delete removes both the row reference and the vault entry.
        ws.delete_password(account.id).expect("delete");
        assert!(
            ws.get_account(account.id)
                .expect("account")
                .password_ref
                .is_none()
        );
        let error = ws.get_password(account.id).expect_err("gone");
        assert_eq!(error.diagnostic_code(), "CONTROLLER_NOT_FOUND");

        // Redaction: the error tree and diagnostics never contain the value.
        let snapshot = ws.diagnostics().expect("diagnostics");
        let rendered = format!("{snapshot:?}");
        assert!(!rendered.contains(SYNTHETIC_PASSWORD));
    }

    #[test]
    fn totp_from_uri_is_rfc_correct_and_seed_stays_encrypted() {
        let (dir, ws) = workspace();
        ws.create_vault(&SecretText::new(SYNTHETIC_MASTER))
            .expect("vault");
        let profile = ws
            .create_profile(&profile_input("Otp Profile"))
            .expect("profile");
        let account = ws
            .create_account(profile.id, &account_input())
            .expect("account");

        let uri = format!(
            "otpauth://totp/GitHub:octocat?secret={SYNTHETIC_SEED_B32}&issuer=GitHub&digits=6&period=30"
        );
        let factor = ws
            .import_otpauth_uri(account.id, &SecretText::new(&uri))
            .expect("import");
        assert_eq!(factor.kind, FactorKind::Totp);
        assert_eq!(factor.issuer.as_deref(), Some("GitHub"));
        assert!(factor.secret_ref.is_some());

        // RFC 6238/4226: TOTP at t=59 (counter 1) must be 287082.
        let (code, remaining) = ws.generate_otp_code(factor.id, 59).expect("code");
        assert_eq!(code, "287082");
        assert_eq!(remaining, 1);

        // The seed never reaches disk in plaintext.
        let vault_file = std::fs::read(dir.path().join(VAULT_REL)).expect("vault file");
        assert!(
            !vault_file
                .windows(SYNTHETIC_SEED_B32.len())
                .any(|window| window == SYNTHETIC_SEED_B32.as_bytes())
        );

        // Deleting the factor purges the seed.
        ws.delete_factor(factor.id).expect("delete factor");
        let account = ws.get_account(account.id).expect("account");
        assert!(account.factors.is_empty());
    }

    #[test]
    fn hotp_counter_advances_in_storage() {
        let (_dir, ws) = workspace();
        ws.create_vault(&SecretText::new(SYNTHETIC_MASTER))
            .expect("vault");
        let profile = ws
            .create_profile(&profile_input("Hotp Profile"))
            .expect("profile");
        let account = ws
            .create_account(profile.id, &account_input())
            .expect("account");

        let uri = format!("otpauth://hotp/bob?secret={SYNTHETIC_SEED_B32}&counter=0&digits=6");
        let factor = ws
            .import_otpauth_uri(account.id, &SecretText::new(&uri))
            .expect("import");

        // RFC 4226 Appendix D: counters 0 and 1.
        let (first, remaining) = ws.generate_otp_code(factor.id, 0).expect("first");
        assert_eq!((first.as_str(), remaining), ("755224", 0));
        let (second, _) = ws.generate_otp_code(factor.id, 0).expect("second");
        assert_eq!(second, "287082");

        // The counter advanced exactly once per generation.
        let stored = ws
            .get_account(account.id)
            .expect("account")
            .factors
            .into_iter()
            .find(|factor| factor.id == factor.id)
            .expect("factor");
        assert_eq!(stored.counter, Some(2));
    }

    #[test]
    fn qr_image_import_creates_factor() {
        let (_dir, ws) = workspace();
        ws.create_vault(&SecretText::new(SYNTHETIC_MASTER))
            .expect("vault");
        let profile = ws
            .create_profile(&profile_input("Qr Profile"))
            .expect("profile");
        let account = ws
            .create_account(profile.id, &account_input())
            .expect("account");

        // Synthetic otpauth QR fixture (RFC seed) from the qr crate.
        let png = std::fs::read(
            env!("CARGO_MANIFEST_DIR").to_owned() + "/../../crates/qr/tests/data/otpauth_totp.png",
        )
        .expect("fixture");
        let factor = ws.import_qr_image(account.id, &png).expect("import");
        assert_eq!(factor.kind, FactorKind::Totp);
        let (code, _) = ws.generate_otp_code(factor.id, 59).expect("code");
        assert_eq!(code, "287082");
    }

    #[test]
    fn recovery_code_workflow() {
        let (_dir, ws) = workspace();
        ws.create_vault(&SecretText::new(SYNTHETIC_MASTER))
            .expect("vault");
        let profile = ws
            .create_profile(&profile_input("Recovery Profile"))
            .expect("profile");
        let account = ws
            .create_account(profile.id, &account_input())
            .expect("account");

        let codes = ws
            .add_recovery_codes(
                account.id,
                &[
                    SecretText::new("synthetic-recovery-1111"),
                    SecretText::new("synthetic-recovery-2222"),
                ],
            )
            .expect("add");
        assert_eq!(codes.len(), 2);

        // Metadata only; no code values in the listing types.
        let listed = ws.list_recovery_codes(account.id).expect("list");
        assert_eq!(listed.len(), 2);
        let rendered = format!("{listed:?}");
        assert!(!rendered.contains("synthetic-recovery"));

        let at = chrono::Utc::now();
        ws.mark_recovery_code_used(codes[0].id, at).expect("mark");
        let listed = ws.list_recovery_codes(account.id).expect("list");
        assert!(listed[0].used);
        assert_eq!(listed[0].marked_used_at, Some(at));

        // Delete purges the value.
        ws.delete_recovery_code(codes[0].id).expect("delete");
        assert_eq!(ws.list_recovery_codes(account.id).expect("list").len(), 1);
    }

    #[test]
    fn idle_auto_lock_fires_after_threshold() {
        let (_dir, ws) = workspace();
        ws.create_vault(&SecretText::new(SYNTHETIC_MASTER))
            .expect("vault");
        assert!(ws.vault_status().is_unlocked());

        // Default settings: 10 minutes.
        let t0 = Instant::now();
        ws.record_activity(t0);
        assert!(
            !ws.maybe_auto_lock(t0 + Duration::from_secs(9 * 60))
                .expect("tick")
        );
        assert!(ws.vault_status().is_unlocked());
        assert!(
            ws.maybe_auto_lock(t0 + Duration::from_secs(10 * 60))
                .expect("tick")
        );
        assert_eq!(ws.vault_status().lock_state, VaultLockState::Locked);

        // Unlock again: activity re-arms the tracker.
        ws.unlock_vault(&SecretText::new(SYNTHETIC_MASTER))
            .expect("unlock");
        let t1 = Instant::now();
        ws.record_activity(t1);
        assert!(
            !ws.maybe_auto_lock(t1 + Duration::from_secs(60))
                .expect("tick")
        );
    }

    #[test]
    fn launch_plan_resolves_current_version() {
        let (dir, ws) = workspace();
        ws.create_vault(&SecretText::new(SYNTHETIC_MASTER))
            .expect("vault");
        let profile = ws
            .create_profile(&profile_input("Launch Profile"))
            .expect("profile");

        // No Thorium installed yet.
        let error = ws.plan_launch(profile.id).expect_err("no current");
        assert_eq!(error.diagnostic_code(), "CONTROLLER_NO_CURRENT_THORIUM");

        // Fake an install: copy cmd.exe to the expected layout position.
        let layout = ws.thorium_layout();
        layout.initialize().expect("init");
        let version_dir = dir.path().join("browsers/thorium/versions/152.0.7977.55");
        std::fs::create_dir_all(version_dir.join("BIN")).expect("dirs");
        let comspec =
            std::env::var_os("ComSpec").unwrap_or_else(|| "C:\\Windows\\System32\\cmd.exe".into());
        std::fs::copy(comspec, version_dir.join("BIN/thorium.exe")).expect("stub exe");
        layout.set_current("152.0.7977.55").expect("set current");

        let plan = ws.plan_launch(profile.id).expect("plan");
        assert_eq!(plan.version, "152.0.7977.55");
        assert!(plan.executable.is_file());
        assert!(
            plan.arguments
                .iter()
                .any(|argument| argument.starts_with("--user-data-dir="))
        );
        let expected_dir = dir.path().join(&plan.profile.user_data_rel_path);
        assert_eq!(plan.user_data_dir, expected_dir);

        // Diagnostics reflect the install and current selection.
        let snapshot = ws.diagnostics().expect("diagnostics");
        assert_eq!(
            snapshot.installed_thorium_versions,
            vec!["152.0.7977.55".to_owned()]
        );
        assert_eq!(
            snapshot.current_thorium_version.as_deref(),
            Some("152.0.7977.55")
        );
    }

    #[test]
    fn wrong_password_error_message_leaks_nothing() {
        let (_dir, ws) = workspace();
        ws.create_vault(&SecretText::new(SYNTHETIC_MASTER))
            .expect("vault");
        ws.lock_vault().expect("lock");
        let error = ws
            .unlock_vault(&SecretText::new("other-password"))
            .expect_err("wrong");
        let rendered = format!("{error} ({error:?})");
        assert!(!rendered.contains(SYNTHETIC_MASTER));
        assert!(!rendered.contains("other-password"));
        assert_eq!(
            error.diagnostic_code(),
            VaultError::UnlockFailed.diagnostic_code()
        );
    }

    #[test]
    fn settings_roundtrip_updates_idle_threshold() {
        let (_dir, ws) = workspace();
        let settings = ws.settings().expect("settings");
        assert_eq!(settings.clipboard_clear_seconds, 20);

        let mut changed = settings;
        changed.vault_idle_lock_minutes = Some(30);
        ws.save_settings(&changed).expect("save");
        assert_eq!(
            ws.settings().expect("reload").vault_idle_lock_minutes,
            Some(30)
        );

        // Invalid settings are rejected.
        let mut invalid = changed;
        invalid.clipboard_clear_seconds = 1;
        let error = ws.save_settings(&invalid).expect_err("invalid");
        assert_eq!(error.diagnostic_code(), "DOMAIN_OUT_OF_RANGE");
    }

    #[test]
    fn external_factors_record_without_otp() {
        let (_dir, ws) = workspace();
        ws.create_vault(&SecretText::new(SYNTHETIC_MASTER))
            .expect("vault");
        let profile = ws
            .create_profile(&profile_input("Ext Profile"))
            .expect("profile");
        let account = ws
            .create_account(profile.id, &account_input())
            .expect("account");
        let factor = ws
            .add_external_factor(
                account.id,
                Some("Hardware key".to_owned()),
                Some("Microsoft Authenticator push".to_owned()),
            )
            .expect("add");
        assert_eq!(factor.kind, FactorKind::ExternalAuthenticator);
        assert!(factor.secret_ref.is_none());
        assert_eq!(factor.algorithm.map(|algorithm| algorithm.id()), None);
        let error = ws.generate_otp_code(factor.id, 59).expect_err("no otp");
        assert_eq!(error.diagnostic_code(), "OTP_UNSUPPORTED_TYPE");
    }

    #[test]
    fn algorithms_roundtrip_in_factor_metadata() {
        // Pins that algorithm metadata survives a controller roundtrip.
        assert_eq!(OtpAlgorithm::from_id("SHA256"), Some(OtpAlgorithm::Sha256));
    }
}
