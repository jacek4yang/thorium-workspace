//! Portable workspace bootstrap.
//!
//! All persistent business data is rooted beside the executable
//! (`std::env::current_exe()`). The layout mirrors the README contract:
//!
//! ```text
//! <exe dir>/
//! ├── workspace.db
//! ├── vault/
//! ├── browsers/thorium/versions/
//! ├── profiles/
//! ├── runtime/
//! ├── backups/
//! └── logs/
//! ```
//!
//! Bootstrap never falls back to `%APPDATA%`/`%LOCALAPPDATA%`: if the
//! executable directory is not writable the error explains how to fix it.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use crate::error::PlatformError;

/// Directory names created under the workspace root at first start.
pub const LAYOUT_DIRS: &[&str] = &[
    "vault",
    "browsers",
    "browsers/thorium",
    "browsers/thorium/versions",
    "profiles",
    "runtime",
    "backups",
    "logs",
];

/// Resolves the directory containing the running executable.
pub fn exe_dir() -> Result<PathBuf, PlatformError> {
    let exe = std::env::current_exe().map_err(|source| PlatformError::ExePath { source })?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or(PlatformError::ExePath {
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "executable path has no parent directory",
            ),
        })
}

/// Probes a directory for writability by creating and removing a
/// uniquely-named temporary file.
pub fn verify_writable(dir: &Path) -> Result<(), PlatformError> {
    let probe = dir.join(format!(".write-probe-{}", std::process::id()));
    let result = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe);
    match result {
        Ok(file) => {
            drop(file);
            // Best-effort cleanup; a leftover probe file would violate the
            // tidy workspace contract, so a failure here is reported.
            if let Err(source) = std::fs::remove_file(&probe) {
                return Err(PlatformError::Io {
                    path: probe,
                    source,
                });
            }
            Ok(())
        }
        Err(source) => {
            let _ = source;
            Err(PlatformError::NotWritable {
                path: dir.to_path_buf(),
            })
        }
    }
}

/// The canonical workspace root (the executable's directory).
pub fn workspace_root() -> Result<PathBuf, PlatformError> {
    let root = exe_dir()?;
    verify_writable(&root)?;
    Ok(root)
}

/// Creates the portable directory layout under `root`, returning the
/// resolved root. Idempotent.
pub fn initialize_layout(root: &Path) -> Result<(), PlatformError> {
    verify_writable(root)?;
    for relative in LAYOUT_DIRS {
        let dir = root.join(relative);
        std::fs::create_dir_all(&dir).map_err(|source| PlatformError::Io { path: dir, source })?;
    }
    Ok(())
}

/// Resolves a file path relative to the workspace root.
pub fn under_workspace(root: &Path, relative: &str) -> PathBuf {
    root.join(relative)
}

#[cfg(test)]
mod tests {
    use super::*;
    use thorium_workspace_domain::DiagnosticCode as _;

    #[test]
    fn writability_probe_passes_on_writable_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        verify_writable(dir.path()).expect("writable");
        // No probe files linger.
        let leftovers: Vec<_> = std::fs::read_dir(dir.path()).expect("list").collect();
        assert!(leftovers.is_empty(), "probe must clean up after itself");
    }

    #[test]
    fn writability_probe_fails_on_missing_dir() {
        let error =
            verify_writable(Path::new("Z:/definitely/not/here")).expect_err("missing drive");
        assert_eq!(error.diagnostic_code(), "PLATFORM_NOT_WRITABLE");
    }

    #[test]
    fn layout_is_idempotent_and_complete() {
        let dir = tempfile::tempdir().expect("tempdir");
        initialize_layout(dir.path()).expect("first run");
        initialize_layout(dir.path()).expect("second run");
        for relative in LAYOUT_DIRS {
            let joined = dir.path().join(relative);
            assert!(joined.is_dir(), "{relative} missing after initialization");
        }
    }

    #[test]
    fn exe_dir_resolves_inside_target() {
        // During tests the executable lives in the Cargo target tree; the
        // assertion only proves that a real directory came back.
        let dir = exe_dir().expect("exe dir");
        assert!(dir.is_dir());
    }
}
