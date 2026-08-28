//! Thorium version service.
//!
//! Keeps the installation records in the database consistent with what is
//! actually on disk, and refuses to remove a version anything still needs.

use std::collections::BTreeSet;

use tw_domain::{DiagnosticCode, ThoriumChannel, ThoriumInstallation, ThoriumRelease};
use tw_storage::{Database, ProfileRepo, RuntimeRepo, ThoriumRepo};
use tw_thorium::{InstallProgress, InstallRequest, ThoriumManager};

use crate::error::{AppError, AppResult};

/// A version as the UI sees it: the record, plus why it may not be removable.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledVersion {
    /// The version string.
    pub version: String,
    /// Which channel it came from.
    pub channel: ThoriumChannel,
    /// Absolute path to `thorium.exe`.
    pub executable_path: String,
    /// When it was installed, in Unix epoch seconds.
    pub installed_at: i64,
    /// Lowercase hex SHA-256 of the archive it came from.
    pub archive_sha256: String,
    /// Whether it is the selected version.
    pub is_current: bool,
    /// How many profiles are pinned to it.
    pub pinned_by_profiles: usize,
    /// Whether a running session is using it.
    pub in_use: bool,
    /// Whether the files are actually present.
    pub present_on_disk: bool,
}

/// Reconciles the installation table with the versions actually on disk.
///
/// The filesystem is the source of truth: a version deleted by hand should
/// disappear from the UI, and one restored by hand should reappear.
///
/// # Errors
///
/// Returns a storage error.
pub fn reconcile(db: &mut Database, manager: &ThoriumManager) -> AppResult<usize> {
    let on_disk: BTreeSet<String> = manager.installed_versions().into_iter().collect();
    let recorded = ThoriumRepo::list(db.connection())?;

    let mut changes = 0usize;
    for record in &recorded {
        if !on_disk.contains(&record.version) {
            ThoriumRepo::delete(db.connection(), &record.version)?;
            changes += 1;
        }
    }

    let recorded_versions: BTreeSet<String> = recorded.iter().map(|r| r.version.clone()).collect();
    for version in &on_disk {
        if recorded_versions.contains(version) {
            continue;
        }
        // A version present on disk with no record: adopt it rather than
        // ignoring it, so a restored folder becomes usable again.
        let Ok(executable) = manager.executable_for(version) else {
            continue;
        };
        ThoriumRepo::upsert(
            db.connection(),
            &ThoriumInstallation {
                version: version.clone(),
                channel: ThoriumChannel::default(),
                install_dir: manager
                    .paths()
                    .version_dir(version)
                    .to_string_lossy()
                    .into_owned(),
                executable_path: executable.to_string_lossy().into_owned(),
                installed_at: tw_domain::Timestamp::now(),
                source_url: String::new(),
                archive_sha256: String::new(),
                is_current: false,
            },
        )?;
        changes += 1;
    }

    // The marker file decides which version is current, so the table follows it.
    match manager.current_version() {
        Some(current) => {
            let already = ThoriumRepo::current(db.connection())?.map(|i| i.version);
            if already.as_deref() != Some(current.as_str()) {
                ThoriumRepo::set_current(db.connection_mut(), &current)?;
                changes += 1;
            }
        }
        None => {
            if let Some(stale) = ThoriumRepo::current(db.connection())? {
                let mut cleared = stale;
                cleared.is_current = false;
                ThoriumRepo::upsert(db.connection(), &cleared)?;
                changes += 1;
            }
        }
    }
    Ok(changes)
}

/// Lists installed versions with everything the UI needs to decide what is
/// safe to remove.
///
/// # Errors
///
/// Returns a storage error.
pub fn list(db: &Database, manager: &ThoriumManager) -> AppResult<Vec<InstalledVersion>> {
    let in_use: BTreeSet<String> = RuntimeRepo::versions_in_use(db.connection())?
        .into_iter()
        .collect();
    let on_disk: BTreeSet<String> = manager.installed_versions().into_iter().collect();
    let current = manager.current_version();

    let mut out = Vec::new();
    for record in ThoriumRepo::list(db.connection())? {
        let pinned = ProfileRepo::pinned_to_version(db.connection(), &record.version)?.len();
        out.push(InstalledVersion {
            is_current: current.as_deref() == Some(record.version.as_str()),
            pinned_by_profiles: pinned,
            in_use: in_use.contains(&record.version),
            present_on_disk: on_disk.contains(&record.version),
            version: record.version,
            channel: record.channel,
            executable_path: record.executable_path,
            installed_at: record.installed_at.as_unix_seconds(),
            archive_sha256: record.archive_sha256,
        });
    }
    Ok(out)
}

/// Installs a version and records it.
///
/// # Errors
///
/// See [`tw_thorium::ThoriumError`], plus storage errors.
pub async fn install(
    db: &mut Database,
    manager: &ThoriumManager,
    request: &InstallRequest,
    on_progress: impl FnMut(InstallProgress),
) -> AppResult<ThoriumInstallation> {
    let installation = manager.install(request, on_progress).await?;
    ThoriumRepo::upsert(db.connection(), &installation)?;
    if installation.is_current {
        ThoriumRepo::set_current(db.connection_mut(), &installation.version)?;
    }
    Ok(installation)
}

/// Selects a version as current.
///
/// # Errors
///
/// Returns [`DiagnosticCode::ThoriumVersionMissing`] or a storage error.
pub fn set_current(db: &mut Database, manager: &ThoriumManager, version: &str) -> AppResult<()> {
    manager.set_current(version)?;
    ThoriumRepo::set_current(db.connection_mut(), version)?;
    Ok(())
}

/// Removes a version.
///
/// Refuses while a running session uses it, or while a profile is pinned to it:
/// a pinned profile would otherwise fail to launch with no explanation.
///
/// # Errors
///
/// Returns [`DiagnosticCode::ThoriumVersionInUse`],
/// [`DiagnosticCode::ThoriumVersionMissing`] or a storage error.
pub fn remove(db: &mut Database, manager: &ThoriumManager, version: &str) -> AppResult<()> {
    let in_use = RuntimeRepo::versions_in_use(db.connection())?
        .iter()
        .any(|v| v == version);
    if in_use {
        return Err(AppError::new(
            DiagnosticCode::ThoriumVersionInUse,
            format!("Thorium {version} is in use by a running profile"),
        )
        .with_remedy("Stop that profile first."));
    }

    let pinned = ProfileRepo::pinned_to_version(db.connection(), version)?;
    if !pinned.is_empty() {
        return Err(AppError::new(
            DiagnosticCode::ThoriumVersionInUse,
            format!("{} profile(s) are pinned to Thorium {version}", pinned.len()),
        )
        .with_remedy("Change those profiles to follow Current, or pin them to another version."));
    }

    manager.remove_version(version, 0)?;
    // The record is removed after the files, so a failed delete leaves a
    // version that is still listed rather than one that has silently vanished.
    if ThoriumRepo::get(db.connection(), version).is_ok() {
        ThoriumRepo::delete(db.connection(), version)?;
    }
    reconcile(db, manager)?;
    Ok(())
}

/// Reverts to the previous installed version.
///
/// # Errors
///
/// Returns [`DiagnosticCode::ThoriumVersionMissing`] when there is nothing to
/// revert to.
pub fn rollback(db: &mut Database, manager: &ThoriumManager) -> AppResult<String> {
    let version = manager.rollback()?;
    ThoriumRepo::set_current(db.connection_mut(), &version)?;
    Ok(version)
}

/// Looks up the newest installable release without installing it.
///
/// # Errors
///
/// See [`tw_thorium::ThoriumError`].
pub async fn check_for_update(
    manager: &ThoriumManager,
    channel: ThoriumChannel,
) -> AppResult<ThoriumRelease> {
    Ok(manager.releases().latest_installable(channel).await?)
}

#[cfg(test)]
mod tests {
    use tw_domain::{BrowserProfileDraft, ProfileRuntimeStatus, ThoriumSelection, Timestamp};
    use tw_storage::RuntimeSession;

    use super::*;

    fn manager(dir: &tempfile::TempDir) -> ThoriumManager {
        ThoriumManager::new(dir.path(), tw_thorium::ReleaseClientConfig::default()).expect("manager")
    }

    fn fake_install(manager: &ThoriumManager, version: &str) {
        let bin = manager.paths().version_dir(version).join("BIN");
        std::fs::create_dir_all(&bin).expect("mkdir");
        std::fs::write(bin.join("thorium.exe"), vec![0u8; 300 * 1024]).expect("write");
    }

    #[test]
    fn reconciliation_adopts_versions_found_on_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        let mut db = Database::open_in_memory().expect("db");

        fake_install(&manager, "M152");
        manager.set_current("M152").expect("select");

        assert_eq!(reconcile(&mut db, &manager).expect("reconcile"), 2);
        let listed = list(&db, &manager).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].version, "M152");
        assert!(listed[0].is_current);
        assert!(listed[0].present_on_disk);
    }

    #[test]
    fn reconciliation_drops_records_whose_files_are_gone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        let mut db = Database::open_in_memory().expect("db");

        fake_install(&manager, "M152");
        reconcile(&mut db, &manager).expect("first");
        assert_eq!(list(&db, &manager).expect("list").len(), 1);

        std::fs::remove_dir_all(manager.paths().version_dir("M152")).expect("delete by hand");
        reconcile(&mut db, &manager).expect("second");
        assert!(list(&db, &manager).expect("list").is_empty());
    }

    #[test]
    fn reconciliation_follows_the_on_disk_selection() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        let mut db = Database::open_in_memory().expect("db");

        fake_install(&manager, "M151");
        fake_install(&manager, "M152");
        manager.set_current("M152").expect("select");
        reconcile(&mut db, &manager).expect("reconcile");
        assert_eq!(
            ThoriumRepo::current(db.connection())
                .expect("current")
                .map(|i| i.version),
            Some("M152".to_owned())
        );

        // Something changed the marker outside the app.
        manager.set_current("M151").expect("select");
        reconcile(&mut db, &manager).expect("reconcile");
        assert_eq!(
            ThoriumRepo::current(db.connection())
                .expect("current")
                .map(|i| i.version),
            Some("M151".to_owned())
        );
    }

    #[test]
    fn a_version_a_profile_is_pinned_to_cannot_be_removed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        let mut db = Database::open_in_memory().expect("db");
        fake_install(&manager, "M151");
        fake_install(&manager, "M152");
        reconcile(&mut db, &manager).expect("reconcile");

        crate::profiles::create_profile(
            &mut db,
            &BrowserProfileDraft {
                name: "Pinned".to_owned(),
                thorium: ThoriumSelection::Pinned("M151".to_owned()),
                ..Default::default()
            },
        )
        .expect("profile");

        let error = remove(&mut db, &manager, "M151").expect_err("must refuse");
        assert_eq!(error.code, DiagnosticCode::ThoriumVersionInUse);
        assert!(error.message.contains("pinned"), "{}", error.message);
        assert!(
            manager.paths().version_dir("M151").is_dir(),
            "the files must survive"
        );
    }

    #[test]
    fn a_version_a_running_profile_uses_cannot_be_removed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        let mut db = Database::open_in_memory().expect("db");
        fake_install(&manager, "M152");
        reconcile(&mut db, &manager).expect("reconcile");

        let profile = crate::profiles::create_profile(
            &mut db,
            &BrowserProfileDraft {
                name: "Work".to_owned(),
                ..Default::default()
            },
        )
        .expect("profile");
        RuntimeRepo::upsert(
            db.connection(),
            &RuntimeSession {
                profile_id: profile.id,
                status: ProfileRuntimeStatus::Running,
                pid: Some(1),
                cdp_port: None,
                thorium_version: Some("M152".to_owned()),
                started_at: Some(Timestamp::now()),
                updated_at: Timestamp::now(),
            },
        )
        .expect("session");

        let error = remove(&mut db, &manager, "M152").expect_err("must refuse");
        assert_eq!(error.code, DiagnosticCode::ThoriumVersionInUse);
        assert!(error.remedy.is_some());
    }

    #[test]
    fn an_unused_version_can_be_removed_and_the_record_follows() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        let mut db = Database::open_in_memory().expect("db");
        fake_install(&manager, "M151");
        fake_install(&manager, "M152");
        manager.set_current("M152").expect("select");
        reconcile(&mut db, &manager).expect("reconcile");

        remove(&mut db, &manager, "M151").expect("remove");
        let listed = list(&db, &manager).expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].version, "M152");
        assert!(!manager.paths().version_dir("M151").exists());
    }

    #[test]
    fn rollback_switches_the_selection_in_both_places() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        let mut db = Database::open_in_memory().expect("db");
        fake_install(&manager, "M151");
        fake_install(&manager, "M152");
        manager.set_current("M152").expect("select");
        reconcile(&mut db, &manager).expect("reconcile");

        assert_eq!(rollback(&mut db, &manager).expect("rollback"), "M151");
        assert_eq!(manager.current_version().as_deref(), Some("M151"));
        assert_eq!(
            ThoriumRepo::current(db.connection())
                .expect("current")
                .map(|i| i.version),
            Some("M151".to_owned())
        );
    }

    #[test]
    fn the_listing_reports_why_a_version_is_not_removable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        let mut db = Database::open_in_memory().expect("db");
        fake_install(&manager, "M152");
        reconcile(&mut db, &manager).expect("reconcile");

        crate::profiles::create_profile(
            &mut db,
            &BrowserProfileDraft {
                name: "Pinned".to_owned(),
                thorium: ThoriumSelection::Pinned("M152".to_owned()),
                ..Default::default()
            },
        )
        .expect("profile");

        let listed = list(&db, &manager).expect("list");
        assert_eq!(listed[0].pinned_by_profiles, 1);
        assert!(!listed[0].in_use);
    }
}
