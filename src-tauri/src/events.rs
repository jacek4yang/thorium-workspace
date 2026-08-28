//! Events and background tasks.
//!
//! The UI cannot notice that a browser exited, or that the vault has been idle
//! long enough to lock. These tasks do, and tell it.

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager, Window, WindowEvent};
use tw_vault::{LockReason, VaultState};

use crate::state::AppState;

/// Event names, in one place so the frontend and backend cannot drift.
pub mod names {
    /// The vault locked or unlocked.
    pub const VAULT_STATE: &str = "vault:state";
    /// A profile started, stopped or exited.
    pub const PROFILES_CHANGED: &str = "profiles:changed";
    /// The set of installed Thorium versions changed.
    pub const THORIUM_CHANGED: &str = "thorium:changed";
    /// Progress during a Thorium install.
    pub const THORIUM_INSTALL_PROGRESS: &str = "thorium:install-progress";
}

/// How often the idle-lock and session-reaper ticks run.
///
/// Five seconds is frequent enough that an idle lock feels prompt and rare
/// enough to be invisible in a process list.
const TICK: Duration = Duration::from_secs(5);

/// Tells the UI the vault's state changed.
pub fn emit_vault_state(app: &AppHandle, state: &VaultState) {
    crate::commands::emit(app, names::VAULT_STATE, state.clone());
}

/// Tells the UI to re-read the profile list.
pub fn emit_profiles_changed(app: &AppHandle) {
    crate::commands::emit(app, names::PROFILES_CHANGED, ());
}

/// Tells the UI to re-read the Thorium version list.
pub fn emit_thorium_changed(app: &AppHandle) {
    crate::commands::emit(app, names::THORIUM_CHANGED, ());
}

/// Reports install progress.
pub fn emit_install_progress(app: &AppHandle, progress: &tw_thorium::InstallProgress) {
    crate::commands::emit(
        app,
        names::THORIUM_INSTALL_PROGRESS,
        crate::commands::install_progress_event(progress),
    );
}

/// Starts the periodic tasks.
pub fn spawn_background_tasks(app: AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(TICK);
        // A missed tick must not cause a burst of catch-up ticks.
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            ticker.tick().await;

            // `try_with` rather than `with`: a periodic tick must never queue
            // behind a long-running install and then fire a dozen times.
            let outcome = state.try_with(|workspace| {
                let locked = workspace.lock_vault_if_idle();
                let exited = workspace.reap_exited_sessions();
                let vault_state = workspace.vault_state();
                (locked, exited, vault_state)
            });

            let Some((locked, exited, vault_state)) = outcome else {
                continue;
            };
            if locked {
                tracing::info!("the vault locked after the idle timeout");
                emit_vault_state(&app, &vault_state);
            }
            if !exited.is_empty() {
                tracing::info!(count = exited.len(), "browser sessions exited on their own");
                emit_profiles_changed(&app);
            }
        }
    });
}

/// Applies the lock-on-minimize setting.
pub fn handle_window_event(window: &Window, event: &WindowEvent, state: &Arc<AppState>) {
    let should_check = match event {
        // Tauri reports minimise as a resize to a zero-ish size on some
        // platforms, so the window is asked directly rather than inferred.
        WindowEvent::Resized(_) | WindowEvent::Focused(false) => true,
        _ => false,
    };
    if !should_check {
        return;
    }
    if !window.is_minimized().unwrap_or(false) {
        return;
    }
    if let Some(state) = state.try_with(tw_controller::Workspace::lock_vault_on_minimize)
        && state
    {
        tracing::info!("the vault locked because the window was minimised");
        crate::commands::emit(
            &window.app_handle().clone(),
            names::VAULT_STATE,
            VaultState::Locked {
                reason: LockReason::Minimized,
            },
        );
    }
}

/// Stops browsers, clears runtime state and locks the vault before exit.
///
/// Runs on the Tauri runtime because stopping a session is asynchronous.
pub fn shutdown(app: &AppHandle, state: &Arc<AppState>) {
    tracing::info!("shutting down");
    let state = Arc::clone(state);
    let _ = app;
    tauri::async_runtime::block_on(async move {
        // A shutdown must not hang the process: if something is genuinely stuck
        // the Job Object still terminates the browser tree when this process
        // exits and its handles close.
        let _ = tokio::time::timeout(Duration::from_secs(20), async {
            let _ = state
                .with_async(async |workspace| {
                    workspace.shutdown().await;
                    Ok(())
                })
                .await;
        })
        .await;
    });
}
