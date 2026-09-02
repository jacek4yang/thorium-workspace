//! Application services implemented on [`Workspace`].
//!
//! Each service composes the existing crate APIs; no subsystem logic is
//! duplicated. Secrets cross these APIs only as
//! [`thorium_workspace_secrets::SecretText`]/[`thorium_workspace_secrets::SecretBytes`]
//! and never appear in errors or logs.

use std::time::Instant;

use thorium_workspace_domain::{
    Account, AccountId, AccountInput, BrowserProfile, FactorId, FactorKind, LocaleTag, ProfileId,
    ProfileInput, RecoveryCode, RecoveryCodeId, SecondFactor, SecretRef, ThoriumSelection,
    WorkspaceSettings,
};
use thorium_workspace_secrets::{SecretBytes, SecretText};
use thorium_workspace_vault::{VaultEntry, VaultEntryKind, VaultLockState, VaultPayload};

use crate::error::ControllerError;
use crate::workspace::Workspace;

/// Current vault state surfaced to the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    /// Whether a vault file exists.
    pub exists: bool,
    /// Lock state.
    pub lock_state: VaultLockState,
}

impl VaultStatus {
    /// Whether content operations are possible.
    pub fn is_unlocked(&self) -> bool {
        self.lock_state == VaultLockState::Unlocked
    }
}

/// Everything needed to launch a profile (used by the launcher and
/// diagnostics).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    /// The profile to launch.
    pub profile: BrowserProfile,
    /// Resolved browser executable.
    pub executable: std::path::PathBuf,
    /// Built launch arguments.
    pub arguments: Vec<String>,
    /// Absolute user data directory.
    pub user_data_dir: std::path::PathBuf,
    /// Resolved Thorium version.
    pub version: String,
}

impl Workspace {
    // ------------------------------------------------------------------
    // Vault lifecycle

    /// Current vault existence and lock state.
    pub fn vault_status(&self) -> VaultStatus {
        let vault = self.vault();
        VaultStatus {
            exists: vault.exists(),
            lock_state: vault.lock_state(),
        }
    }

    /// Creates the vault with a master password and leaves it unlocked.
    pub fn create_vault(&self, master_password: &SecretText) -> Result<(), ControllerError> {
        self.vault().create(master_password)?;
        Ok(())
    }

    /// Unlocks the vault with the master password.
    pub fn unlock_vault(&self, master_password: &SecretText) -> Result<(), ControllerError> {
        self.vault().unlock(master_password)?;
        Ok(())
    }

    /// Locks the vault (drops the derived key).
    pub fn lock_vault(&self) -> Result<(), ControllerError> {
        self.vault().lock();
        Ok(())
    }

    /// Rotates the master password. Content is preserved.
    pub fn change_master_password(
        &self,
        current: &SecretText,
        new_password: &SecretText,
    ) -> Result<(), ControllerError> {
        self.vault().change_master_password(current, new_password)?;
        Ok(())
    }

    /// Loads and decrypts the whole payload. Deliberately `pub(crate)`:
    /// application services never hand the full decrypted vault to
    /// callers; only targeted entries cross this boundary.
    fn load_payload(&self) -> Result<VaultPayload, ControllerError> {
        Ok(self.vault().load()?)
    }

    /// Applies a mutation to the vault payload and persists it atomically.
    fn mutate_payload<T>(
        &self,
        mutate: impl FnOnce(&mut VaultPayload) -> T,
    ) -> Result<T, ControllerError> {
        let vault = self.vault();
        let mut payload = vault.load()?;
        let outcome = mutate(&mut payload);
        vault.save(&payload)?;
        Ok(outcome)
    }

    // ------------------------------------------------------------------
    // Activity + idle auto-lock

    /// Records user activity (resets the idle clock).
    pub fn record_activity(&self, now: Instant) {
        self.idle().record_activity(now);
    }

    /// Locks the vault when the idle threshold has elapsed. Returns
    /// whether a lock was performed.
    pub fn maybe_auto_lock(&self, now: Instant) -> Result<bool, ControllerError> {
        let should_lock = {
            let idle = self.idle();
            let unlocked = self.vault().lock_state() == VaultLockState::Unlocked;
            unlocked && idle.is_idle(now)
        };
        if !should_lock {
            return Ok(false);
        }
        self.lock_vault()?;
        self.idle().disarm();
        Ok(true)
    }

    // ------------------------------------------------------------------
    // Settings

    /// Loads workspace settings (defaults before first save).
    pub fn settings(&self) -> Result<WorkspaceSettings, ControllerError> {
        Ok(self
            .store()
            .load_settings()?
            .unwrap_or_else(WorkspaceSettings::default))
    }

    /// Saves workspace settings and applies the idle threshold.
    pub fn save_settings(&self, settings: &WorkspaceSettings) -> Result<(), ControllerError> {
        settings.validate()?;
        self.store().save_settings(settings)?;
        let mut idle = self.idle();
        idle.set_threshold(
            settings
                .vault_idle_lock_minutes
                .map(|minutes| std::time::Duration::from_secs(60 * u64::from(minutes))),
        );
        Ok(())
    }

    // ------------------------------------------------------------------
    // Profiles

    /// Lists profiles with derived account associations.
    pub fn list_profiles(&self) -> Result<Vec<BrowserProfile>, ControllerError> {
        let mut profiles = self.store().list_profiles()?;
        for profile in &mut profiles {
            profile.account_ids = self
                .store()
                .list_accounts(profile.id)?
                .into_iter()
                .map(|account| account.id)
                .collect();
        }
        Ok(profiles)
    }

    /// Loads one profile (with derived account associations).
    pub fn get_profile(&self, id: ProfileId) -> Result<BrowserProfile, ControllerError> {
        let mut profile = self
            .store()
            .get_profile(id)?
            .ok_or(ControllerError::NotFound { entity: "profile" })?;
        profile.account_ids = self
            .store()
            .list_accounts(profile.id)?
            .into_iter()
            .map(|account| account.id)
            .collect();
        Ok(profile)
    }

    /// Creates a profile from validated input.
    pub fn create_profile(&self, input: &ProfileInput) -> Result<BrowserProfile, ControllerError> {
        let profile = BrowserProfile::from_validated(input.validate()?);
        self.store().create_profile(&profile)?;
        // The isolated User Data directory belongs to the profile's
        // identity; create it eagerly so isolation is guaranteed before
        // any launch.
        let user_data_dir = self.root().join(&profile.user_data_rel_path);
        thorium_workspace_browser_profile::prepare_user_data_dir(&user_data_dir)?;
        Ok(profile)
    }

    /// Updates a profile (name, version selection, startup URLs, locale,
    /// timezone). Timestamps are bumped here.
    pub fn update_profile(
        &self,
        profile: &BrowserProfile,
    ) -> Result<BrowserProfile, ControllerError> {
        let mut updated = profile.clone();
        // Re-validate user-editable metadata.
        let input = ProfileInput {
            name: updated.name.clone(),
            thorium_version: updated.thorium_version.clone(),
            startup_urls: updated.startup_urls.clone(),
            locale: updated.locale.clone(),
            timezone: updated.timezone.clone(),
        };
        let validated = input.validate()?;
        updated.name = validated.name;
        updated.thorium_version = validated.thorium_version;
        updated.startup_urls = validated.startup_urls;
        updated.locale = validated.locale;
        updated.timezone = validated.timezone;
        updated.updated_at = chrono::Utc::now();
        if !self.store().update_profile(&updated)? {
            return Err(ControllerError::NotFound { entity: "profile" });
        }
        Ok(updated)
    }

    /// Deletes a profile and purges every secret behind its accounts
    /// (passwords, factor seeds, recovery codes). Refuses while the vault
    /// is locked so purges are never skipped silently.
    pub fn delete_profile(&self, id: ProfileId) -> Result<(), ControllerError> {
        let accounts = self.store().list_accounts(id)?;
        let mut refs = Vec::new();
        for account in accounts {
            if let Some(reference) = &account.password_ref {
                refs.push(reference.clone());
            }
            for factor in &account.factors {
                if let Some(reference) = &factor.secret_ref {
                    refs.push(reference.clone());
                }
            }
            for code in &account.recovery_codes {
                refs.push(code.secret_ref.clone());
            }
        }
        if !refs.is_empty() {
            self.mutate_payload(|payload| {
                for reference in &refs {
                    payload.remove(reference);
                }
            })?;
        }
        if !self.store().delete_profile(id)? {
            return Err(ControllerError::NotFound { entity: "profile" });
        }
        Ok(())
    }

    /// Resolves the launch plan for a profile without spawning anything.
    pub fn plan_launch(&self, id: ProfileId) -> Result<LaunchPlan, ControllerError> {
        let profile = self.get_profile(id)?;
        let layout = self.thorium_layout();
        let version = match &profile.thorium_version {
            ThoriumSelection::Current => layout
                .current_version()?
                .ok_or(ControllerError::NoCurrentThorium)?,
            ThoriumSelection::Pinned { version } => version.clone(),
        };
        if !layout.is_installed(&version) {
            return Err(thorium_workspace_thorium::ThoriumError::NotInstalled { version }.into());
        }
        let user_data_dir = self.root().join(&profile.user_data_rel_path);
        let spec = thorium_workspace_browser_profile::LaunchSpec {
            user_data_dir: user_data_dir.clone(),
            startup_urls: profile.startup_urls.clone(),
            locale: profile.locale.as_deref().map(LocaleTag::new).transpose()?,
            extra_arguments: Vec::new(),
        };
        let arguments = spec.build_arguments()?;
        Ok(LaunchPlan {
            profile,
            executable: layout.executable_path(&version),
            arguments,
            user_data_dir,
            version,
        })
    }

    /// Launches a profile as a supervised session.
    pub fn launch_profile(&self, id: ProfileId) -> Result<LaunchPlan, ControllerError> {
        self.reap_dead_sessions();
        let plan = self.plan_launch(id)?;
        let session = thorium_workspace_browser_profile::Session::launch(
            id,
            &plan.executable,
            &plan.arguments,
            &plan.user_data_dir,
        )?;
        self.sessions().insert(id, session);
        Ok(plan)
    }

    /// Stops a running profile session (tree shutdown + reap).
    pub fn stop_profile(&self, id: ProfileId) -> Result<(), ControllerError> {
        let session = self
            .sessions()
            .remove(&id)
            .ok_or(ControllerError::NotFound { entity: "session" })?;
        session.shutdown()?;
        Ok(())
    }

    /// Profile ids with live sessions (finished sessions are reaped).
    pub fn running_profiles(&self) -> Result<Vec<ProfileId>, ControllerError> {
        self.reap_dead_sessions();
        Ok(self.sessions().keys().copied().collect())
    }

    fn reap_dead_sessions(&self) {
        self.sessions().retain(|_, session| session.is_running());
    }

    // ------------------------------------------------------------------
    // Accounts

    /// Lists the accounts of a profile.
    pub fn list_accounts(&self, profile_id: ProfileId) -> Result<Vec<Account>, ControllerError> {
        Ok(self.store().list_accounts(profile_id)?)
    }

    /// Loads one account.
    pub fn get_account(&self, id: AccountId) -> Result<Account, ControllerError> {
        self.store()
            .get_account(id)?
            .ok_or(ControllerError::NotFound { entity: "account" })
    }

    /// Creates an account from validated input.
    pub fn create_account(
        &self,
        profile_id: ProfileId,
        input: &AccountInput,
    ) -> Result<Account, ControllerError> {
        let validated = input.validate()?;
        let now = chrono::Utc::now();
        let account = Account {
            id: AccountId::new(),
            profile_id,
            display_name: validated.display_name,
            service_kind: validated.service_kind,
            username: validated.username,
            email: validated.email,
            login_url: validated.login_url,
            tags: validated.tags,
            notes: validated.notes,
            password_ref: None,
            factors: Vec::new(),
            recovery_codes: Vec::new(),
            created_at: now,
            updated_at: now,
        };
        self.store().create_account(&account)?;
        Ok(account)
    }

    /// Updates account metadata.
    pub fn update_account(&self, account: &Account) -> Result<(), ControllerError> {
        let input = AccountInput {
            display_name: account.display_name.clone(),
            service_kind: account.service_kind.clone(),
            username: account.username.clone(),
            email: account.email.clone(),
            login_url: account.login_url.clone(),
            tags: account.tags.clone(),
            notes: account.notes.clone(),
        };
        let validated = input.validate()?;
        let mut updated = account.clone();
        updated.display_name = validated.display_name;
        updated.service_kind = validated.service_kind;
        updated.username = validated.username;
        updated.email = validated.email;
        updated.login_url = validated.login_url;
        updated.tags = validated.tags;
        updated.notes = validated.notes;
        updated.updated_at = chrono::Utc::now();
        if !self.store().update_account(&updated)? {
            return Err(ControllerError::NotFound { entity: "account" });
        }
        Ok(())
    }

    /// Deletes an account and purges its secrets from the vault.
    pub fn delete_account(&self, id: AccountId) -> Result<(), ControllerError> {
        let account = self.get_account(id)?;
        let mut refs = Vec::new();
        if let Some(reference) = &account.password_ref {
            refs.push(reference.clone());
        }
        for factor in &account.factors {
            if let Some(reference) = &factor.secret_ref {
                refs.push(reference.clone());
            }
        }
        for code in &account.recovery_codes {
            refs.push(code.secret_ref.clone());
        }
        if !refs.is_empty() {
            self.mutate_payload(|payload| {
                for reference in &refs {
                    payload.remove(reference);
                }
            })?;
        }
        if !self.store().delete_account(id)? {
            return Err(ControllerError::NotFound { entity: "account" });
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // Account passwords

    fn password_ref(account: &Account) -> Result<SecretRef, ControllerError> {
        account
            .password_ref
            .clone()
            .ok_or(ControllerError::NotFound { entity: "password" })
    }

    /// Stores (or replaces) an account password in the vault.
    pub fn set_password(
        &self,
        account_id: AccountId,
        password: &SecretText,
    ) -> Result<(), ControllerError> {
        let account = self.get_account(account_id)?;
        let reference = SecretRef::for_password(&account_id);
        let now = chrono::Utc::now();
        self.mutate_payload(|payload| {
            payload.put(VaultEntry {
                secret_ref: reference.clone(),
                kind: VaultEntryKind::Password,
                value: SecretBytes::new(password.expose().as_bytes()),
                created_at: now,
                updated_at: now,
            });
        })?;
        let mut updated = account.clone();
        updated.password_ref = Some(reference);
        updated.updated_at = now;
        if !self.store().update_account(&updated)? {
            return Err(ControllerError::NotFound { entity: "account" });
        }
        Ok(())
    }

    /// Retrieves an account password. Explicit-by-design: the UI calls
    /// this only for reveal/copy actions.
    pub fn get_password(&self, account_id: AccountId) -> Result<SecretText, ControllerError> {
        let account = self.get_account(account_id)?;
        let reference = Self::password_ref(&account)?;
        let payload = self.load_payload()?;
        let entry = payload
            .get(&reference)
            .ok_or(ControllerError::NotFound { entity: "password" })?;
        let text = String::from_utf8(entry.value.expose().to_vec()).map_err(|_| {
            thorium_workspace_vault::VaultError::Payload(
                "stored password is not valid UTF-8".to_owned(),
            )
        })?;
        Ok(SecretText::new(&text))
    }

    /// Copies an account password to the clipboard and schedules the
    /// conditional clear. Returns the delay used.
    pub fn copy_password(
        &self,
        account_id: AccountId,
        now: Instant,
    ) -> Result<std::time::Duration, ControllerError> {
        let settings = self.settings()?;
        let delay = std::time::Duration::from_secs(u64::from(settings.clipboard_clear_seconds));
        let password = self.get_password(account_id)?;
        self.clipboard_state()
            .copy_scheduled(&*self.clipboard(), password, delay, now)?;
        Ok(delay)
    }

    /// Deletes an account password.
    pub fn delete_password(&self, account_id: AccountId) -> Result<(), ControllerError> {
        let account = self.get_account(account_id)?;
        let reference = Self::password_ref(&account)?;
        self.mutate_payload(|payload| {
            payload.remove(&reference);
        })?;
        let mut updated = account.clone();
        updated.password_ref = None;
        updated.updated_at = chrono::Utc::now();
        if !self.store().update_account(&updated)? {
            return Err(ControllerError::NotFound { entity: "account" });
        }
        Ok(())
    }

    // ------------------------------------------------------------------
    // Factors / OTP / QR

    /// Imports an `otpauth://` URI (e.g. from a QR payload): stores the
    /// seed in the vault and creates the factor metadata row.
    pub fn import_otpauth_uri(
        &self,
        account_id: AccountId,
        uri: &SecretText,
    ) -> Result<SecondFactor, ControllerError> {
        let parsed = thorium_workspace_otp::parse_otpauth_uri(uri.expose())?;
        let factor = SecondFactor {
            id: FactorId::new(),
            account_id,
            kind: parsed.kind,
            label: None,
            issuer: parsed.issuer.clone(),
            account_label: parsed.account_label.clone(),
            algorithm: Some(parsed.algorithm),
            digits: Some(parsed.digits),
            period_seconds: Some(parsed.period_seconds),
            counter: parsed.counter,
            secret_ref: None,
            external_note: None,
            created_at: chrono::Utc::now(),
        };
        let reference = SecretRef::for_otp_seed(&factor.id);
        let factor_with_ref = SecondFactor {
            secret_ref: Some(reference.clone()),
            ..factor.clone()
        };
        let now = chrono::Utc::now();
        self.mutate_payload(|payload| {
            payload.put(VaultEntry {
                secret_ref: reference.clone(),
                kind: VaultEntryKind::OtpSeed,
                value: SecretBytes::new(parsed.secret.expose()),
                created_at: now,
                updated_at: now,
            });
        })?;
        self.store().add_factor(&factor_with_ref)?;
        Ok(factor_with_ref)
    }

    /// Records an external authenticator (no workspace-side OTP).
    pub fn add_external_factor(
        &self,
        account_id: AccountId,
        label: Option<String>,
        note: Option<String>,
    ) -> Result<SecondFactor, ControllerError> {
        let factor = SecondFactor {
            id: FactorId::new(),
            account_id,
            kind: FactorKind::ExternalAuthenticator,
            label,
            issuer: None,
            account_label: None,
            algorithm: None,
            digits: None,
            period_seconds: None,
            counter: None,
            secret_ref: None,
            external_note: note,
            created_at: chrono::Utc::now(),
        };
        self.store().add_factor(&factor)?;
        Ok(factor)
    }

    /// Generates the current OTP code for a factor. Returns
    /// `(code, seconds_remaining_in_window)` for TOTP, or the current
    /// counter code for HOTP (counter is advanced).
    pub fn generate_otp_code(
        &self,
        factor_id: FactorId,
        unix_time: u64,
    ) -> Result<(String, u32), ControllerError> {
        let factor = self
            .store()
            .get_factor(factor_id)?
            .ok_or(ControllerError::NotFound { entity: "factor" })?;
        let Some(reference) = factor.secret_ref.clone() else {
            return Err(thorium_workspace_otp::OtpError::UnsupportedType.into());
        };
        let payload = self.load_payload()?;
        let entry = payload
            .get(&reference)
            .ok_or(ControllerError::NotFound { entity: "otp seed" })?;
        let seed: SecretBytes = SecretBytes::new(entry.value.expose());
        match factor.kind {
            FactorKind::Totp => {
                let code = thorium_workspace_otp::totp(
                    seed.expose(),
                    unix_time,
                    factor.period_seconds.unwrap_or(30),
                    factor
                        .algorithm
                        .ok_or(thorium_workspace_otp::OtpError::InvalidSecret)?,
                    factor.digits.unwrap_or(6),
                )?;
                let remaining = thorium_workspace_otp::seconds_remaining(
                    unix_time,
                    factor.period_seconds.unwrap_or(30),
                );
                Ok((code, remaining))
            }
            FactorKind::Hotp => {
                let counter = factor.counter.unwrap_or(0);
                let code = thorium_workspace_otp::hotp(
                    seed.expose(),
                    counter,
                    factor
                        .algorithm
                        .ok_or(thorium_workspace_otp::OtpError::InvalidSecret)?,
                    factor.digits.unwrap_or(6),
                )?;
                self.store().set_hotp_counter(factor_id, counter + 1)?;
                Ok((code, 0))
            }
            FactorKind::ExternalAuthenticator => {
                Err(thorium_workspace_otp::OtpError::UnsupportedType.into())
            }
        }
    }

    /// Decodes a QR image payload and imports it for the account. The
    /// payload text never appears in errors.
    pub fn import_qr_image(
        &self,
        account_id: AccountId,
        image_bytes: &[u8],
    ) -> Result<SecondFactor, ControllerError> {
        let payload = thorium_workspace_qr::decode_single(image_bytes)?;
        let uri = SecretText::new(&payload);
        self.import_otpauth_uri(account_id, &uri)
    }

    /// Deletes a factor and purges its seed.
    pub fn delete_factor(&self, factor_id: FactorId) -> Result<(), ControllerError> {
        let factor = self
            .store()
            .get_factor(factor_id)?
            .ok_or(ControllerError::NotFound { entity: "factor" })?;
        if let Some(reference) = &factor.secret_ref {
            self.mutate_payload(|payload| {
                payload.remove(reference);
            })?;
        }
        self.store().delete_factor(factor_id)?;
        Ok(())
    }

    // ------------------------------------------------------------------
    // Recovery codes

    /// Adds recovery code values (encrypted at rest) to an account.
    pub fn add_recovery_codes(
        &self,
        account_id: AccountId,
        values: &[SecretText],
    ) -> Result<Vec<RecoveryCode>, ControllerError> {
        let account = self.get_account(account_id)?;
        let next_position = account
            .recovery_codes
            .iter()
            .map(|code| code.position)
            .max()
            .map_or(0, |max| max + 1);
        let now = chrono::Utc::now();
        let mut created = Vec::with_capacity(values.len());
        for (offset, value) in values.iter().enumerate() {
            let id = RecoveryCodeId::new();
            let reference = SecretRef::for_recovery_code(&id);
            let code = RecoveryCode {
                id,
                account_id,
                position: next_position + offset as u32,
                used: false,
                marked_used_at: None,
                secret_ref: reference.clone(),
            };
            self.mutate_payload(|payload| {
                payload.put(VaultEntry {
                    secret_ref: reference.clone(),
                    kind: VaultEntryKind::RecoveryCode,
                    value: SecretBytes::new(value.expose().as_bytes()),
                    created_at: now,
                    updated_at: now,
                });
            })?;
            self.store().add_recovery_code(&code)?;
            created.push(code);
        }
        Ok(created)
    }

    /// Lists recovery code slots (metadata only; values stay in the
    /// vault).
    pub fn list_recovery_codes(
        &self,
        account_id: AccountId,
    ) -> Result<Vec<RecoveryCode>, ControllerError> {
        Ok(self.store().list_recovery_codes(account_id)?)
    }

    /// Marks a recovery code used (idempotent, first timestamp wins).
    pub fn mark_recovery_code_used(
        &self,
        id: RecoveryCodeId,
        at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), ControllerError> {
        if !self.store().mark_recovery_code_used(id, at)? {
            return Err(ControllerError::NotFound {
                entity: "recovery code",
            });
        }
        Ok(())
    }

    /// Deletes a recovery code slot and purges its value.
    pub fn delete_recovery_code(&self, id: RecoveryCodeId) -> Result<(), ControllerError> {
        let account = self.get_account(self.store_account_of_recovery_code(id)?)?;
        let Some(code) = account.recovery_codes.iter().find(|code| code.id == id) else {
            return Err(ControllerError::NotFound {
                entity: "recovery code",
            });
        };
        let reference = code.secret_ref.clone();
        self.mutate_payload(|payload| {
            payload.remove(&reference);
        })?;
        if !self.store().delete_recovery_code(id)? {
            return Err(ControllerError::NotFound {
                entity: "recovery code",
            });
        }
        Ok(())
    }

    fn store_account_of_recovery_code(
        &self,
        id: RecoveryCodeId,
    ) -> Result<AccountId, ControllerError> {
        self.store()
            .recovery_code_owner(id)?
            .ok_or(ControllerError::NotFound {
                entity: "recovery code",
            })
    }
}
