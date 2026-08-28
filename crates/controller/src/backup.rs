//! Logical backup and restore.
//!
//! A backup captures what cannot be recreated: the metadata database, the
//! encrypted vault and the settings. Browser `User Data` is deliberately
//! excluded — it is gigabytes of recreatable cache and history, and copying a
//! *running* Chromium profile produces a torn snapshot that looks valid and is
//! not.
//!
//! The vault goes into the archive exactly as it is on disk: still encrypted,
//! still needing the master password. A backup is therefore no more sensitive
//! than the workspace it came from, but it is no less sensitive either.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tw_domain::{DiagnosticCode, Timestamp};
use tw_storage::Database;

use crate::bootstrap::WorkspacePaths;
use crate::error::{AppError, AppResult};

/// The manifest written into every backup archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    /// Backup format version.
    pub format_version: u32,
    /// The application version that wrote it.
    pub app_version: String,
    /// The database schema version at the time.
    pub schema_version: u32,
    /// When it was taken, in Unix epoch seconds.
    pub created_at: i64,
    /// Whether the archive contains a vault.
    pub includes_vault: bool,
}

/// Current backup format version.
pub const BACKUP_FORMAT_VERSION: u32 = 1;

const MANIFEST_ENTRY: &str = "manifest.json";
const DATABASE_ENTRY: &str = "workspace.db";
const VAULT_ENTRY: &str = "vault/workspace.twvault";

/// Largest backup archive accepted on restore.
///
/// A logical backup is a database plus a vault: a few megabytes at most. A
/// wildly larger archive is not one of ours.
pub const MAX_BACKUP_BYTES: u64 = 512 * 1024 * 1024;

/// What a backup produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupOutcome {
    /// Where the archive was written.
    pub path: String,
    /// Its size in bytes.
    pub bytes: u64,
    /// Its manifest.
    pub manifest: BackupManifest,
}

/// Writes a logical backup into the workspace's `backups` directory.
///
/// The database is copied through SQLite's online backup API rather than as a
/// file, so the snapshot is consistent even with the WAL active.
///
/// # Errors
///
/// Returns [`DiagnosticCode::BackupFailed`] on any failure.
pub fn create(paths: &WorkspacePaths, db: &Database) -> AppResult<BackupOutcome> {
    let failed = |detail: String| AppError::new(DiagnosticCode::BackupFailed, detail);

    std::fs::create_dir_all(paths.backups_dir()).map_err(|e| failed(e.to_string()))?;
    let stamp = Timestamp::now().as_unix_seconds();
    let archive_path = paths
        .backups_dir()
        .join(format!("thorium-workspace-backup-{stamp}.zip"));

    // Snapshot the database into the runtime directory first: zipping the live
    // file would capture a torn state.
    let snapshot = paths.runtime_dir().join(format!("backup-{stamp}.db"));
    db.backup_to(&snapshot).map_err(|e| failed(e.to_string()))?;

    let manifest = BackupManifest {
        format_version: BACKUP_FORMAT_VERSION,
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        schema_version: db.schema_version().map_err(|e| failed(e.to_string()))?,
        created_at: stamp,
        includes_vault: paths.vault_file().is_file(),
    };

    let result = write_archive(&archive_path, &snapshot, paths, &manifest);
    // The snapshot is transient whatever happened.
    let _ = std::fs::remove_file(&snapshot);
    result?;

    let bytes = std::fs::metadata(&archive_path)
        .map_err(|e| failed(e.to_string()))?
        .len();
    Ok(BackupOutcome {
        path: archive_path.to_string_lossy().into_owned(),
        bytes,
        manifest,
    })
}

fn write_archive(
    archive_path: &Path,
    snapshot: &Path,
    paths: &WorkspacePaths,
    manifest: &BackupManifest,
) -> AppResult<()> {
    let failed = |detail: String| AppError::new(DiagnosticCode::BackupFailed, detail);

    let file = std::fs::File::create(archive_path).map_err(|e| failed(e.to_string()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    zip.start_file(MANIFEST_ENTRY, options)
        .map_err(|e| failed(e.to_string()))?;
    let encoded = serde_json::to_vec_pretty(manifest).map_err(|e| failed(e.to_string()))?;
    zip.write_all(&encoded).map_err(|e| failed(e.to_string()))?;

    zip.start_file(DATABASE_ENTRY, options)
        .map_err(|e| failed(e.to_string()))?;
    let mut database = std::fs::File::open(snapshot).map_err(|e| failed(e.to_string()))?;
    std::io::copy(&mut database, &mut zip).map_err(|e| failed(e.to_string()))?;

    if manifest.includes_vault {
        zip.start_file(VAULT_ENTRY, options)
            .map_err(|e| failed(e.to_string()))?;
        let mut vault = std::fs::File::open(paths.vault_file()).map_err(|e| failed(e.to_string()))?;
        std::io::copy(&mut vault, &mut zip).map_err(|e| failed(e.to_string()))?;
    }

    zip.finish().map_err(|e| failed(e.to_string()))?;
    Ok(())
}

/// Reads a backup's manifest without restoring anything.
///
/// # Errors
///
/// Returns [`DiagnosticCode::RestoreFailed`] when the archive is unreadable or
/// is not a Thorium Workspace backup.
pub fn inspect(archive_path: &Path) -> AppResult<BackupManifest> {
    let failed = |detail: String| AppError::new(DiagnosticCode::RestoreFailed, detail);

    let metadata = std::fs::metadata(archive_path).map_err(|e| failed(e.to_string()))?;
    if metadata.len() > MAX_BACKUP_BYTES {
        return Err(failed(
            "the archive is too large to be a workspace backup".to_owned(),
        ));
    }
    let file = std::fs::File::open(archive_path).map_err(|e| failed(e.to_string()))?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|_| failed("the file is not a readable archive".to_owned()))?;
    let mut entry = zip
        .by_name(MANIFEST_ENTRY)
        .map_err(|_| failed("the archive is not a Thorium Workspace backup".to_owned()))?;
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|e| failed(e.to_string()))?;
    let manifest: BackupManifest =
        serde_json::from_str(&text).map_err(|_| failed("the backup manifest is not readable".to_owned()))?;
    if manifest.format_version > BACKUP_FORMAT_VERSION {
        return Err(failed(format!(
            "this backup uses format {} which this version cannot read",
            manifest.format_version
        )));
    }
    Ok(manifest)
}

/// What a restore produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcome {
    /// The manifest that was restored from.
    pub manifest: BackupManifest,
    /// Where the pre-restore safety copy was written.
    pub safety_backup: Option<String>,
    /// Whether a vault was restored.
    pub vault_restored: bool,
}

/// Restores a backup over the workspace.
///
/// The current database and vault are copied aside first, unconditionally: a
/// restore is the one operation that can destroy everything the user has, and
/// "I restored the wrong file" must be recoverable.
///
/// The caller must have closed the database and locked the vault beforehand; the
/// restored files only take effect on the next start.
///
/// # Errors
///
/// Returns [`DiagnosticCode::RestoreFailed`] on any failure.
pub fn restore(paths: &WorkspacePaths, archive_path: &Path) -> AppResult<RestoreOutcome> {
    let failed = |detail: String| AppError::new(DiagnosticCode::RestoreFailed, detail);
    let manifest = inspect(archive_path)?;

    let stamp = Timestamp::now().as_unix_seconds();
    let safety_dir = paths.backups_dir().join(format!("pre-restore-{stamp}"));
    std::fs::create_dir_all(&safety_dir).map_err(|e| failed(e.to_string()))?;
    let mut safety_backup = None;
    if paths.database().is_file() {
        std::fs::copy(paths.database(), safety_dir.join("workspace.db"))
            .map_err(|e| failed(e.to_string()))?;
        safety_backup = Some(safety_dir.to_string_lossy().into_owned());
    }
    if paths.vault_file().is_file() {
        std::fs::copy(paths.vault_file(), safety_dir.join("workspace.twvault"))
            .map_err(|e| failed(e.to_string()))?;
        safety_backup = Some(safety_dir.to_string_lossy().into_owned());
    }

    let file = std::fs::File::open(archive_path).map_err(|e| failed(e.to_string()))?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|_| failed("the file is not a readable archive".to_owned()))?;

    extract_entry(&mut zip, DATABASE_ENTRY, &paths.database())?;
    let vault_restored = if manifest.includes_vault {
        std::fs::create_dir_all(paths.vault_dir()).map_err(|e| failed(e.to_string()))?;
        extract_entry(&mut zip, VAULT_ENTRY, &paths.vault_file())?;
        true
    } else {
        false
    };

    // A stale WAL alongside a replaced database would be applied on next open
    // and undo the restore.
    for suffix in ["-wal", "-shm"] {
        let mut name = paths.database().into_os_string();
        name.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(name));
    }

    Ok(RestoreOutcome {
        manifest,
        safety_backup,
        vault_restored,
    })
}

fn extract_entry<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
    destination: &Path,
) -> AppResult<()> {
    let failed = |detail: String| AppError::new(DiagnosticCode::RestoreFailed, detail);
    let mut entry = zip
        .by_name(name)
        .map_err(|_| failed(format!("the backup does not contain {name}")))?;
    if entry.size() > MAX_BACKUP_BYTES {
        return Err(failed(format!("{name} in the backup is implausibly large")));
    }
    // Write to a temporary file and rename, so an interrupted restore does not
    // leave a half-written database in place of a working one.
    let temp = destination.with_extension("restore-tmp");
    let mut out = std::fs::File::create(&temp).map_err(|e| failed(e.to_string()))?;
    std::io::copy(&mut entry, &mut out).map_err(|e| failed(e.to_string()))?;
    out.sync_all().map_err(|e| failed(e.to_string()))?;
    drop(out);
    std::fs::rename(&temp, destination).map_err(|e| failed(e.to_string()))?;
    Ok(())
}

/// Lists the backups in the workspace, newest first.
#[must_use]
pub fn list(paths: &WorkspacePaths) -> Vec<PathBuf> {
    let mut archives: Vec<PathBuf> = std::fs::read_dir(paths.backups_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")))
        .collect();
    archives.sort();
    archives.reverse();
    archives
}

#[cfg(test)]
mod tests {
    use tw_secrets::SecretString;
    use tw_storage::SettingsRepo;

    use super::*;
    use crate::bootstrap::Bootstrap;

    fn workspace(dir: &tempfile::TempDir) -> (WorkspacePaths, Database) {
        let bootstrap = Bootstrap::run_in(dir.path()).expect("bootstrap");
        let (paths, guard, db, _) = bootstrap.into_parts();
        // The guard is released here: these tests exercise backup, not locking.
        drop(guard);
        (paths, db)
    }

    #[test]
    fn a_backup_contains_the_database_the_vault_and_a_manifest() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (paths, db) = workspace(&dir);
        SettingsRepo::set_raw(db.connection(), "theme", "dark").expect("write setting");

        let mut vault = crate::vault::VaultService::new(paths.vault_file(), Default::default());
        vault
            .create(&SecretString::new("correct horse battery staple"))
            .expect("vault");

        let outcome = create(&paths, &db).expect("backup");
        assert!(outcome.manifest.includes_vault);
        assert_eq!(outcome.manifest.format_version, BACKUP_FORMAT_VERSION);
        assert_eq!(outcome.manifest.schema_version, tw_storage::SCHEMA_VERSION);
        assert!(outcome.bytes > 0);

        let archive = std::fs::File::open(&outcome.path).expect("open");
        let mut zip = zip::ZipArchive::new(archive).expect("read");
        let names: Vec<String> = (0..zip.len())
            .filter_map(|i| zip.by_index(i).ok().map(|e| e.name().to_owned()))
            .collect();
        assert!(names.contains(&MANIFEST_ENTRY.to_owned()));
        assert!(names.contains(&DATABASE_ENTRY.to_owned()));
        assert!(names.contains(&VAULT_ENTRY.to_owned()));
    }

    #[test]
    fn a_backup_never_contains_browser_user_data() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (paths, db) = workspace(&dir);
        let user_data = paths.profiles_dir().join("abc").join("User Data");
        std::fs::create_dir_all(&user_data).expect("mkdir");
        std::fs::write(user_data.join("Cookies"), vec![0u8; 4096]).expect("write");

        let outcome = create(&paths, &db).expect("backup");
        let archive = std::fs::File::open(&outcome.path).expect("open");
        let mut zip = zip::ZipArchive::new(archive).expect("read");
        for index in 0..zip.len() {
            let name = zip.by_index(index).expect("entry").name().to_owned();
            assert!(
                !name.contains("User Data"),
                "{name} must not be in a logical backup"
            );
            assert!(!name.contains("Cookies"), "{name}");
        }
    }

    #[test]
    fn the_vault_is_stored_still_encrypted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (paths, db) = workspace(&dir);
        let mut vault = crate::vault::VaultService::new(paths.vault_file(), Default::default());
        vault
            .create(&SecretString::new("correct horse battery staple"))
            .expect("vault");
        vault
            .store(tw_vault::SecretKind::Password, SecretString::new("hunter2"))
            .expect("store");

        let outcome = create(&paths, &db).expect("backup");
        let bytes = std::fs::read(&outcome.path).expect("read");
        assert!(
            !bytes.windows(7).any(|w| w == b"hunter2"),
            "the archive must not contain a plaintext secret"
        );
    }

    #[test]
    fn a_backup_round_trips_through_restore() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (paths, db) = workspace(&dir);
        SettingsRepo::set_raw(db.connection(), "theme", "dark").expect("write");
        let outcome = create(&paths, &db).expect("backup");

        // Change the workspace after the backup.
        SettingsRepo::set_raw(db.connection(), "theme", "light").expect("write");
        drop(db);

        let restored = restore(&paths, Path::new(&outcome.path)).expect("restore");
        assert_eq!(restored.manifest.created_at, outcome.manifest.created_at);
        assert!(
            restored.safety_backup.is_some(),
            "a restore must always keep a way back"
        );

        let reopened = Database::open(paths.database(), &Default::default()).expect("reopen");
        assert_eq!(
            SettingsRepo::get_raw(reopened.connection(), "theme")
                .expect("read")
                .as_deref(),
            Some("dark"),
            "the backup's contents should have replaced the newer ones"
        );
    }

    #[test]
    fn restoring_keeps_a_copy_of_what_it_replaced() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (paths, db) = workspace(&dir);
        SettingsRepo::set_raw(db.connection(), "theme", "dark").expect("write");
        let outcome = create(&paths, &db).expect("backup");
        SettingsRepo::set_raw(db.connection(), "theme", "light").expect("write");
        drop(db);

        let restored = restore(&paths, Path::new(&outcome.path)).expect("restore");
        let safety = PathBuf::from(restored.safety_backup.expect("safety copy"));
        let previous = Database::open(safety.join("workspace.db"), &Default::default()).expect("open");
        assert_eq!(
            SettingsRepo::get_raw(previous.connection(), "theme")
                .expect("read")
                .as_deref(),
            Some("light"),
            "the replaced database must still be recoverable"
        );
    }

    #[test]
    fn a_file_that_is_not_a_backup_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (paths, _db) = workspace(&dir);
        let bogus = dir.path().join("not-a-backup.zip");
        std::fs::write(&bogus, b"definitely not a zip archive").expect("write");

        let error = inspect(&bogus).expect_err("must refuse");
        assert_eq!(error.code, DiagnosticCode::RestoreFailed);
        assert!(restore(&paths, &bogus).is_err());
        assert!(
            paths.database().is_file(),
            "a refused restore must not touch the workspace"
        );
    }

    #[test]
    fn a_zip_without_a_manifest_is_not_treated_as_a_backup() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("other.zip");
        {
            let file = std::fs::File::create(&path).expect("create");
            let mut zip = zip::ZipWriter::new(file);
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
            zip.start_file("readme.txt", options).expect("start");
            zip.write_all(b"hello").expect("write");
            zip.finish().expect("finish");
        }
        let error = inspect(&path).expect_err("must refuse");
        assert!(
            error.message.contains("not a Thorium Workspace backup"),
            "{}",
            error.message
        );
    }

    #[test]
    fn backups_are_listed_newest_first() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (paths, db) = workspace(&dir);
        assert!(list(&paths).is_empty());

        std::fs::write(paths.backups_dir().join("thorium-workspace-backup-100.zip"), b"a").expect("write");
        std::fs::write(paths.backups_dir().join("thorium-workspace-backup-200.zip"), b"b").expect("write");
        std::fs::write(paths.backups_dir().join("notes.txt"), b"c").expect("write");
        let _ = create(&paths, &db);

        let listed = list(&paths);
        assert!(listed.iter().all(|p| p.extension().is_some_and(|e| e == "zip")));
        assert!(listed.len() >= 2);
        assert!(listed.windows(2).all(|w| w[0] >= w[1]), "newest first");
    }
}
