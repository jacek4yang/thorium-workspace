//! The typed command boundary.
//!
//! Every command takes and returns plain serializable data. Secrets cross this
//! boundary in exactly three directions, all of them explicit:
//!
//! * **in**: a master password, an account password, an OTP secret or recovery
//!   codes the user just typed;
//! * **out**: a value the user explicitly asked to *reveal* on screen;
//! * **never**: everything else. Copying a secret to the clipboard happens
//!   entirely inside the backend, so a copied password is never serialized to
//!   the frontend at all.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};
use tw_controller::{AppError, CopyKind};
use tw_domain::{
    Account, AccountDraft, AccountId, BrowserProfile, BrowserProfileDraft, DiagnosticCode, FactorId,
    OtpParameters, ProfileId, RecoveryCode, RecoveryCodeId, SecondFactor, SecondFactorDraft, ServicePreset,
    ThoriumChannel, TimeZoneId, WorkspaceSettings, account::service_presets,
};
use tw_secrets::SecretString;
use tw_vault::{LockReason, VaultState};

use crate::state::SharedState;

/// Registers every command with Tauri.
pub fn handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    tauri::generate_handler![
        startup_status,
        get_settings,
        set_settings,
        list_timezones,
        list_service_presets,
        vault_state,
        create_vault,
        unlock_vault,
        lock_vault,
        change_master_password,
        collect_orphaned_secrets,
        list_accounts,
        list_accounts_for_profile,
        create_account,
        update_account,
        set_account_password,
        delete_account,
        reveal_account_password,
        copy_account_password,
        list_factors,
        add_otp_factor_from_uri,
        add_otp_factor_manual,
        add_external_factor,
        import_otp_from_image_file,
        import_otp_from_clipboard,
        import_otp_from_screen,
        generate_code,
        copy_code,
        delete_factor,
        list_recovery_codes,
        add_recovery_codes,
        set_recovery_code_used,
        reveal_recovery_code,
        copy_recovery_code,
        delete_recovery_code,
        copy_plain_value,
        clear_clipboard,
        list_profiles,
        create_profile,
        update_profile,
        delete_profile,
        profile_status,
        launch_profile,
        stop_profile,
        list_thorium_versions,
        check_for_thorium_update,
        install_thorium,
        set_current_thorium,
        remove_thorium,
        rollback_thorium,
        create_backup,
        list_backups,
        diagnostics,
        copy_diagnostics,
    ]
}

/// What the frontend needs before it can render anything.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupStatus {
    /// Application version.
    pub app_version: String,
    /// The startup failure, when the workspace could not be opened.
    pub error: Option<AppError>,
    /// The workspace root, when there is one.
    pub workspace_root: Option<String>,
    /// Whether this run created the workspace.
    pub first_run: bool,
    /// Whether a vault file exists.
    pub vault_exists: bool,
    /// Whether Windows process supervision is active.
    pub windows_supervision: bool,
}

/// Reports whether the application actually started.
#[tauri::command]
async fn startup_status(state: State<'_, SharedState>) -> Result<StartupStatus, AppError> {
    let app_version = env!("CARGO_PKG_VERSION").to_owned();
    let windows_supervision = tw_windows_platform::ProcessGroup::is_supervising();
    if let Some(error) = state.startup_error() {
        return Ok(StartupStatus {
            app_version,
            error: Some(error.clone()),
            workspace_root: None,
            first_run: false,
            vault_exists: false,
            windows_supervision,
        });
    }
    state
        .with(|workspace| {
            let report = workspace.bootstrap_report();
            Ok(StartupStatus {
                app_version,
                error: None,
                workspace_root: Some(report.workspace_root.clone()),
                first_run: report.first_run,
                vault_exists: report.vault_exists,
                windows_supervision,
            })
        })
        .await
}

// ---- Settings -------------------------------------------------------------

#[tauri::command]
async fn get_settings(state: State<'_, SharedState>) -> Result<WorkspaceSettings, AppError> {
    state.with(|w| Ok(w.settings().clone())).await
}

#[tauri::command]
async fn set_settings(
    state: State<'_, SharedState>,
    settings: WorkspaceSettings,
) -> Result<WorkspaceSettings, AppError> {
    state
        .with(|w| {
            w.set_settings(settings)?;
            Ok(w.settings().clone())
        })
        .await
}

#[tauri::command]
fn list_timezones() -> Vec<&'static str> {
    TimeZoneId::available()
}

#[tauri::command]
fn list_service_presets() -> Vec<ServicePreset> {
    service_presets()
}

// ---- Vault ----------------------------------------------------------------

#[tauri::command]
async fn vault_state(state: State<'_, SharedState>) -> Result<VaultState, AppError> {
    state.with(|w| Ok(w.vault_state())).await
}

#[tauri::command]
async fn create_vault(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
    password: String,
) -> Result<VaultState, AppError> {
    let password = SecretString::new(password);
    let result = state.with(|w| w.create_vault(&password)).await?;
    crate::events::emit_vault_state(&app, &result);
    Ok(result)
}

#[tauri::command]
async fn unlock_vault(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
    password: String,
) -> Result<VaultState, AppError> {
    let password = SecretString::new(password);
    let result = state.with(|w| w.unlock_vault(&password)).await?;
    crate::events::emit_vault_state(&app, &result);
    Ok(result)
}

#[tauri::command]
async fn lock_vault(app: tauri::AppHandle, state: State<'_, SharedState>) -> Result<VaultState, AppError> {
    let result = state.with(|w| Ok(w.lock_vault(LockReason::Manual))).await?;
    crate::events::emit_vault_state(&app, &result);
    Ok(result)
}

#[tauri::command]
async fn change_master_password(
    state: State<'_, SharedState>,
    current: String,
    next: String,
) -> Result<(), AppError> {
    let current = SecretString::new(current);
    let next = SecretString::new(next);
    state.with(|w| w.change_master_password(&current, &next)).await
}

#[tauri::command]
async fn collect_orphaned_secrets(state: State<'_, SharedState>) -> Result<usize, AppError> {
    state
        .with(tw_controller::Workspace::collect_orphaned_secrets)
        .await
}

// ---- Accounts -------------------------------------------------------------

#[tauri::command]
async fn list_accounts(state: State<'_, SharedState>) -> Result<Vec<Account>, AppError> {
    state.with(|w| w.list_accounts()).await
}

#[tauri::command]
async fn list_accounts_for_profile(
    state: State<'_, SharedState>,
    profile_id: ProfileId,
) -> Result<Vec<Account>, AppError> {
    state.with(|w| w.list_accounts_for_profile(profile_id)).await
}

#[tauri::command]
async fn create_account(
    state: State<'_, SharedState>,
    draft: AccountDraft,
    password: Option<String>,
) -> Result<Account, AppError> {
    let password = password.map(SecretString::new);
    state.with(move |w| w.create_account(&draft, password)).await
}

#[tauri::command]
async fn update_account(
    state: State<'_, SharedState>,
    id: AccountId,
    draft: AccountDraft,
) -> Result<Account, AppError> {
    state.with(move |w| w.update_account(id, &draft)).await
}

#[tauri::command]
async fn set_account_password(
    state: State<'_, SharedState>,
    id: AccountId,
    password: Option<String>,
) -> Result<Account, AppError> {
    let password = password.map(SecretString::new);
    state.with(move |w| w.set_account_password(id, password)).await
}

#[tauri::command]
async fn delete_account(state: State<'_, SharedState>, id: AccountId) -> Result<(), AppError> {
    state.with(move |w| w.delete_account(id)).await
}

/// Reveals a password for on-screen display.
///
/// The one command that deliberately returns plaintext. Called only when the
/// user presses "show".
#[tauri::command]
async fn reveal_account_password(state: State<'_, SharedState>, id: AccountId) -> Result<String, AppError> {
    state
        .with(move |w| w.reveal_account_password(id).map(|s| s.expose().to_owned()))
        .await
}

/// Copies a password to the clipboard without it ever reaching the frontend.
#[tauri::command]
async fn copy_account_password(state: State<'_, SharedState>, id: AccountId) -> Result<(), AppError> {
    state.with(move |w| w.copy_account_password(id)).await
}

// ---- Second factors -------------------------------------------------------

#[tauri::command]
async fn list_factors(
    state: State<'_, SharedState>,
    account_id: AccountId,
) -> Result<Vec<SecondFactor>, AppError> {
    state.with(move |w| w.list_factors(account_id)).await
}

#[tauri::command]
async fn add_otp_factor_from_uri(
    state: State<'_, SharedState>,
    account_id: AccountId,
    label: Option<String>,
    uri: String,
) -> Result<SecondFactor, AppError> {
    let credential = tw_otp::parse_otpauth_uri(&uri)?;
    let label = label.unwrap_or_else(|| credential.suggested_label());
    state
        .with(move |w| w.add_otp_factor(account_id, &label, &credential))
        .await
}

#[tauri::command]
async fn add_otp_factor_manual(
    state: State<'_, SharedState>,
    account_id: AccountId,
    label: String,
    parameters: OtpParameters,
    secret: String,
) -> Result<SecondFactor, AppError> {
    let secret = SecretString::new(secret);
    state
        .with(move |w| w.add_otp_factor_manual(account_id, &label, &parameters, &secret))
        .await
}

#[tauri::command]
async fn add_external_factor(
    state: State<'_, SharedState>,
    account_id: AccountId,
    draft: SecondFactorDraft,
) -> Result<SecondFactor, AppError> {
    state
        .with(move |w| w.add_external_factor(account_id, &draft))
        .await
}

/// What a QR import found, without the secret.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedFactor {
    /// The factor that was created.
    pub factor: SecondFactor,
    /// The label it was given.
    pub label: String,
}

#[tauri::command]
async fn import_otp_from_image_file(
    state: State<'_, SharedState>,
    account_id: AccountId,
    path: PathBuf,
) -> Result<ImportedFactor, AppError> {
    let credential = tw_qr::credential_from_image_file(&path)?;
    let label = credential.suggested_label();
    let factor = state
        .with({
            let label = label.clone();
            move |w| w.add_otp_factor(account_id, &label, &credential)
        })
        .await?;
    Ok(ImportedFactor { factor, label })
}

#[tauri::command]
async fn import_otp_from_clipboard(
    state: State<'_, SharedState>,
    account_id: AccountId,
) -> Result<ImportedFactor, AppError> {
    let (width, height, rgba) = tw_controller::clipboard::read_clipboard_image()?;
    let credential = tw_qr::credential_from_rgba(width, height, &rgba)?;
    let label = credential.suggested_label();
    let factor = state
        .with({
            let label = label.clone();
            move |w| w.add_otp_factor(account_id, &label, &credential)
        })
        .await?;
    Ok(ImportedFactor { factor, label })
}

/// Scans the whole screen for a two-factor QR code.
///
/// Deliberately a whole-screen scan rather than a drag-to-select overlay: there
/// is no transparent always-on-top window to get wrong, and nothing for a user
/// to mis-drag. The captured pixels never leave the process.
#[tauri::command]
async fn import_otp_from_screen(
    state: State<'_, SharedState>,
    account_id: AccountId,
) -> Result<ImportedFactor, AppError> {
    let capture = tw_windows_platform::capture_virtual_screen().map_err(|e| {
        let error: AppError = e.into();
        if error.code == DiagnosticCode::UnsupportedPlatform {
            error.with_remedy("Import the QR code from an image file or the clipboard instead.")
        } else {
            error
        }
    })?;
    let credential = tw_qr::credential_from_rgba(capture.width, capture.height, &capture.rgba)?;
    let label = credential.suggested_label();
    let factor = state
        .with({
            let label = label.clone();
            move |w| w.add_otp_factor(account_id, &label, &credential)
        })
        .await?;
    Ok(ImportedFactor { factor, label })
}

#[tauri::command]
async fn generate_code(
    state: State<'_, SharedState>,
    factor_id: FactorId,
) -> Result<tw_otp::OtpCode, AppError> {
    state.with(move |w| w.generate_code(factor_id)).await
}

#[tauri::command]
async fn copy_code(state: State<'_, SharedState>, factor_id: FactorId) -> Result<tw_otp::OtpCode, AppError> {
    state.with(move |w| w.copy_code(factor_id)).await
}

#[tauri::command]
async fn delete_factor(state: State<'_, SharedState>, factor_id: FactorId) -> Result<(), AppError> {
    state.with(move |w| w.delete_factor(factor_id)).await
}

// ---- Recovery codes -------------------------------------------------------

#[tauri::command]
async fn list_recovery_codes(
    state: State<'_, SharedState>,
    account_id: AccountId,
) -> Result<Vec<RecoveryCode>, AppError> {
    state.with(move |w| w.list_recovery_codes(account_id)).await
}

#[tauri::command]
async fn add_recovery_codes(
    state: State<'_, SharedState>,
    account_id: AccountId,
    pasted: String,
) -> Result<Vec<RecoveryCode>, AppError> {
    state
        .with(move |w| w.add_recovery_codes(account_id, &pasted))
        .await
}

#[tauri::command]
async fn set_recovery_code_used(
    state: State<'_, SharedState>,
    id: RecoveryCodeId,
    used: bool,
) -> Result<RecoveryCode, AppError> {
    state.with(move |w| w.set_recovery_code_used(id, used)).await
}

#[tauri::command]
async fn reveal_recovery_code(state: State<'_, SharedState>, id: RecoveryCodeId) -> Result<String, AppError> {
    state
        .with(move |w| w.reveal_recovery_code(id).map(|s| s.expose().to_owned()))
        .await
}

#[tauri::command]
async fn copy_recovery_code(state: State<'_, SharedState>, id: RecoveryCodeId) -> Result<(), AppError> {
    state.with(move |w| w.copy_recovery_code(id)).await
}

#[tauri::command]
async fn delete_recovery_code(state: State<'_, SharedState>, id: RecoveryCodeId) -> Result<(), AppError> {
    state.with(move |w| w.delete_recovery_code(id)).await
}

// ---- Clipboard ------------------------------------------------------------

/// Copies a non-secret field, such as a username.
///
/// Kept separate from the secret paths so a plain field is never scheduled for
/// automatic clearing.
#[tauri::command]
async fn copy_plain_value(state: State<'_, SharedState>, value: String) -> Result<(), AppError> {
    let value = SecretString::new(value);
    state
        .with(move |w| w.copy_secret(&value, CopyKind::PlainField))
        .await
}

#[tauri::command]
async fn clear_clipboard(state: State<'_, SharedState>) -> Result<bool, AppError> {
    state.with(|w| Ok(w.clear_clipboard())).await
}

// ---- Profiles -------------------------------------------------------------

/// A profile plus everything the list view shows.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    /// The profile.
    pub profile: BrowserProfile,
    /// Observed status.
    pub status: tw_domain::ProfileRuntimeStatus,
    /// How many accounts are attached.
    pub account_count: usize,
}

#[tauri::command]
async fn list_profiles(state: State<'_, SharedState>) -> Result<Vec<ProfileView>, AppError> {
    state
        .with(|w| {
            let mut views = Vec::new();
            for profile in w.list_profiles()? {
                let status = w.profile_status(profile.id)?;
                views.push(ProfileView {
                    account_count: profile.account_ids.len(),
                    status,
                    profile,
                });
            }
            Ok(views)
        })
        .await
}

#[tauri::command]
async fn create_profile(
    state: State<'_, SharedState>,
    draft: BrowserProfileDraft,
) -> Result<BrowserProfile, AppError> {
    state.with(move |w| w.create_profile(&draft)).await
}

#[tauri::command]
async fn update_profile(
    state: State<'_, SharedState>,
    id: ProfileId,
    draft: BrowserProfileDraft,
) -> Result<BrowserProfile, AppError> {
    state.with(move |w| w.update_profile(id, &draft)).await
}

#[tauri::command]
async fn delete_profile(
    state: State<'_, SharedState>,
    id: ProfileId,
    delete_browser_data: bool,
) -> Result<Option<String>, AppError> {
    state
        .with(move |w| {
            w.delete_profile(id, delete_browser_data)
                .map(|path| path.map(|p| p.to_string_lossy().into_owned()))
        })
        .await
}

#[tauri::command]
async fn profile_status(
    state: State<'_, SharedState>,
    id: ProfileId,
) -> Result<tw_domain::ProfileRuntimeStatus, AppError> {
    state.with(move |w| w.profile_status(id)).await
}

#[tauri::command]
async fn launch_profile(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
    id: ProfileId,
) -> Result<tw_controller::workspace::LaunchOutcome, AppError> {
    let outcome = state
        .with_async(async move |w| w.launch_profile(id).await)
        .await?;
    crate::events::emit_profiles_changed(&app);
    Ok(outcome)
}

#[tauri::command]
async fn stop_profile(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
    id: ProfileId,
) -> Result<(), AppError> {
    state.with_async(async move |w| w.stop_profile(id).await).await?;
    crate::events::emit_profiles_changed(&app);
    Ok(())
}

// ---- Thorium --------------------------------------------------------------

#[tauri::command]
async fn list_thorium_versions(
    state: State<'_, SharedState>,
) -> Result<Vec<tw_controller::thorium::InstalledVersion>, AppError> {
    state.with(|w| w.list_thorium_versions()).await
}

/// A release, as the update check reports it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvailableRelease {
    /// Upstream tag.
    pub tag: String,
    /// Release title.
    pub name: String,
    /// The upstream page, so the user can check for themselves.
    pub html_url: String,
    /// The asset that would be downloaded.
    pub asset_name: String,
    /// Its size in bytes.
    pub asset_size_bytes: u64,
    /// Whether that version is already installed.
    pub already_installed: bool,
}

#[tauri::command]
async fn check_for_thorium_update(state: State<'_, SharedState>) -> Result<AvailableRelease, AppError> {
    let release = state
        .with_async(async |w| w.check_for_thorium_update().await)
        .await?;
    let asset = release.choose_asset()?;
    let version = release.install_version();
    let already_installed = state
        .with(|w| {
            Ok(w.list_thorium_versions()?
                .into_iter()
                .any(|v| v.version == version))
        })
        .await?;
    Ok(AvailableRelease {
        tag: release.tag.clone(),
        name: release.name.clone(),
        html_url: release.html_url.clone(),
        asset_name: asset.name.clone(),
        asset_size_bytes: asset.size_bytes,
        already_installed,
    })
}

#[tauri::command]
async fn install_thorium(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
    channel: Option<ThoriumChannel>,
) -> Result<tw_domain::ThoriumInstallation, AppError> {
    let channel = match channel {
        Some(channel) => channel,
        None => state.with(|w| Ok(w.settings().thorium_channel)).await?,
    };
    let progress_app = app.clone();
    let installation = state
        .with_async(async move |w| {
            w.install_thorium(channel, move |progress| {
                crate::events::emit_install_progress(&progress_app, &progress);
            })
            .await
        })
        .await?;
    crate::events::emit_thorium_changed(&app);
    Ok(installation)
}

#[tauri::command]
async fn set_current_thorium(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
    version: String,
) -> Result<(), AppError> {
    state.with(move |w| w.set_current_thorium(&version)).await?;
    crate::events::emit_thorium_changed(&app);
    Ok(())
}

#[tauri::command]
async fn remove_thorium(
    app: tauri::AppHandle,
    state: State<'_, SharedState>,
    version: String,
) -> Result<(), AppError> {
    state.with(move |w| w.remove_thorium(&version)).await?;
    crate::events::emit_thorium_changed(&app);
    Ok(())
}

#[tauri::command]
async fn rollback_thorium(app: tauri::AppHandle, state: State<'_, SharedState>) -> Result<String, AppError> {
    let version = state.with(tw_controller::Workspace::rollback_thorium).await?;
    crate::events::emit_thorium_changed(&app);
    Ok(version)
}

// ---- Backup and diagnostics ----------------------------------------------

#[tauri::command]
async fn create_backup(
    state: State<'_, SharedState>,
) -> Result<tw_controller::backup::BackupOutcome, AppError> {
    state.with(|w| w.create_backup()).await
}

/// A backup as the list shows it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntry {
    /// Full path.
    pub path: String,
    /// File name.
    pub name: String,
    /// Size in bytes.
    pub bytes: u64,
}

#[tauri::command]
async fn list_backups(state: State<'_, SharedState>) -> Result<Vec<BackupEntry>, AppError> {
    state
        .with(|w| {
            Ok(w.list_backups()
                .into_iter()
                .map(|path| BackupEntry {
                    bytes: std::fs::metadata(&path).map(|m| m.len()).unwrap_or_default(),
                    name: path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    path: path.to_string_lossy().into_owned(),
                })
                .collect())
        })
        .await
}

#[tauri::command]
async fn diagnostics(state: State<'_, SharedState>) -> Result<tw_controller::DiagnosticReport, AppError> {
    state.with(|w| w.diagnostics()).await
}

/// Copies a redacted diagnostic report to the clipboard.
///
/// The report is redacted before it is copied, not after: the value that reaches
/// the clipboard is the same one the user can read on screen.
#[tauri::command]
async fn copy_diagnostics(state: State<'_, SharedState>) -> Result<String, AppError> {
    state
        .with(|w| {
            let text = w.diagnostics()?.to_shareable_text();
            w.copy_secret(&SecretString::new(text.clone()), CopyKind::PlainField)?;
            Ok(text)
        })
        .await
}

/// Emitted with the payload of the install-progress event.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgressEvent {
    /// The progress stage.
    pub progress: tw_thorium::InstallProgress,
}

/// Convenience for `events` so it does not need to re-derive the payload shape.
pub(crate) fn install_progress_event(progress: &tw_thorium::InstallProgress) -> InstallProgressEvent {
    InstallProgressEvent {
        progress: progress.clone(),
    }
}

/// Emits an event, logging rather than failing when there is no window.
pub(crate) fn emit<T: Serialize + Clone>(app: &tauri::AppHandle, event: &str, payload: T) {
    if let Err(error) = app.emit(event, payload) {
        tracing::debug!(event, error = %error, "an event could not be delivered");
    }
}
