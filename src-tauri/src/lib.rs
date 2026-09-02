//! Tauri application shell for Thorium Workspace.
//!
//! All security-sensitive and persistent behavior lives in the Rust crates
//! (`thorium-workspace-controller` and below). This shell only wires Tauri
//! plugins, commands, and events. Errors cross to the frontend as
//! `{ code, message }` pairs built from stable diagnostic codes — never as
//! raw `Debug` output, which could embed sensitive context.

use std::time::{Duration, Instant};

use tauri::{Emitter, Manager as _};

use thorium_workspace_controller::error::ControllerError;
use thorium_workspace_controller::services::VaultStatus;
use thorium_workspace_controller::{
    DiagnosticsSnapshot, ReleaseOption, ThoriumVersionInfo, Workspace,
};
use thorium_workspace_domain::DiagnosticCode as _;
use thorium_workspace_domain::{
    Account, AccountInput, BrowserProfile, ProfileInput, RecoveryCode, SecondFactor,
    WorkspaceSettings,
};
use thorium_workspace_secrets::SecretText;

/// Frontend-safe error: stable code + user message.
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CmdError {
    /// Stable diagnostic code (e.g. `VAULT_LOCKED`).
    code: &'static str,
    /// Human-readable message without secret material.
    message: String,
}

impl CmdError {
    fn from_controller(error: ControllerError) -> Self {
        Self {
            code: error.diagnostic_code(),
            message: error.user_message(),
        }
    }
}

type CmdResult<T> = Result<T, CmdError>;

fn controller<T>(result: Result<T, ControllerError>) -> CmdResult<T> {
    result.map_err(CmdError::from_controller)
}

fn parse_id<T: std::str::FromStr<Err = thorium_workspace_domain::DomainError>>(
    id: &str,
) -> CmdResult<T> {
    id.parse()
        .map_err(|error| CmdError::from_controller(ControllerError::from(error)))
}

/// The workspace is the single piece of managed state.
type Ws<'a> = tauri::State<'a, Workspace>;

// ---------------------------------------------------------------------------
// Vault

#[tauri::command]
fn vault_status(ws: Ws<'_>) -> VaultStatus {
    ws.vault_status()
}

#[tauri::command]
fn vault_create(ws: Ws<'_>, master_password: String) -> CmdResult<()> {
    controller(ws.create_vault(&SecretText::new(&master_password)))
}

#[tauri::command]
fn vault_unlock(ws: Ws<'_>, master_password: String) -> CmdResult<()> {
    ws.record_activity(Instant::now());
    controller(ws.unlock_vault(&SecretText::new(&master_password)))
}

#[tauri::command]
fn vault_lock(ws: Ws<'_>) -> CmdResult<()> {
    controller(ws.lock_vault())
}

#[tauri::command]
fn vault_change_password(ws: Ws<'_>, current: String, new_password: String) -> CmdResult<()> {
    controller(
        ws.change_master_password(&SecretText::new(&current), &SecretText::new(&new_password)),
    )
}

// ---------------------------------------------------------------------------
// Settings

#[tauri::command]
fn settings_get(ws: Ws<'_>) -> CmdResult<WorkspaceSettings> {
    controller(ws.settings())
}

#[tauri::command]
fn settings_save(ws: Ws<'_>, settings: WorkspaceSettings) -> CmdResult<()> {
    controller(ws.save_settings(&settings))
}

// ---------------------------------------------------------------------------
// Profiles

#[tauri::command]
fn profiles_list(ws: Ws<'_>) -> CmdResult<Vec<BrowserProfile>> {
    controller(ws.list_profiles())
}

#[tauri::command]
fn profile_get(ws: Ws<'_>, profile_id: String) -> CmdResult<BrowserProfile> {
    controller(ws.get_profile(parse_id(&profile_id)?))
}

#[tauri::command]
fn profile_create(ws: Ws<'_>, input: ProfileInput) -> CmdResult<BrowserProfile> {
    ws.record_activity(Instant::now());
    controller(ws.create_profile(&input))
}

#[tauri::command]
fn profile_update(ws: Ws<'_>, profile: BrowserProfile) -> CmdResult<BrowserProfile> {
    ws.record_activity(Instant::now());
    controller(ws.update_profile(&profile))
}

#[tauri::command]
fn profile_delete(ws: Ws<'_>, profile_id: String) -> CmdResult<()> {
    controller(ws.delete_profile(parse_id(&profile_id)?))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LaunchPlanDto {
    executable: String,
    arguments: Vec<String>,
    user_data_dir: String,
    version: String,
}

#[tauri::command]
fn profile_launch(ws: Ws<'_>, profile_id: String) -> CmdResult<LaunchPlanDto> {
    ws.record_activity(Instant::now());
    let plan = controller(ws.launch_profile(parse_id(&profile_id)?))?;
    Ok(LaunchPlanDto {
        executable: plan.executable.to_string_lossy().into_owned(),
        arguments: plan.arguments,
        user_data_dir: plan.user_data_dir.to_string_lossy().into_owned(),
        version: plan.version,
    })
}

#[tauri::command]
fn profile_stop(ws: Ws<'_>, profile_id: String) -> CmdResult<()> {
    controller(ws.stop_profile(parse_id(&profile_id)?))
}

#[tauri::command]
fn running_profiles(ws: Ws<'_>) -> CmdResult<Vec<String>> {
    Ok(controller(ws.running_profiles())?
        .into_iter()
        .map(|id| id.to_string())
        .collect())
}

// ---------------------------------------------------------------------------
// Accounts

#[tauri::command]
fn accounts_list(ws: Ws<'_>, profile_id: String) -> CmdResult<Vec<Account>> {
    controller(ws.list_accounts(parse_id(&profile_id)?))
}

#[tauri::command]
fn account_create(ws: Ws<'_>, profile_id: String, input: AccountInput) -> CmdResult<Account> {
    controller(ws.create_account(parse_id(&profile_id)?, &input))
}

#[tauri::command]
fn account_update(ws: Ws<'_>, account: Account) -> CmdResult<()> {
    controller(ws.update_account(&account))
}

#[tauri::command]
fn account_delete(ws: Ws<'_>, account_id: String) -> CmdResult<()> {
    controller(ws.delete_account(parse_id(&account_id)?))
}

// ---------------------------------------------------------------------------
// Passwords

#[tauri::command]
fn password_set(ws: Ws<'_>, account_id: String, password: String) -> CmdResult<()> {
    controller(ws.set_password(parse_id(&account_id)?, &SecretText::new(&password)))
}

#[tauri::command]
fn password_delete(ws: Ws<'_>, account_id: String) -> CmdResult<()> {
    controller(ws.delete_password(parse_id(&account_id)?))
}

#[tauri::command]
fn password_copy(ws: Ws<'_>, account_id: String) -> CmdResult<u32> {
    let delay = controller(ws.copy_password(parse_id(&account_id)?, Instant::now()))?;
    Ok(delay.as_secs() as u32)
}

#[tauri::command]
fn password_reveal(ws: Ws<'_>, account_id: String) -> CmdResult<String> {
    // Explicit reveal: the only command that returns secret material, and
    // the frontend calls it solely for user-initiated reveal actions.
    let password = controller(ws.get_password(parse_id(&account_id)?))?;
    Ok(password.expose().to_owned())
}

// ---------------------------------------------------------------------------
// Factors / OTP / QR

#[tauri::command]
fn factor_import_otpauth(ws: Ws<'_>, account_id: String, uri: String) -> CmdResult<SecondFactor> {
    controller(ws.import_otpauth_uri(parse_id(&account_id)?, &SecretText::new(&uri)))
}

#[tauri::command]
fn factor_import_qr_file(
    ws: Ws<'_>,
    account_id: String,
    image_path: String,
) -> CmdResult<SecondFactor> {
    let bytes = std::fs::read(&image_path).map_err(|source| {
        CmdError::from_controller(ControllerError::Platform(
            thorium_workspace_windows_platform::PlatformError::Io {
                path: image_path.clone().into(),
                source,
            },
        ))
    })?;
    controller(ws.import_qr_image(parse_id(&account_id)?, &bytes))
}

#[tauri::command]
fn factor_add_external(
    ws: Ws<'_>,
    account_id: String,
    label: Option<String>,
    note: Option<String>,
) -> CmdResult<SecondFactor> {
    controller(ws.add_external_factor(parse_id(&account_id)?, label, note))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct OtpCodeDto {
    code: String,
    seconds_remaining: u32,
}

#[tauri::command]
fn factor_generate(ws: Ws<'_>, factor_id: String) -> CmdResult<OtpCodeDto> {
    let unix_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0);
    let (code, seconds_remaining) =
        controller(ws.generate_otp_code(parse_id(&factor_id)?, unix_time))?;
    Ok(OtpCodeDto {
        code,
        seconds_remaining,
    })
}

#[tauri::command]
fn factor_delete(ws: Ws<'_>, factor_id: String) -> CmdResult<()> {
    controller(ws.delete_factor(parse_id(&factor_id)?))
}

// ---------------------------------------------------------------------------
// Recovery codes

#[tauri::command]
fn recovery_add(
    ws: Ws<'_>,
    account_id: String,
    values: Vec<String>,
) -> CmdResult<Vec<RecoveryCode>> {
    let secrets: Vec<SecretText> = values.iter().map(|value| SecretText::new(value)).collect();
    controller(ws.add_recovery_codes(parse_id(&account_id)?, &secrets))
}

#[tauri::command]
fn recovery_list(ws: Ws<'_>, account_id: String) -> CmdResult<Vec<RecoveryCode>> {
    controller(ws.list_recovery_codes(parse_id(&account_id)?))
}

#[tauri::command]
fn recovery_mark_used(ws: Ws<'_>, code_id: String) -> CmdResult<()> {
    controller(ws.mark_recovery_code_used(parse_id(&code_id)?, chrono::Utc::now()))
}

#[tauri::command]
fn recovery_delete(ws: Ws<'_>, code_id: String) -> CmdResult<()> {
    controller(ws.delete_recovery_code(parse_id(&code_id)?))
}

// ---------------------------------------------------------------------------
// Thorium management

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    downloaded: u64,
    total: u64,
}

#[tauri::command]
async fn thorium_discover(ws: Ws<'_>) -> CmdResult<Vec<ReleaseOption>> {
    controller(ws.discover_thorium_releases().await)
}

#[tauri::command]
fn thorium_installed(ws: Ws<'_>) -> CmdResult<Vec<ThoriumVersionInfo>> {
    controller(ws.installed_thorium_versions())
}

#[tauri::command]
fn thorium_set_current(ws: Ws<'_>, version: String) -> CmdResult<()> {
    ws.record_activity(Instant::now());
    controller(ws.set_current_thorium(&version))
}

#[tauri::command]
fn thorium_delete(ws: Ws<'_>, version: String) -> CmdResult<()> {
    controller(ws.delete_thorium_version(&version))
}

#[tauri::command]
async fn thorium_install(
    app: tauri::AppHandle,
    ws: Ws<'_>,
    url: String,
    version: String,
    variant: String,
    size_bytes: u64,
) -> CmdResult<()> {
    ws.record_activity(Instant::now());
    let emitter = app.clone();
    let progress = move |downloaded: u64, total: u64| {
        // The upstream content-length is authoritative when present;
        // fall back to the asset size for the denominator.
        let _ = emitter.emit(
            "thorium://progress",
            DownloadProgress {
                downloaded,
                total: if total == 0 { size_bytes } else { total },
            },
        );
    };
    controller(
        ws.install_thorium(&url, &version, &variant, &progress)
            .await,
    )
}

// ---------------------------------------------------------------------------
// Diagnostics

#[tauri::command]
fn diagnostics(ws: Ws<'_>) -> CmdResult<DiagnosticsSnapshot> {
    controller(ws.diagnostics())
}

// ---------------------------------------------------------------------------

/// One-second housekeeping loop: clipboard conditional clear + idle lock.
/// Owned by the app lifecycle (no detached uncontrolled threads).
fn spawn_housekeeping(handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let workspace = handle.state::<Workspace>();
            // A failed tick is not fatal: the conditional-clear semantics and
            // the next tick keep behavior safe.
            let _ = workspace.housekeeping_tick(Instant::now());
        }
    });
}

pub fn run() {
    // Bootstrap before the webview so a non-writable directory fails
    // fast with an actionable message (stderr is best-effort in release
    // builds; the message is actionable in dev console and logs).
    let workspace = match Workspace::bootstrap(None) {
        Ok(workspace) => workspace,
        Err(error) => {
            eprintln!("fatal: workspace bootstrap failed: {error}");
            std::process::exit(1);
        }
    };

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(workspace)
        .invoke_handler(tauri::generate_handler![
            vault_status,
            vault_create,
            vault_unlock,
            vault_lock,
            vault_change_password,
            settings_get,
            settings_save,
            profiles_list,
            profile_get,
            profile_create,
            profile_update,
            profile_delete,
            profile_launch,
            profile_stop,
            running_profiles,
            accounts_list,
            account_create,
            account_update,
            account_delete,
            password_set,
            password_delete,
            password_copy,
            password_reveal,
            factor_import_otpauth,
            factor_import_qr_file,
            factor_add_external,
            factor_generate,
            factor_delete,
            recovery_add,
            recovery_list,
            recovery_mark_used,
            recovery_delete,
            thorium_discover,
            thorium_installed,
            thorium_set_current,
            thorium_delete,
            thorium_install,
            diagnostics,
        ])
        .on_window_event(|window, event| {
            let workspace = window.app_handle().state::<Workspace>();
            match event {
                tauri::WindowEvent::Focused(true) => {
                    workspace.record_activity(Instant::now());
                }
                // Tauri exposes no dedicated minimize event on Windows; a
                // resize to a zero-sized client area is the minimize
                // transition. Only acts when the user enabled the setting.
                tauri::WindowEvent::Resized(size)
                    if (size.width == 0 || size.height == 0)
                        && workspace
                            .settings()
                            .map(|settings| settings.vault_lock_on_minimize)
                            .unwrap_or(false) =>
                {
                    let _ = workspace.lock_vault();
                }
                _ => {}
            }
        })
        .setup(|app| {
            spawn_housekeeping(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(error) = result {
        eprintln!("fatal: failed to start Thorium Workspace: {error}");
        std::process::exit(1);
    }
}
