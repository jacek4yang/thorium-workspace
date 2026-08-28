//! The workspace: one object owning every subsystem.
//!
//! The Tauri command layer holds exactly one of these behind a mutex and calls
//! methods on it. Nothing above this type reaches into storage, the vault or the
//! browser directly.

use std::path::{Path, PathBuf};

use tw_browser_profile::BrowserSession;
use tw_domain::{
    Account, AccountDraft, AccountId, BrowserProfile, BrowserProfileDraft, DiagnosticCode, FactorId,
    OtpParameters, ProfileId, RecoveryCode, RecoveryCodeId, SecondFactor, SecondFactorDraft, ThoriumChannel,
    WorkspaceSettings,
};
use tw_otp::{OtpCode, OtpCredential, OtpSecret};
use tw_secrets::SecretString;
use tw_storage::{AccountRepo, Database, ProfileRepo, RecoveryCodeRepo, SecondFactorRepo, SettingsRepo};
use tw_thorium::{InstallProgress, InstallRequest, ReleaseClientConfig, ThoriumManager};
use tw_vault::{LockReason, VaultState};
use tw_windows_platform::SingleInstanceGuard;

use crate::bootstrap::{Bootstrap, BootstrapReport, WorkspacePaths};
use crate::clipboard::{ClipboardGuard, CopyKind};
use crate::diagnostics::{DiagnosticReport, DiagnosticsBuilder, ProfileDiagnostic};
use crate::error::{AppError, AppResult};
use crate::profiles::SessionRegistry;
use crate::vault::VaultService;

/// Everything the application owns.
pub struct Workspace {
    paths: WorkspacePaths,
    bootstrap: BootstrapReport,
    database: Database,
    vault: VaultService,
    thorium: ThoriumManager,
    sessions: SessionRegistry,
    clipboard: ClipboardGuard,
    settings: WorkspaceSettings,
    /// Held for the life of the process: releasing it would let a second
    /// instance open the same workspace.
    _instance: SingleInstanceGuard,
}

impl std::fmt::Debug for Workspace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Workspace")
            .field("root", &self.paths.root())
            .field("vault_unlocked", &self.vault.is_unlocked())
            .field("running_profiles", &self.sessions.ids().len())
            .finish_non_exhaustive()
    }
}

impl Workspace {
    /// Opens the workspace beside the running executable.
    ///
    /// # Errors
    ///
    /// See [`Bootstrap::run`].
    pub fn open() -> AppResult<Self> {
        Self::from_bootstrap(Bootstrap::run()?)
    }

    /// Opens the workspace at an explicit root. Used by tests.
    ///
    /// # Errors
    ///
    /// See [`Bootstrap::run_in`].
    pub fn open_in(root: &Path) -> AppResult<Self> {
        Self::from_bootstrap(Bootstrap::run_in(root)?)
    }

    fn from_bootstrap(mut bootstrap: Bootstrap) -> AppResult<Self> {
        let thorium = ThoriumManager::new(bootstrap.paths().root(), ReleaseClientConfig::default())?;
        let staging_removed = thorium.clean_staging().unwrap_or(0);
        bootstrap.set_stale_staging_removed(staging_removed);

        let (paths, instance, mut database, report) = bootstrap.into_parts();
        let settings = SettingsRepo::load(database.connection())?;
        let vault = VaultService::new(paths.vault_file(), settings.vault);

        // Reconcile what the last run left behind against what is actually true
        // now, before the UI reads any of it.
        crate::profiles::recover_runtime_state(&database, paths.root())?;
        crate::thorium::reconcile(&mut database, &thorium)?;

        Ok(Self {
            paths,
            bootstrap: report,
            database,
            vault,
            thorium,
            sessions: SessionRegistry::new(),
            clipboard: ClipboardGuard::system(),
            settings,
            _instance: instance,
        })
    }

    /// The workspace layout.
    #[must_use]
    pub const fn paths(&self) -> &WorkspacePaths {
        &self.paths
    }

    /// What bootstrap did.
    #[must_use]
    pub const fn bootstrap_report(&self) -> &BootstrapReport {
        &self.bootstrap
    }

    /// The current settings.
    #[must_use]
    pub const fn settings(&self) -> &WorkspaceSettings {
        &self.settings
    }

    /// Replaces the settings and persists them.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::InvalidInput`] when they fail validation.
    pub fn set_settings(&mut self, settings: WorkspaceSettings) -> AppResult<()> {
        settings.validate()?;
        SettingsRepo::save(self.database.connection(), &settings)?;
        self.vault.set_settings(settings.vault);
        self.settings = settings;
        Ok(())
    }

    // ---- Vault ------------------------------------------------------------

    /// The vault's state.
    #[must_use]
    pub fn vault_state(&self) -> VaultState {
        self.vault.state()
    }

    /// Creates the vault.
    ///
    /// # Errors
    ///
    /// See [`VaultService::create`].
    pub fn create_vault(&mut self, password: &SecretString) -> AppResult<VaultState> {
        self.vault.create(password)
    }

    /// Unlocks the vault.
    ///
    /// # Errors
    ///
    /// See [`VaultService::unlock`].
    pub fn unlock_vault(&mut self, password: &SecretString) -> AppResult<VaultState> {
        self.vault.unlock(password)
    }

    /// Locks the vault and stops tracking anything on the clipboard.
    pub fn lock_vault(&mut self, reason: LockReason) -> VaultState {
        self.clipboard.forget();
        self.vault.lock(reason)
    }

    /// Locks the vault if it has been idle long enough.
    pub fn lock_vault_if_idle(&mut self) -> bool {
        let locked = self.vault.lock_if_idle();
        if locked {
            self.clipboard.forget();
        }
        locked
    }

    /// Locks the vault on minimize, if configured.
    pub fn lock_vault_on_minimize(&mut self) -> bool {
        let locked = self.vault.lock_on_minimize();
        if locked {
            self.clipboard.forget();
        }
        locked
    }

    /// Records user activity so the idle timer restarts.
    pub fn touch_vault(&mut self) {
        self.vault.touch();
    }

    /// Changes the master password.
    ///
    /// # Errors
    ///
    /// See [`VaultService::change_password`].
    pub fn change_master_password(&mut self, current: &SecretString, next: &SecretString) -> AppResult<()> {
        self.vault.change_password(current, next)
    }

    /// Removes vault secrets nothing references any more.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::VaultLocked`] or a storage error.
    pub fn collect_orphaned_secrets(&mut self) -> AppResult<usize> {
        let live = crate::accounts::live_secret_refs(self.database.connection())?;
        self.vault.collect_orphans(&live)
    }

    // ---- Accounts ---------------------------------------------------------

    /// Lists accounts.
    ///
    /// # Errors
    ///
    /// Returns a storage error.
    pub fn list_accounts(&self) -> AppResult<Vec<Account>> {
        Ok(AccountRepo::list(self.database.connection())?)
    }

    /// Lists the accounts attached to a profile.
    ///
    /// # Errors
    ///
    /// Returns a storage error.
    pub fn list_accounts_for_profile(&self, id: ProfileId) -> AppResult<Vec<Account>> {
        Ok(AccountRepo::list_for_profile(self.database.connection(), id)?)
    }

    /// Creates an account.
    ///
    /// # Errors
    ///
    /// See [`crate::accounts::create_account`].
    pub fn create_account(
        &mut self,
        draft: &AccountDraft,
        password: Option<SecretString>,
    ) -> AppResult<Account> {
        crate::accounts::create_account(self.database.connection(), &mut self.vault, draft, password)
    }

    /// Updates an account's metadata.
    ///
    /// # Errors
    ///
    /// See [`crate::accounts::update_account`].
    pub fn update_account(&mut self, id: AccountId, draft: &AccountDraft) -> AppResult<Account> {
        crate::accounts::update_account(self.database.connection(), id, draft)
    }

    /// Sets or clears an account's password.
    ///
    /// # Errors
    ///
    /// See [`crate::accounts::set_account_password`].
    pub fn set_account_password(
        &mut self,
        id: AccountId,
        password: Option<SecretString>,
    ) -> AppResult<Account> {
        crate::accounts::set_account_password(self.database.connection(), &mut self.vault, id, password)
    }

    /// Deletes an account and its secrets.
    ///
    /// # Errors
    ///
    /// See [`crate::accounts::delete_account`].
    pub fn delete_account(&mut self, id: AccountId) -> AppResult<()> {
        crate::accounts::delete_account(self.database.connection(), &mut self.vault, id)
    }

    /// Copies an account's password to the clipboard.
    ///
    /// The plaintext never crosses the Tauri boundary: it goes from the vault
    /// straight to the clipboard inside this process.
    ///
    /// # Errors
    ///
    /// See [`crate::accounts::reveal_account_password`].
    pub fn copy_account_password(&mut self, id: AccountId) -> AppResult<()> {
        let password =
            crate::accounts::reveal_account_password(self.database.connection(), &mut self.vault, id)?;
        self.copy_secret(&password, CopyKind::Password)
    }

    /// Reveals an account's password for on-screen display.
    ///
    /// This is the one path that deliberately hands plaintext to the frontend,
    /// and it exists because a user sometimes has to read a password out.
    ///
    /// # Errors
    ///
    /// See [`crate::accounts::reveal_account_password`].
    pub fn reveal_account_password(&mut self, id: AccountId) -> AppResult<SecretString> {
        crate::accounts::reveal_account_password(self.database.connection(), &mut self.vault, id)
    }

    // ---- Second factors ---------------------------------------------------

    /// Lists an account's factors.
    ///
    /// # Errors
    ///
    /// Returns a storage error.
    pub fn list_factors(&self, account_id: AccountId) -> AppResult<Vec<SecondFactor>> {
        Ok(SecondFactorRepo::list_for_account(
            self.database.connection(),
            account_id,
        )?)
    }

    /// Adds an OTP factor from a parsed credential.
    ///
    /// # Errors
    ///
    /// See [`crate::accounts::add_otp_factor`].
    pub fn add_otp_factor(
        &mut self,
        account_id: AccountId,
        label: &str,
        credential: &OtpCredential,
    ) -> AppResult<SecondFactor> {
        crate::accounts::add_otp_factor(
            self.database.connection(),
            &mut self.vault,
            account_id,
            label,
            &credential.parameters,
            &credential.secret,
        )
    }

    /// Adds an OTP factor from a manually entered Base32 secret.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::OtpSecretInvalid`] for an undecodable secret.
    pub fn add_otp_factor_manual(
        &mut self,
        account_id: AccountId,
        label: &str,
        parameters: &OtpParameters,
        base32_secret: &SecretString,
    ) -> AppResult<SecondFactor> {
        let secret = OtpSecret::from_base32(base32_secret.expose())?;
        crate::accounts::add_otp_factor(
            self.database.connection(),
            &mut self.vault,
            account_id,
            label,
            parameters,
            &secret,
        )
    }

    /// Records a factor handled by another application or device.
    ///
    /// # Errors
    ///
    /// See [`crate::accounts::add_external_factor`].
    pub fn add_external_factor(
        &mut self,
        account_id: AccountId,
        draft: &SecondFactorDraft,
    ) -> AppResult<SecondFactor> {
        crate::accounts::add_external_factor(self.database.connection(), account_id, draft)
    }

    /// Generates the current code for a factor.
    ///
    /// # Errors
    ///
    /// See [`crate::accounts::generate_code`].
    pub fn generate_code(&mut self, factor_id: FactorId) -> AppResult<OtpCode> {
        crate::accounts::generate_code(self.database.connection(), &mut self.vault, factor_id)
    }

    /// Generates a code and copies it to the clipboard.
    ///
    /// # Errors
    ///
    /// See [`crate::accounts::generate_code`].
    pub fn copy_code(&mut self, factor_id: FactorId) -> AppResult<OtpCode> {
        let code = self.generate_code(factor_id)?;
        self.copy_secret(&SecretString::new(code.code.clone()), CopyKind::OtpCode)?;
        Ok(code)
    }

    /// Deletes a factor.
    ///
    /// # Errors
    ///
    /// See [`crate::accounts::delete_factor`].
    pub fn delete_factor(&mut self, factor_id: FactorId) -> AppResult<()> {
        crate::accounts::delete_factor(self.database.connection(), &mut self.vault, factor_id)
    }

    // ---- Recovery codes ---------------------------------------------------

    /// Lists an account's recovery codes.
    ///
    /// # Errors
    ///
    /// Returns a storage error.
    pub fn list_recovery_codes(&self, account_id: AccountId) -> AppResult<Vec<RecoveryCode>> {
        Ok(RecoveryCodeRepo::list_for_account(
            self.database.connection(),
            account_id,
        )?)
    }

    /// Stores recovery codes pasted as text.
    ///
    /// # Errors
    ///
    /// See [`crate::accounts::add_recovery_codes`].
    pub fn add_recovery_codes(
        &mut self,
        account_id: AccountId,
        pasted: &str,
    ) -> AppResult<Vec<RecoveryCode>> {
        let codes = crate::accounts::split_recovery_codes(pasted);
        if codes.is_empty() {
            return Err(AppError::invalid("no recovery codes were found in that text"));
        }
        crate::accounts::add_recovery_codes(self.database.connection(), &mut self.vault, account_id, codes)
    }

    /// Marks a recovery code used or unused.
    ///
    /// # Errors
    ///
    /// Returns a storage error.
    pub fn set_recovery_code_used(&mut self, id: RecoveryCodeId, used: bool) -> AppResult<RecoveryCode> {
        crate::accounts::set_recovery_code_used(self.database.connection(), id, used)
    }

    /// Copies a recovery code to the clipboard.
    ///
    /// # Errors
    ///
    /// See [`crate::accounts::reveal_recovery_code`].
    pub fn copy_recovery_code(&mut self, id: RecoveryCodeId) -> AppResult<()> {
        let code = crate::accounts::reveal_recovery_code(self.database.connection(), &mut self.vault, id)?;
        self.copy_secret(&code, CopyKind::RecoveryCode)
    }

    /// Reveals a recovery code for on-screen display.
    ///
    /// # Errors
    ///
    /// See [`crate::accounts::reveal_recovery_code`].
    pub fn reveal_recovery_code(&mut self, id: RecoveryCodeId) -> AppResult<SecretString> {
        crate::accounts::reveal_recovery_code(self.database.connection(), &mut self.vault, id)
    }

    /// Deletes a recovery code.
    ///
    /// # Errors
    ///
    /// See [`crate::accounts::delete_recovery_code`].
    pub fn delete_recovery_code(&mut self, id: RecoveryCodeId) -> AppResult<()> {
        crate::accounts::delete_recovery_code(self.database.connection(), &mut self.vault, id)
    }

    // ---- Clipboard --------------------------------------------------------

    /// Copies a secret, honouring the configured clear delay.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::ClipboardFailed`].
    pub fn copy_secret(&mut self, value: &SecretString, kind: CopyKind) -> AppResult<()> {
        self.vault.touch();
        self.clipboard.copy(
            value,
            kind,
            self.settings.clipboard.clear_enabled,
            std::time::Duration::from_secs(u64::from(self.settings.clipboard.clear_after_seconds)),
        )
    }

    /// Clears the clipboard now, if it still holds what this app put there.
    pub fn clear_clipboard(&self) -> bool {
        self.clipboard.clear_now()
    }

    // ---- Profiles ---------------------------------------------------------

    /// Lists profiles.
    ///
    /// # Errors
    ///
    /// Returns a storage error.
    pub fn list_profiles(&self) -> AppResult<Vec<BrowserProfile>> {
        Ok(ProfileRepo::list(self.database.connection())?)
    }

    /// Creates a profile.
    ///
    /// # Errors
    ///
    /// See [`crate::profiles::create_profile`].
    pub fn create_profile(&mut self, draft: &BrowserProfileDraft) -> AppResult<BrowserProfile> {
        crate::profiles::create_profile(&mut self.database, draft)
    }

    /// Updates a profile.
    ///
    /// # Errors
    ///
    /// See [`crate::profiles::update_profile`].
    pub fn update_profile(
        &mut self,
        id: ProfileId,
        draft: &BrowserProfileDraft,
    ) -> AppResult<BrowserProfile> {
        crate::profiles::update_profile(&mut self.database, id, draft)
    }

    /// Deletes a profile, optionally with its browser data.
    ///
    /// # Errors
    ///
    /// See [`crate::profiles::delete_profile`].
    pub fn delete_profile(&mut self, id: ProfileId, delete_browser_data: bool) -> AppResult<Option<PathBuf>> {
        let root = self.paths.root().to_path_buf();
        crate::profiles::delete_profile(&mut self.database, &root, id, delete_browser_data)
    }

    /// The observed status of a profile.
    ///
    /// # Errors
    ///
    /// Returns a storage error.
    pub fn profile_status(&self, id: ProfileId) -> AppResult<tw_domain::ProfileRuntimeStatus> {
        let profile = ProfileRepo::get(self.database.connection(), id)?;
        Ok(crate::profiles::observed_status(
            &self.sessions,
            self.paths.root(),
            &profile,
        ))
    }

    /// Launches a profile.
    ///
    /// If it is already running, its window is brought to the front and the
    /// caller is told, rather than a second conflicting browser being started.
    ///
    /// # Errors
    ///
    /// See [`tw_browser_profile::ProfileError`].
    pub async fn launch_profile(&mut self, id: ProfileId) -> AppResult<LaunchOutcome> {
        if self.sessions.contains(id) {
            let focused = self.sessions.focus(id).unwrap_or(false);
            return Ok(LaunchOutcome {
                started: false,
                focused,
                profile_id: id,
            });
        }

        let profile = ProfileRepo::get(self.database.connection(), id)?;
        let (version, executable) = crate::profiles::resolve_version(&self.thorium, &profile)?;
        let session = BrowserSession::launch(&executable, self.paths.root(), &profile, &version).await?;
        crate::profiles::record_session(&self.database, id, &session.state())?;
        self.sessions.insert(id, session);
        Ok(LaunchOutcome {
            started: true,
            focused: false,
            profile_id: id,
        })
    }

    /// Stops a running profile.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::ProfileNotRunning`] when nothing is running.
    pub async fn stop_profile(&mut self, id: ProfileId) -> AppResult<()> {
        let session = self
            .sessions
            .take(id)
            .ok_or_else(|| AppError::new(DiagnosticCode::ProfileNotRunning, "this profile is not running"))?;
        session.stop().await?;
        crate::profiles::clear_session(&self.database, id)?;
        Ok(())
    }

    /// Drops sessions whose browser exited on its own, and clears their rows.
    pub fn reap_exited_sessions(&mut self) -> Vec<ProfileId> {
        let dead = self.sessions.reap_exited();
        for id in &dead {
            let _ = crate::profiles::clear_session(&self.database, *id);
        }
        dead
    }

    /// Stops every running profile. Called at shutdown.
    pub async fn shutdown(&mut self) {
        self.clipboard.forget();
        self.sessions.stop_all().await;
        let _ = tw_storage::RuntimeRepo::clear_all(self.database.connection());
        self.vault.lock(LockReason::Shutdown);
    }

    // ---- Thorium ----------------------------------------------------------

    /// Lists installed Thorium versions.
    ///
    /// # Errors
    ///
    /// Returns a storage error.
    pub fn list_thorium_versions(&self) -> AppResult<Vec<crate::thorium::InstalledVersion>> {
        crate::thorium::list(&self.database, &self.thorium)
    }

    /// Looks up the newest installable release.
    ///
    /// # Errors
    ///
    /// See [`tw_thorium::ThoriumError`].
    pub async fn check_for_thorium_update(&self) -> AppResult<tw_domain::ThoriumRelease> {
        crate::thorium::check_for_update(&self.thorium, self.settings.thorium_channel).await
    }

    /// Installs a Thorium version.
    ///
    /// # Errors
    ///
    /// See [`tw_thorium::ThoriumError`].
    pub async fn install_thorium(
        &mut self,
        channel: ThoriumChannel,
        on_progress: impl FnMut(InstallProgress),
    ) -> AppResult<tw_domain::ThoriumInstallation> {
        let request = InstallRequest::latest(channel);
        crate::thorium::install(&mut self.database, &self.thorium, &request, on_progress).await
    }

    /// Selects a Thorium version.
    ///
    /// # Errors
    ///
    /// See [`crate::thorium::set_current`].
    pub fn set_current_thorium(&mut self, version: &str) -> AppResult<()> {
        crate::thorium::set_current(&mut self.database, &self.thorium, version)
    }

    /// Removes a Thorium version.
    ///
    /// # Errors
    ///
    /// See [`crate::thorium::remove`].
    pub fn remove_thorium(&mut self, version: &str) -> AppResult<()> {
        crate::thorium::remove(&mut self.database, &self.thorium, version)
    }

    /// Reverts to the previous Thorium version.
    ///
    /// # Errors
    ///
    /// See [`crate::thorium::rollback`].
    pub fn rollback_thorium(&mut self) -> AppResult<String> {
        crate::thorium::rollback(&mut self.database, &self.thorium)
    }

    // ---- Backup -----------------------------------------------------------

    /// Writes a logical backup.
    ///
    /// # Errors
    ///
    /// See [`crate::backup::create`].
    pub fn create_backup(&self) -> AppResult<crate::backup::BackupOutcome> {
        crate::backup::create(&self.paths, &self.database)
    }

    /// Lists the backups in the workspace.
    #[must_use]
    pub fn list_backups(&self) -> Vec<PathBuf> {
        crate::backup::list(&self.paths)
    }

    // ---- Diagnostics ------------------------------------------------------

    /// Builds the diagnostics report.
    ///
    /// # Errors
    ///
    /// Returns a storage error.
    pub fn diagnostics(&self) -> AppResult<DiagnosticReport> {
        let integrity = self
            .database
            .integrity_check()
            .unwrap_or_else(|e| format!("check failed: {e}"));

        let vault_state = match self.vault.state() {
            VaultState::Uninitialized => "uninitialized",
            VaultState::Locked { .. } => "locked",
            VaultState::Unlocked { .. } => "unlocked",
        };
        let vault_header = self.vault.peek_header().ok();
        let vault_secret_count = match self.vault.state() {
            VaultState::Unlocked { secret_count, .. } => Some(secret_count),
            _ => None,
        };

        let mut profiles = Vec::new();
        let mut factor_count = 0usize;
        for profile in ProfileRepo::list(self.database.connection())? {
            let layout = tw_browser_profile::ProfileLayout::new(self.paths.root(), &profile);
            let session = self.sessions.state(profile.id);
            profiles.push(ProfileDiagnostic {
                id: profile.id.to_string(),
                name: profile.name.clone(),
                status: crate::profiles::observed_status(&self.sessions, self.paths.root(), &profile),
                thorium_selection: profile.thorium.to_string(),
                locale: profile.locale.as_str().to_owned(),
                timezone: profile.timezone.as_str().to_owned(),
                user_data_present: layout.user_data_dir.is_dir(),
                cdp_active: session.as_ref().is_some_and(|s| s.cdp_port.is_some()),
                emulation_active: session.as_ref().is_some_and(|s| s.emulation_active),
            });
        }
        let accounts = AccountRepo::list(self.database.connection())?;
        for account in &accounts {
            factor_count += SecondFactorRepo::list_for_account(self.database.connection(), account.id)?.len();
        }

        Ok(DiagnosticsBuilder::new(&self.paths, &self.bootstrap).build(
            integrity,
            vault_state,
            vault_header,
            vault_secret_count,
            self.thorium.installed_versions(),
            self.thorium.current_version(),
            self.thorium
                .current_executable()
                .ok()
                .map(|p| p.to_string_lossy().into_owned()),
            &self.settings,
            profiles,
            accounts.len(),
            factor_count,
        ))
    }

    /// Borrows the database, for callers that need a repository directly.
    #[must_use]
    pub const fn database(&self) -> &Database {
        &self.database
    }

    /// Mutably borrows the database.
    #[must_use]
    pub const fn database_mut(&mut self) -> &mut Database {
        &mut self.database
    }
}

/// What a launch request did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchOutcome {
    /// Whether a browser was started.
    pub started: bool,
    /// Whether an already-running window was brought to the front.
    pub focused: bool,
    /// Which profile.
    pub profile_id: ProfileId,
}

#[cfg(test)]
mod tests {
    use tw_domain::ServiceKind;

    use super::*;

    fn workspace(dir: &tempfile::TempDir) -> Workspace {
        Workspace::open_in(dir.path()).expect("workspace")
    }

    const MASTER: &str = "correct horse battery staple";

    #[test]
    fn a_fresh_workspace_opens_with_defaults_and_nothing_in_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = workspace(&dir);
        assert!(workspace.bootstrap_report().first_run);
        assert_eq!(workspace.settings(), &WorkspaceSettings::default());
        assert!(matches!(workspace.vault_state(), VaultState::Uninitialized));
        assert!(workspace.list_accounts().expect("accounts").is_empty());
        assert!(workspace.list_profiles().expect("profiles").is_empty());
        assert!(workspace.list_thorium_versions().expect("versions").is_empty());
    }

    #[test]
    fn settings_are_validated_and_persisted_across_restarts() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut workspace = workspace(&dir);
            let mut settings = WorkspaceSettings {
                theme: tw_domain::ThemePreference::Dark,
                ..WorkspaceSettings::default()
            };
            settings.clipboard.clear_after_seconds = 45;
            workspace.set_settings(settings).expect("save");

            let mut invalid = WorkspaceSettings::default();
            invalid.clipboard.clear_after_seconds = 99_999;
            assert!(workspace.set_settings(invalid).is_err());
        }
        let reopened = workspace(&dir);
        assert_eq!(reopened.settings().theme, tw_domain::ThemePreference::Dark);
        assert_eq!(reopened.settings().clipboard.clear_after_seconds, 45);
    }

    #[test]
    fn the_full_account_lifecycle_works_end_to_end() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut workspace = workspace(&dir);
        workspace.create_vault(&SecretString::new(MASTER)).expect("vault");

        let account = workspace
            .create_account(
                &AccountDraft {
                    display_name: "Build bot".to_owned(),
                    service: Some(ServiceKind::GitHub),
                    username: Some("bot".to_owned()),
                    login_url: Some("https://github.com/login".to_owned()),
                    tags: vec!["ci".to_owned()],
                    ..Default::default()
                },
                Some(SecretString::new("hunter2")),
            )
            .expect("account");

        assert_eq!(workspace.list_accounts().expect("list").len(), 1);
        assert_eq!(
            workspace
                .reveal_account_password(account.id)
                .expect("reveal")
                .expose(),
            "hunter2"
        );

        // A standard TOTP factor, imported the way a QR code would provide it.
        let credential = tw_otp::parse_otpauth_uri(
            "otpauth://totp/Example:bot?secret=JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP&issuer=Example",
        )
        .expect("parse");
        let factor = workspace
            .add_otp_factor(account.id, "Authenticator", &credential)
            .expect("factor");
        let code = workspace.generate_code(factor.id).expect("code");
        assert_eq!(code.code.len(), 6);
        assert!(code.valid_for_seconds.is_some());

        let codes = workspace
            .add_recovery_codes(account.id, "1. aaaa-bbbb\n2. cccc-dddd\n")
            .expect("codes");
        assert_eq!(codes.len(), 2);
        let marked = workspace.set_recovery_code_used(codes[0].id, true).expect("mark");
        assert!(marked.used && marked.used_at.is_some());

        workspace.delete_account(account.id).expect("delete");
        assert!(workspace.list_accounts().expect("list").is_empty());
        assert_eq!(workspace.collect_orphaned_secrets().expect("collect"), 0);
    }

    #[test]
    fn state_survives_a_restart_of_the_manager() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (account_id, profile_id, factor_id) = {
            let mut workspace = workspace(&dir);
            workspace.create_vault(&SecretString::new(MASTER)).expect("vault");
            let account = workspace
                .create_account(
                    &AccountDraft {
                        display_name: "Bot".to_owned(),
                        ..Default::default()
                    },
                    Some(SecretString::new("hunter2")),
                )
                .expect("account");
            let credential = tw_otp::parse_otpauth_uri(
                "otpauth://totp/Example:bot?secret=JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP",
            )
            .expect("parse");
            let factor = workspace
                .add_otp_factor(account.id, "Authenticator", &credential)
                .expect("factor");
            let profile = workspace
                .create_profile(&BrowserProfileDraft {
                    name: "Work".to_owned(),
                    locale: Some("pl-PL".to_owned()),
                    timezone: Some("Europe/Warsaw".to_owned()),
                    ..Default::default()
                })
                .expect("profile");
            (account.id, profile.id, factor.id)
        };

        let mut reopened = workspace(&dir);
        assert!(!reopened.bootstrap_report().first_run);
        assert!(reopened.bootstrap_report().vault_exists);
        assert!(matches!(reopened.vault_state(), VaultState::Locked { .. }));

        // Metadata is readable while locked.
        assert_eq!(reopened.list_accounts().expect("accounts").len(), 1);
        let profile = reopened
            .list_profiles()
            .expect("profiles")
            .into_iter()
            .find(|p| p.id == profile_id)
            .expect("profile survived");
        assert_eq!(profile.locale.as_str(), "pl-PL");
        assert_eq!(profile.timezone.as_str(), "Europe/Warsaw");

        // Secrets need the master password again.
        assert!(reopened.reveal_account_password(account_id).is_err());
        reopened.unlock_vault(&SecretString::new(MASTER)).expect("unlock");
        assert_eq!(
            reopened
                .reveal_account_password(account_id)
                .expect("reveal")
                .expose(),
            "hunter2"
        );
        assert_eq!(reopened.generate_code(factor_id).expect("code").code.len(), 6);
    }

    #[test]
    fn two_profiles_get_independent_user_data_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut workspace = workspace(&dir);
        let first = workspace
            .create_profile(&BrowserProfileDraft {
                name: "First".to_owned(),
                ..Default::default()
            })
            .expect("first");
        let second = workspace
            .create_profile(&BrowserProfileDraft {
                name: "Second".to_owned(),
                ..Default::default()
            })
            .expect("second");
        assert_ne!(first.user_data_dir_name(), second.user_data_dir_name());
        assert_eq!(workspace.list_profiles().expect("list").len(), 2);
    }

    #[tokio::test]
    async fn launching_without_an_installed_browser_says_so() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut workspace = workspace(&dir);
        let profile = workspace
            .create_profile(&BrowserProfileDraft {
                name: "Work".to_owned(),
                ..Default::default()
            })
            .expect("profile");
        let error = workspace
            .launch_profile(profile.id)
            .await
            .expect_err("nothing installed");
        assert_eq!(error.code, DiagnosticCode::ThoriumNotInstalled);
        assert!(error.remedy.is_some());
    }

    #[tokio::test]
    async fn stopping_a_profile_that_is_not_running_is_reported() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut workspace = workspace(&dir);
        let profile = workspace
            .create_profile(&BrowserProfileDraft {
                name: "Work".to_owned(),
                ..Default::default()
            })
            .expect("profile");
        let error = workspace.stop_profile(profile.id).await.expect_err("not running");
        assert_eq!(error.code, DiagnosticCode::ProfileNotRunning);
    }

    #[test]
    fn a_second_workspace_over_the_same_folder_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _first = workspace(&dir);
        let error = Workspace::open_in(dir.path()).expect_err("must refuse");
        assert_eq!(error.code, DiagnosticCode::WorkspaceAlreadyRunning);
    }

    #[test]
    fn diagnostics_describe_the_workspace_without_exposing_secrets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut workspace = workspace(&dir);
        workspace.create_vault(&SecretString::new(MASTER)).expect("vault");
        workspace
            .create_account(
                &AccountDraft {
                    display_name: "Bot".to_owned(),
                    ..Default::default()
                },
                Some(SecretString::new("hunter2")),
            )
            .expect("account");
        workspace
            .create_profile(&BrowserProfileDraft {
                name: "Work".to_owned(),
                ..Default::default()
            })
            .expect("profile");

        let report = workspace.diagnostics().expect("diagnostics");
        assert_eq!(report.schema_version, tw_storage::SCHEMA_VERSION);
        assert_eq!(report.database_integrity, "ok");
        assert_eq!(report.vault_state, "unlocked");
        assert_eq!(report.vault_secret_count, Some(1));
        assert_eq!(report.account_count, 1);
        assert_eq!(report.profiles.len(), 1);
        assert!(report.workspace_writable);

        let text = report.to_shareable_text();
        assert!(!text.contains("hunter2"), "{text}");
        assert!(!text.contains(MASTER), "{text}");
        assert!(!text.to_lowercase().contains("correct horse"), "{text}");
    }

    #[test]
    fn a_backup_can_be_taken_and_listed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut workspace = workspace(&dir);
        workspace.create_vault(&SecretString::new(MASTER)).expect("vault");
        assert!(workspace.list_backups().is_empty());

        let outcome = workspace.create_backup().expect("backup");
        assert!(outcome.manifest.includes_vault);
        assert_eq!(workspace.list_backups().len(), 1);
    }

    #[test]
    fn locking_the_vault_stops_tracking_the_clipboard() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut workspace = workspace(&dir);
        workspace.create_vault(&SecretString::new(MASTER)).expect("vault");
        workspace.lock_vault(LockReason::Manual);
        assert!(matches!(
            workspace.vault_state(),
            VaultState::Locked {
                reason: LockReason::Manual
            }
        ));
        assert!(
            !workspace.clear_clipboard(),
            "nothing is being tracked after a lock"
        );
    }

    #[tokio::test]
    async fn shutdown_locks_the_vault_and_clears_runtime_state() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut workspace = workspace(&dir);
        workspace.create_vault(&SecretString::new(MASTER)).expect("vault");
        workspace.shutdown().await;
        assert!(matches!(
            workspace.vault_state(),
            VaultState::Locked {
                reason: LockReason::Shutdown
            }
        ));
        assert!(
            tw_storage::RuntimeRepo::list(workspace.database().connection())
                .expect("list")
                .is_empty()
        );
    }
}
