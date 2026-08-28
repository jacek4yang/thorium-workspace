//! The Tauri application.
//!
//! This crate is the *boundary*, not the brain: it owns one
//! [`tw_controller::Workspace`], exposes typed commands over it, and runs the
//! background tasks the UI cannot (idle locking, reaping exited browsers).
//! Every decision about encryption, persistence and process lifetime lives
//! below this layer.
//!
//! # Startup failures
//!
//! Bootstrap can fail for reasons the user has to fix: the folder is read-only,
//! or another copy already owns it. The window opens anyway and the frontend
//! renders the error with its remedy. Exiting silently, or showing a blank
//! window, would leave a user with no idea what happened.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

pub mod commands;
pub mod events;
pub mod state;

use std::sync::Arc;

pub use state::AppState;

/// Builds and runs the application.
///
/// # Errors
///
/// Returns an error only when Tauri itself cannot start. A workspace that
/// cannot be prepared is surfaced in the UI instead.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    // rustls has no default crypto provider unless one is installed; do it once,
    // before anything can make an HTTPS request.
    tw_thorium::install_crypto_provider();

    let (state, log_guard) = match tw_controller::Workspace::open() {
        Ok(workspace) => {
            let guard = tw_controller::logging::init(&workspace.paths().logs_dir(), "info");
            tracing::info!(
                version = env!("CARGO_PKG_VERSION"),
                first_run = workspace.bootstrap_report().first_run,
                "workspace opened"
            );
            (Arc::new(AppState::ready(workspace)), guard)
        }
        Err(error) => {
            // No workspace means no log file to write to; the UI is the only
            // place this can be reported.
            eprintln!("Thorium Workspace could not open its workspace: {error}");
            (Arc::new(AppState::failed(error)), None)
        }
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(Arc::clone(&state))
        .setup({
            let state = Arc::clone(&state);
            move |app| {
                events::spawn_background_tasks(app.handle().clone(), Arc::clone(&state));
                Ok(())
            }
        })
        .on_window_event({
            let state = Arc::clone(&state);
            move |window, event| events::handle_window_event(window, event, &state)
        })
        .invoke_handler(commands::handler())
        .build(tauri::generate_context!())?
        .run(move |app_handle, event| {
            if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
                events::shutdown(app_handle, &state);
            }
        });

    drop(log_guard);
    Ok(())
}
