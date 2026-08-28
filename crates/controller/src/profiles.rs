//! Browser profile service.
//!
//! Owns the running sessions and reconciles the observed runtime table against
//! reality at startup.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tw_browser_profile::{BrowserSession, ProfileLayout, ProfileLock, SessionState};
use tw_domain::{
    BrowserProfile, BrowserProfileDraft, DiagnosticCode, ProfileId, ProfileRuntimeStatus, Timestamp,
};
use tw_storage::{Database, ProfileRepo, RuntimeRepo, RuntimeSession};

use crate::error::{AppError, AppResult};

/// Creates a profile.
///
/// # Errors
///
/// Returns [`DiagnosticCode::InvalidInput`] or a storage error.
pub fn create_profile(db: &mut Database, draft: &BrowserProfileDraft) -> AppResult<BrowserProfile> {
    let normalized = draft.normalize()?;
    let now = Timestamp::now();
    let profile = BrowserProfile {
        id: ProfileId::new(),
        name: normalized.name,
        thorium: normalized.thorium,
        startup_urls: normalized.startup_urls,
        locale: normalized.locale,
        timezone: normalized.timezone,
        account_ids: normalized.account_ids.clone(),
        notes: normalized.notes,
        network_route_id: None,
        created_at: now,
        updated_at: now,
    };
    ProfileRepo::insert(db.connection(), &profile)?;
    ProfileRepo::set_accounts(db.connection_mut(), profile.id, &normalized.account_ids)?;
    Ok(profile)
}

/// Updates a profile's configuration.
///
/// # Errors
///
/// Returns [`DiagnosticCode::InvalidInput`] or a storage error.
pub fn update_profile(
    db: &mut Database,
    id: ProfileId,
    draft: &BrowserProfileDraft,
) -> AppResult<BrowserProfile> {
    let existing = ProfileRepo::get(db.connection(), id)?;
    let normalized = draft.normalize()?;
    let updated = BrowserProfile {
        name: normalized.name,
        thorium: normalized.thorium,
        startup_urls: normalized.startup_urls,
        locale: normalized.locale,
        timezone: normalized.timezone,
        account_ids: normalized.account_ids.clone(),
        notes: normalized.notes,
        updated_at: Timestamp::now(),
        ..existing
    };
    ProfileRepo::update(db.connection(), &updated)?;
    ProfileRepo::set_accounts(db.connection_mut(), id, &normalized.account_ids)?;
    Ok(updated)
}

/// Deletes a profile.
///
/// `delete_browser_data` is an explicit choice: a `User Data` directory can hold
/// years of browsing state and gigabytes of cache, so it is never removed
/// implicitly.
///
/// # Errors
///
/// Returns [`DiagnosticCode::ProfileAlreadyRunning`] when a session is live, and
/// a storage error otherwise.
pub fn delete_profile(
    db: &mut Database,
    workspace_root: &Path,
    id: ProfileId,
    delete_browser_data: bool,
) -> AppResult<Option<PathBuf>> {
    let profile = ProfileRepo::get(db.connection(), id)?;
    let layout = ProfileLayout::new(workspace_root, &profile);
    if ProfileLock::is_locked(&layout.profile_dir) {
        return Err(AppError::new(
            DiagnosticCode::ProfileAlreadyRunning,
            "this profile is running and cannot be deleted",
        )
        .with_remedy("Stop the profile first."));
    }

    ProfileRepo::delete(db.connection(), id)?;

    if delete_browser_data && layout.profile_dir.is_dir() {
        std::fs::remove_dir_all(&layout.profile_dir).map_err(|e| {
            AppError::new(
                DiagnosticCode::IoFailed,
                format!("the profile's browser data could not be removed: {e}"),
            )
        })?;
        return Ok(Some(layout.profile_dir));
    }
    Ok(None)
}

/// Which Thorium version a profile should launch.
///
/// # Errors
///
/// Returns [`DiagnosticCode::ThoriumNotInstalled`] when nothing usable is
/// selected, and [`DiagnosticCode::ThoriumVersionMissing`] when a pinned version
/// is gone.
pub fn resolve_version(
    manager: &tw_thorium::ThoriumManager,
    profile: &BrowserProfile,
) -> AppResult<(String, PathBuf)> {
    match profile.thorium.pinned_version() {
        Some(version) => {
            let executable = manager.executable_for(version).map_err(|_| {
                AppError::new(
                    DiagnosticCode::ThoriumVersionMissing,
                    format!("this profile is pinned to Thorium {version}, which is not installed"),
                )
                .with_remedy("Install that version, or change the profile to follow Current.")
            })?;
            Ok((version.to_owned(), executable))
        }
        None => {
            let version = manager.current_version().ok_or_else(|| {
                AppError::new(
                    DiagnosticCode::ThoriumNotInstalled,
                    "no Thorium version is installed",
                )
                .with_remedy("Install Thorium from the Browser page first.")
            })?;
            let executable = manager.executable_for(&version)?;
            Ok((version, executable))
        }
    }
}

/// The running browser sessions.
#[derive(Debug, Default)]
pub struct SessionRegistry {
    sessions: HashMap<ProfileId, BrowserSession>,
}

impl SessionRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether a session is registered for `id`.
    #[must_use]
    pub fn contains(&self, id: ProfileId) -> bool {
        self.sessions.contains_key(&id)
    }

    /// The state of a registered session.
    #[must_use]
    pub fn state(&self, id: ProfileId) -> Option<SessionState> {
        self.sessions.get(&id).map(BrowserSession::state)
    }

    /// Every registered profile id.
    #[must_use]
    pub fn ids(&self) -> Vec<ProfileId> {
        self.sessions.keys().copied().collect()
    }

    /// Registers a session.
    pub fn insert(&mut self, id: ProfileId, session: BrowserSession) {
        self.sessions.insert(id, session);
    }

    /// Removes and returns a session.
    pub fn take(&mut self, id: ProfileId) -> Option<BrowserSession> {
        self.sessions.remove(&id)
    }

    /// Brings a running session's window to the front.
    ///
    /// Returns `Some(true)` when a window was raised, `Some(false)` when the
    /// session is running but no window could be raised, and `None` when there
    /// is no session.
    #[must_use]
    pub fn focus(&self, id: ProfileId) -> Option<bool> {
        self.sessions.get(&id).map(BrowserSession::focus)
    }

    /// Drops sessions whose browser has exited on its own.
    ///
    /// Returns the ids that were dropped.
    pub fn reap_exited(&mut self) -> Vec<ProfileId> {
        let dead: Vec<ProfileId> = self
            .sessions
            .iter()
            .filter(|(_, s)| !s.is_alive())
            .map(|(id, _)| *id)
            .collect();
        for id in &dead {
            self.sessions.remove(id);
        }
        dead
    }

    /// Stops every session. Used at shutdown.
    pub async fn stop_all(&mut self) {
        let ids: Vec<ProfileId> = self.sessions.keys().copied().collect();
        for id in ids {
            if let Some(session) = self.sessions.remove(&id)
                && let Err(error) = session.stop().await
            {
                tracing::warn!(code = %error.code(), "a browser session did not stop cleanly");
            }
        }
    }
}

/// Records a session in the observed runtime table.
///
/// # Errors
///
/// Returns a storage error.
pub fn record_session(db: &Database, id: ProfileId, state: &SessionState) -> AppResult<()> {
    RuntimeRepo::upsert(
        db.connection(),
        &RuntimeSession {
            profile_id: id,
            status: state.status,
            pid: state.pid,
            cdp_port: state.cdp_port,
            thorium_version: state.thorium_version.clone(),
            started_at: Some(Timestamp::now()),
            updated_at: Timestamp::now(),
        },
    )?;
    Ok(())
}

/// Clears a profile's observed runtime record.
///
/// # Errors
///
/// Returns a storage error.
pub fn clear_session(db: &Database, id: ProfileId) -> AppResult<()> {
    RuntimeRepo::clear(db.connection(), id)?;
    Ok(())
}

/// What startup recovery found.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryReport {
    /// Runtime rows left behind by a previous run.
    pub stale_sessions_cleared: usize,
    /// Profile lock files that were not actually held.
    pub stale_locks_found: usize,
}

/// Reconciles observed runtime state with reality at startup.
///
/// Every row in the runtime table describes a process supervised by a manager
/// that is no longer running, so all of them are cleared. Lock files are
/// inspected but never deleted: the operating system releases the lock when the
/// holder dies, and a file that is still locked belongs to a live process.
///
/// # Errors
///
/// Returns a storage error.
pub fn recover_runtime_state(db: &Database, workspace_root: &Path) -> AppResult<RecoveryReport> {
    let stale_sessions_cleared = RuntimeRepo::clear_all(db.connection())?;

    let mut stale_locks_found = 0usize;
    for profile in ProfileRepo::list(db.connection())? {
        let layout = ProfileLayout::new(workspace_root, &profile);
        let lock_file = layout.profile_dir.join(ProfileLock::FILE_NAME);
        if lock_file.is_file() && !ProfileLock::is_locked(&layout.profile_dir) {
            stale_locks_found += 1;
        }
    }
    Ok(RecoveryReport {
        stale_sessions_cleared,
        stale_locks_found,
    })
}

/// The observed status of a profile, from live sessions and the lock file.
#[must_use]
pub fn observed_status(
    registry: &SessionRegistry,
    workspace_root: &Path,
    profile: &BrowserProfile,
) -> ProfileRuntimeStatus {
    if let Some(state) = registry.state(profile.id) {
        return state.status;
    }
    let layout = ProfileLayout::new(workspace_root, profile);
    if ProfileLock::is_locked(&layout.profile_dir) {
        // Locked but not ours: another instance of the manager has it. That
        // should be impossible while the workspace mutex is held, so it is
        // reported rather than assumed away.
        return ProfileRuntimeStatus::Running;
    }
    ProfileRuntimeStatus::Stopped
}

#[cfg(test)]
mod tests {
    use tw_domain::{LocaleTag, ThoriumSelection, TimeZoneId};

    use super::*;

    fn draft(name: &str) -> BrowserProfileDraft {
        BrowserProfileDraft {
            name: name.to_owned(),
            ..Default::default()
        }
    }

    #[test]
    fn profiles_are_created_with_independent_directories() {
        let mut db = Database::open_in_memory().expect("db");
        let a = create_profile(&mut db, &draft("First")).expect("create");
        let b = create_profile(&mut db, &draft("Second")).expect("create");
        assert_ne!(a.user_data_dir_name(), b.user_data_dir_name());
        assert_eq!(ProfileRepo::list(db.connection()).expect("list").len(), 2);
    }

    #[test]
    fn a_profile_can_be_updated_without_changing_its_directory() {
        let mut db = Database::open_in_memory().expect("db");
        let profile = create_profile(&mut db, &draft("Before")).expect("create");
        let before_dir = profile.user_data_dir_name();

        let updated = update_profile(
            &mut db,
            profile.id,
            &BrowserProfileDraft {
                name: "After".to_owned(),
                locale: Some("pl-PL".to_owned()),
                timezone: Some("Europe/Warsaw".to_owned()),
                startup_urls: vec!["https://example.test/".to_owned()],
                ..Default::default()
            },
        )
        .expect("update");

        assert_eq!(updated.name, "After");
        assert_eq!(updated.user_data_dir_name(), before_dir);
        assert_eq!(updated.locale, LocaleTag::parse("pl-PL").expect("locale"));
        assert_eq!(
            updated.timezone,
            TimeZoneId::parse("Europe/Warsaw").expect("timezone")
        );
    }

    #[test]
    fn deleting_a_profile_keeps_its_browser_data_unless_asked() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut db = Database::open_in_memory().expect("db");
        let profile = create_profile(&mut db, &draft("Work")).expect("create");
        let layout = ProfileLayout::new(dir.path(), &profile);
        layout.ensure().expect("create dirs");
        std::fs::write(layout.user_data_dir.join("Preferences"), b"{}").expect("write");

        assert_eq!(
            delete_profile(&mut db, dir.path(), profile.id, false).expect("delete"),
            None
        );
        assert!(
            layout.user_data_dir.join("Preferences").is_file(),
            "browser data must survive by default"
        );
    }

    #[test]
    fn deleting_a_profile_can_remove_its_browser_data_explicitly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut db = Database::open_in_memory().expect("db");
        let profile = create_profile(&mut db, &draft("Work")).expect("create");
        let layout = ProfileLayout::new(dir.path(), &profile);
        layout.ensure().expect("create dirs");

        let removed = delete_profile(&mut db, dir.path(), profile.id, true).expect("delete");
        assert_eq!(removed.as_deref(), Some(layout.profile_dir.as_path()));
        assert!(!layout.profile_dir.exists());
    }

    #[test]
    fn a_running_profile_cannot_be_deleted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut db = Database::open_in_memory().expect("db");
        let profile = create_profile(&mut db, &draft("Work")).expect("create");
        let layout = ProfileLayout::new(dir.path(), &profile);
        let _lock = ProfileLock::acquire(&layout.profile_dir).expect("lock");

        let error = delete_profile(&mut db, dir.path(), profile.id, false).expect_err("must refuse");
        assert_eq!(error.code, DiagnosticCode::ProfileAlreadyRunning);
        assert!(
            ProfileRepo::get(db.connection(), profile.id).is_ok(),
            "the profile must survive"
        );
    }

    #[test]
    fn startup_recovery_clears_stale_runtime_rows_and_notices_stale_locks() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut db = Database::open_in_memory().expect("db");
        let profile = create_profile(&mut db, &draft("Work")).expect("create");

        RuntimeRepo::upsert(
            db.connection(),
            &RuntimeSession {
                profile_id: profile.id,
                status: ProfileRuntimeStatus::Running,
                pid: Some(999_999),
                cdp_port: Some(51234),
                thorium_version: Some("M152".to_owned()),
                started_at: Some(Timestamp::from_unix_seconds(1)),
                updated_at: Timestamp::from_unix_seconds(1),
            },
        )
        .expect("stale row");

        // A lock file a crash left behind, with nothing holding it.
        let layout = ProfileLayout::new(dir.path(), &profile);
        std::fs::create_dir_all(&layout.profile_dir).expect("mkdir");
        std::fs::write(layout.profile_dir.join(ProfileLock::FILE_NAME), b"{}").expect("write");

        let report = recover_runtime_state(&db, dir.path()).expect("recover");
        assert_eq!(report.stale_sessions_cleared, 1);
        assert_eq!(report.stale_locks_found, 1);
        assert!(RuntimeRepo::list(db.connection()).expect("list").is_empty());
        assert!(
            layout.profile_dir.join(ProfileLock::FILE_NAME).is_file(),
            "recovery must not delete files it does not own the lifetime of"
        );
    }

    #[test]
    fn observed_status_prefers_a_live_session_then_the_lock() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut db = Database::open_in_memory().expect("db");
        let profile = create_profile(&mut db, &draft("Work")).expect("create");
        let registry = SessionRegistry::new();

        assert_eq!(
            observed_status(&registry, dir.path(), &profile),
            ProfileRuntimeStatus::Stopped
        );

        let layout = ProfileLayout::new(dir.path(), &profile);
        let lock = ProfileLock::acquire(&layout.profile_dir).expect("lock");
        assert_eq!(
            observed_status(&registry, dir.path(), &profile),
            ProfileRuntimeStatus::Running
        );
        drop(lock);
        assert_eq!(
            observed_status(&registry, dir.path(), &profile),
            ProfileRuntimeStatus::Stopped
        );
    }

    #[test]
    fn version_resolution_reports_a_missing_pin_distinctly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = tw_thorium::ThoriumManager::new(dir.path(), tw_thorium::ReleaseClientConfig::default())
            .expect("manager");

        let mut db = Database::open_in_memory().expect("db");
        let mut profile = create_profile(&mut db, &draft("Work")).expect("create");

        let error = resolve_version(&manager, &profile).expect_err("nothing installed");
        assert_eq!(error.code, DiagnosticCode::ThoriumNotInstalled);
        assert!(error.remedy.is_some());

        profile.thorium = ThoriumSelection::Pinned("M999".to_owned());
        let error = resolve_version(&manager, &profile).expect_err("pin missing");
        assert_eq!(error.code, DiagnosticCode::ThoriumVersionMissing);
        assert!(error.message.contains("M999"), "{}", error.message);
    }

    #[test]
    fn version_resolution_follows_current_and_an_installed_pin() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = tw_thorium::ThoriumManager::new(dir.path(), tw_thorium::ReleaseClientConfig::default())
            .expect("manager");
        for version in ["M151", "M152"] {
            let bin = manager.paths().version_dir(version).join("BIN");
            std::fs::create_dir_all(&bin).expect("mkdir");
            std::fs::write(bin.join("thorium.exe"), vec![0u8; 300 * 1024]).expect("write");
        }
        manager.set_current("M152").expect("select");

        let mut db = Database::open_in_memory().expect("db");
        let mut profile = create_profile(&mut db, &draft("Work")).expect("create");
        let (version, executable) = resolve_version(&manager, &profile).expect("resolve");
        assert_eq!(version, "M152");
        assert!(executable.is_file());

        profile.thorium = ThoriumSelection::Pinned("M151".to_owned());
        assert_eq!(resolve_version(&manager, &profile).expect("resolve").0, "M151");
    }

    #[test]
    fn an_empty_registry_reports_nothing_running() {
        let registry = SessionRegistry::new();
        assert!(registry.ids().is_empty());
        assert!(!registry.contains(ProfileId::new()));
        assert_eq!(registry.focus(ProfileId::new()), None);
        assert_eq!(registry.state(ProfileId::new()), None);
    }
}
