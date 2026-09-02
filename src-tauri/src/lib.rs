//! Tauri application shell for Thorium Workspace.
//!
//! All security-sensitive and persistent behavior lives in the Rust crates
//! (`thorium-workspace-controller` and below). This shell only wires Tauri
//! plugins, commands, and events.

pub fn run() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!());
    if let Err(error) = result {
        // A GUI app has no console to print to; stderr is best-effort here.
        // The controller phase replaces this with a native error dialog.
        eprintln!("fatal: failed to start Thorium Workspace: {error}");
        std::process::exit(1);
    }
}
