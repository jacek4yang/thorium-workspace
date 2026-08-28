//! Portable bootstrap.
//!
//! On first start the application must, in order:
//!
//! 1. resolve the directory the executable is in;
//! 2. verify that directory is actually writable;
//! 3. claim it, so a second instance cannot race this one;
//! 4. create the portable directory layout;
//! 5. initialise versioned storage;
//! 6. clean up stale runtime and temporary files from an interrupted run.
//!
//! Business data is rooted beside the executable and **never** silently falls
//! back to `%APPDATA%` or `%LOCALAPPDATA%`. A read-only folder is a clear,
//! actionable error, not a reason to scatter the user's passwords somewhere they
//! did not choose.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tw_domain::DiagnosticCode;
use tw_storage::{Database, DatabaseOptions};
use tw_windows_platform::SingleInstanceGuard;

use crate::error::{AppError, AppResult};

/// The portable directory layout.
///
/// ```text
/// ThoriumWorkspace/
/// ├── ThoriumWorkspace.exe
/// ├── workspace.db            metadata (no secrets)
/// ├── vault/workspace.twvault the encrypted vault
/// ├── browsers/thorium/       installed browser versions
/// ├── profiles/<id>/User Data one directory per browser profile
/// ├── runtime/                transient state, cleared at startup
/// ├── backups/                logical backups
/// └── logs/                   rolling log files
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspacePaths {
    root: PathBuf,
}

impl WorkspacePaths {
    /// Builds the layout rooted at `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The workspace root: the directory the executable lives in.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The metadata database.
    #[must_use]
    pub fn database(&self) -> PathBuf {
        self.root.join("workspace.db")
    }

    /// The vault directory.
    #[must_use]
    pub fn vault_dir(&self) -> PathBuf {
        self.root.join("vault")
    }

    /// The vault file.
    #[must_use]
    pub fn vault_file(&self) -> PathBuf {
        self.vault_dir().join("workspace.twvault")
    }

    /// The browsers directory.
    #[must_use]
    pub fn browsers_dir(&self) -> PathBuf {
        self.root.join("browsers")
    }

    /// The profiles directory.
    #[must_use]
    pub fn profiles_dir(&self) -> PathBuf {
        self.root.join("profiles")
    }

    /// The transient runtime directory, cleared at startup.
    #[must_use]
    pub fn runtime_dir(&self) -> PathBuf {
        self.root.join("runtime")
    }

    /// The backups directory.
    #[must_use]
    pub fn backups_dir(&self) -> PathBuf {
        self.root.join("backups")
    }

    /// The logs directory.
    #[must_use]
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Every directory the workspace owns.
    #[must_use]
    pub fn all_directories(&self) -> Vec<PathBuf> {
        vec![
            self.vault_dir(),
            self.browsers_dir(),
            self.profiles_dir(),
            self.runtime_dir(),
            self.backups_dir(),
            self.logs_dir(),
        ]
    }
}

/// Alias kept for readability at call sites that talk about "the layout".
pub type WorkspaceLayout = WorkspacePaths;

/// What bootstrap did, for the Diagnostics page and the first-run screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapReport {
    /// The resolved workspace root.
    pub workspace_root: String,
    /// Whether the root was writable.
    pub writable: bool,
    /// Whether this run created the layout for the first time.
    pub first_run: bool,
    /// The schema version after migration.
    pub schema_version: u32,
    /// Whether a vault file already exists.
    pub vault_exists: bool,
    /// How many stale runtime files were removed.
    pub stale_files_removed: usize,
    /// How many stale staging entries the Thorium manager removed.
    pub stale_staging_removed: usize,
    /// The single-instance object name held for this workspace.
    pub instance_name: String,
}

/// Resolves and prepares the portable workspace.
#[derive(Debug)]
pub struct Bootstrap {
    paths: WorkspacePaths,
    guard: SingleInstanceGuard,
    report: BootstrapReport,
    database: Database,
}

impl Bootstrap {
    /// Runs bootstrap against the directory containing the current executable.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::ExecutableDirectoryUnresolved`] when the
    /// executable path cannot be determined, and otherwise whatever
    /// [`Bootstrap::run_in`] returns.
    pub fn run() -> AppResult<Self> {
        let exe = std::env::current_exe().map_err(|e| {
            AppError::new(
                DiagnosticCode::ExecutableDirectoryUnresolved,
                format!("the application's own location could not be determined: {e}"),
            )
            .with_remedy("Run the application from a normal folder rather than a temporary or virtual one.")
        })?;
        let root = exe
            .parent()
            .ok_or_else(|| {
                AppError::new(
                    DiagnosticCode::ExecutableDirectoryUnresolved,
                    "the application's own location has no parent directory",
                )
            })?
            .to_path_buf();
        Self::run_in(&root)
    }

    /// Runs bootstrap against an explicit root.
    ///
    /// Used by tests and by the `--workspace` development flag. Production
    /// startup uses [`Bootstrap::run`], which is always the executable's own
    /// directory.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::WorkspaceNotWritable`] when the directory
    /// cannot be written, [`DiagnosticCode::WorkspaceAlreadyRunning`] when
    /// another instance holds it, and [`DiagnosticCode::WorkspaceLayoutFailed`]
    /// when the layout cannot be created.
    pub fn run_in(root: &Path) -> AppResult<Self> {
        let paths = WorkspacePaths::new(root);
        let first_run = !paths.database().exists();

        verify_writable(root)?;

        // Claimed before anything is opened: two managers sharing one database,
        // vault and profile set is the failure this prevents.
        let guard = SingleInstanceGuard::acquire(root).map_err(|e| {
            let error: AppError = e.into();
            if error.code == DiagnosticCode::WorkspaceAlreadyRunning {
                error.with_remedy(
                    "Close the other Thorium Workspace window, or run this copy from a different folder.",
                )
            } else {
                error
            }
        })?;

        for dir in paths.all_directories() {
            std::fs::create_dir_all(&dir).map_err(|e| {
                AppError::new(
                    DiagnosticCode::WorkspaceLayoutFailed,
                    format!("{} could not be created: {e}", dir.display()),
                )
            })?;
        }

        let stale_files_removed = clean_runtime_dir(&paths.runtime_dir())?;

        let database = Database::open(paths.database(), &DatabaseOptions::default())?;
        let schema_version = database.schema_version()?;

        let report = BootstrapReport {
            workspace_root: root.display().to_string(),
            writable: true,
            first_run,
            schema_version,
            vault_exists: paths.vault_file().is_file(),
            stale_files_removed,
            stale_staging_removed: 0,
            instance_name: guard.name().to_owned(),
        };

        Ok(Self {
            paths,
            guard,
            report,
            database,
        })
    }

    /// The workspace layout.
    #[must_use]
    pub const fn paths(&self) -> &WorkspacePaths {
        &self.paths
    }

    /// What bootstrap did.
    #[must_use]
    pub const fn report(&self) -> &BootstrapReport {
        &self.report
    }

    /// Records how many stale Thorium staging entries were cleaned.
    pub fn set_stale_staging_removed(&mut self, count: usize) {
        self.report.stale_staging_removed = count;
    }

    /// The instance guard. Must be held for the life of the process.
    #[must_use]
    pub const fn guard(&self) -> &SingleInstanceGuard {
        &self.guard
    }

    /// Consumes the bootstrap, returning its parts.
    #[must_use]
    pub fn into_parts(self) -> (WorkspacePaths, SingleInstanceGuard, Database, BootstrapReport) {
        (self.paths, self.guard, self.database, self.report)
    }
}

/// Verifies the directory exists and can actually be written to.
///
/// Checking the read-only attribute is not enough: a directory can be
/// unwritable because of an ACL, because it is on read-only media, or because
/// the app is running from a virtualised install location. The only reliable
/// test is to write a file.
fn verify_writable(root: &Path) -> AppResult<()> {
    let unwritable = |detail: String| {
        AppError::new(
            DiagnosticCode::WorkspaceNotWritable,
            format!("Thorium Workspace cannot write to {}: {detail}", root.display()),
        )
        .with_remedy(
            "Move ThoriumWorkspace.exe to a folder you can write to, such as a folder in your user \
             profile or on a removable drive, and run it again. It deliberately never stores your \
             data anywhere other than beside itself.",
        )
    };

    if !root.exists() {
        std::fs::create_dir_all(root).map_err(|e| unwritable(e.to_string()))?;
    }
    if !root.is_dir() {
        return Err(unwritable("it is not a directory".to_owned()));
    }

    let probe = root.join(".thorium-workspace-write-test");
    std::fs::write(&probe, b"ok").map_err(|e| unwritable(e.to_string()))?;
    let readback = std::fs::read(&probe).map_err(|e| unwritable(e.to_string()))?;
    let _ = std::fs::remove_file(&probe);
    if readback != b"ok" {
        return Err(unwritable("a test file did not read back correctly".to_owned()));
    }
    Ok(())
}

/// Reads the effective user id on Unix, used only to skip a permission test
/// that is meaningless when running as root.
#[cfg(all(unix, test))]
fn effective_uid() -> u32 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata("/proc/self").map_or(u32::MAX, |m| m.uid())
}

/// Removes everything in the runtime directory.
///
/// Only this project's own transient directory is touched. Installed browsers,
/// profile data, the database, the vault and backups are never cleaned
/// implicitly: recovery must not be able to destroy the user's data.
fn clean_runtime_dir(runtime: &Path) -> AppResult<usize> {
    let mut removed = 0usize;
    if let Ok(entries) = std::fs::read_dir(runtime) {
        for entry in entries.flatten() {
            let path = entry.path();
            let result = if path.is_dir() {
                std::fs::remove_dir_all(&path)
            } else {
                std::fs::remove_file(&path)
            };
            match result {
                Ok(()) => removed += 1,
                Err(e) => {
                    // A file another process still holds open is not a reason to
                    // refuse to start.
                    tracing::warn!(path = %path.display(), error = %e, "a stale runtime file could not be removed");
                }
            }
        }
    }
    std::fs::create_dir_all(runtime).map_err(|e| {
        AppError::new(
            DiagnosticCode::StaleRuntimeCleanupFailed,
            format!("{} could not be recreated: {e}", runtime.display()),
        )
    })?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_is_rooted_beside_the_executable_and_never_in_appdata() {
        let paths = WorkspacePaths::new("C:\\Portable\\ThoriumWorkspace");
        for path in [
            paths.database(),
            paths.vault_file(),
            paths.browsers_dir(),
            paths.profiles_dir(),
            paths.runtime_dir(),
            paths.backups_dir(),
            paths.logs_dir(),
        ] {
            assert!(
                path.starts_with(paths.root()),
                "{path:?} escaped the workspace root"
            );
            let text = path.to_string_lossy().to_lowercase();
            assert!(!text.contains("appdata"), "{path:?}");
            assert!(!text.contains("localappdata"), "{path:?}");
        }
    }

    #[test]
    fn bootstrap_creates_the_whole_layout_and_reports_a_first_run() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bootstrap = Bootstrap::run_in(dir.path()).expect("bootstrap");
        let report = bootstrap.report();

        assert!(report.first_run);
        assert!(report.writable);
        assert!(!report.vault_exists, "a fresh workspace has no vault yet");
        assert_eq!(report.schema_version, tw_storage::SCHEMA_VERSION);
        assert!(report.instance_name.starts_with("ThoriumWorkspace-"));

        for path in bootstrap.paths().all_directories() {
            assert!(path.is_dir(), "{path:?} was not created");
        }
        assert!(bootstrap.paths().database().is_file());
    }

    #[test]
    fn a_second_bootstrap_reports_that_it_is_not_a_first_run() {
        let dir = tempfile::tempdir().expect("tempdir");
        drop(Bootstrap::run_in(dir.path()).expect("first"));
        let second = Bootstrap::run_in(dir.path()).expect("second");
        assert!(!second.report().first_run);
        assert_eq!(second.report().schema_version, tw_storage::SCHEMA_VERSION);
    }

    #[test]
    fn a_second_concurrent_instance_is_refused_with_a_remedy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _first = Bootstrap::run_in(dir.path()).expect("first");
        let error = Bootstrap::run_in(dir.path()).expect_err("second must be refused");
        assert_eq!(error.code, DiagnosticCode::WorkspaceAlreadyRunning);
        assert!(
            error.remedy.is_some(),
            "the user must be told what to do about it"
        );
    }

    #[test]
    fn separate_workspaces_can_be_bootstrapped_at_once() {
        let a = tempfile::tempdir().expect("tempdir");
        let b = tempfile::tempdir().expect("tempdir");
        let _first = Bootstrap::run_in(a.path()).expect("a");
        let _second = Bootstrap::run_in(b.path()).expect("b");
    }

    #[test]
    fn stale_runtime_files_are_cleaned_and_nothing_else_is() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let bootstrap = Bootstrap::run_in(dir.path()).expect("first");
            let paths = bootstrap.paths().clone();
            // Simulate an interrupted run.
            std::fs::write(paths.runtime_dir().join("session.json"), b"stale").expect("write");
            std::fs::create_dir_all(paths.runtime_dir().join("scratch")).expect("mkdir");
            // Data that must survive.
            std::fs::write(paths.backups_dir().join("backup.zip"), b"keep").expect("write");
            std::fs::create_dir_all(paths.profiles_dir().join("abc")).expect("mkdir");
            std::fs::write(paths.profiles_dir().join("abc").join("data"), b"keep").expect("write");
        }

        let bootstrap = Bootstrap::run_in(dir.path()).expect("second");
        assert_eq!(bootstrap.report().stale_files_removed, 2);
        let paths = bootstrap.paths();
        assert!(paths.runtime_dir().is_dir());
        assert_eq!(std::fs::read_dir(paths.runtime_dir()).expect("read").count(), 0);
        assert!(
            paths.backups_dir().join("backup.zip").is_file(),
            "backups must never be cleaned"
        );
        assert!(
            paths.profiles_dir().join("abc").join("data").is_file(),
            "profile data must never be cleaned"
        );
    }

    #[test]
    fn an_existing_vault_is_reported() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let bootstrap = Bootstrap::run_in(dir.path()).expect("first");
            std::fs::write(bootstrap.paths().vault_file(), b"not a real vault").expect("write");
        }
        assert!(
            Bootstrap::run_in(dir.path())
                .expect("second")
                .report()
                .vault_exists
        );
    }

    #[test]
    fn an_unwritable_directory_fails_with_an_actionable_message() {
        // A path that cannot exist as a directory: a file stands where the
        // workspace root would be.
        let dir = tempfile::tempdir().expect("tempdir");
        let blocked = dir.path().join("a-file-not-a-directory");
        std::fs::write(&blocked, b"x").expect("write");

        let error = Bootstrap::run_in(&blocked).expect_err("must fail");
        assert_eq!(error.code, DiagnosticCode::WorkspaceNotWritable);
        assert!(error.message.contains("cannot write"), "{}", error.message);
        let remedy = error.remedy.expect("a remedy is required");
        assert!(remedy.contains("folder you can write to"), "{remedy}");
        assert!(
            remedy.contains("never stores your data anywhere other than beside itself"),
            "{remedy}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_read_only_directory_is_detected_by_actually_writing() {
        use std::os::unix::fs::PermissionsExt;

        // Root bypasses directory permission bits, so the write probe would
        // succeed and this behaviour cannot be observed. The check itself is
        // still exercised on Windows CI and by the test above.
        if effective_uid() == 0 {
            return;
        }

        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("read-only");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o555)).expect("chmod");

        let result = Bootstrap::run_in(&root);
        // Restore permissions so the temporary directory can be cleaned up.
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).expect("chmod");

        let error = result.expect_err("a read-only workspace must be refused");
        assert_eq!(error.code, DiagnosticCode::WorkspaceNotWritable);
    }

    #[test]
    fn the_write_probe_leaves_nothing_behind() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bootstrap = Bootstrap::run_in(dir.path()).expect("bootstrap");
        drop(bootstrap);
        let names: Vec<String> = std::fs::read_dir(dir.path())
            .expect("read")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(!names.iter().any(|n| n.contains("write-test")), "{names:?}");
    }
}
