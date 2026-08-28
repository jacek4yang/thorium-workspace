//! Entry point.
#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(error) = thorium_workspace_lib::run() {
        // Reaching here means Tauri itself could not start, so there is no
        // window to report through. Under the Windows subsystem stderr goes
        // nowhere, so the message is also written beside the executable, or to
        // the temporary directory when that is not writable.
        let message = format!("Thorium Workspace could not start: {error}");
        eprintln!("{message}");
        write_startup_error(&message);
        std::process::exit(1);
    }
}

fn write_startup_error(message: &str) {
    const FILE_NAME: &str = "ThoriumWorkspace-startup-error.txt";
    let beside_exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(FILE_NAME)));
    if let Some(path) = beside_exe
        && std::fs::write(&path, message).is_ok()
    {
        return;
    }
    let _ = std::fs::write(std::env::temp_dir().join(FILE_NAME), message);
}
