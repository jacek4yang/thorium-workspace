//! Vault service.
//!
//! Wraps [`tw_vault::VaultSession`] with the workspace's settings: idle timeout,
//! lock-on-minimize and the orphan collection that runs after deletions.

use std::collections::BTreeSet;

use tw_domain::{DiagnosticCode, SecretRef, VaultSettings};
use tw_secrets::SecretString;
use tw_vault::{KdfParameters, LockReason, SecretKind, VaultSession, VaultState};

use crate::error::{AppError, AppResult};

/// The vault, plus the settings that govern when it locks.
#[derive(Debug)]
pub struct VaultService {
    session: VaultSession,
    settings: VaultSettings,
}

impl VaultService {
    /// Builds a service over the vault file at `path`.
    #[must_use]
    pub fn new(path: impl Into<std::path::PathBuf>, settings: VaultSettings) -> Self {
        Self {
            session: VaultSession::new(path, KdfParameters::default()),
            settings,
        }
    }

    /// Replaces the locking settings.
    pub fn set_settings(&mut self, settings: VaultSettings) {
        self.settings = settings;
    }

    /// The current settings.
    #[must_use]
    pub const fn settings(&self) -> VaultSettings {
        self.settings
    }

    /// The state the frontend is allowed to see.
    #[must_use]
    pub fn state(&self) -> VaultState {
        self.session.state(
            self.settings
                .idle_lock_enabled
                .then_some(self.settings.idle_lock_seconds),
        )
    }

    /// Whether the vault is unlocked.
    #[must_use]
    pub fn is_unlocked(&self) -> bool {
        self.session.is_unlocked()
    }

    /// Whether a vault file exists.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.session.exists()
    }

    /// Creates the vault.
    ///
    /// # Errors
    ///
    /// See [`tw_vault::VaultStore::create`].
    pub fn create(&mut self, password: &SecretString) -> AppResult<VaultState> {
        self.session.create(password)?;
        Ok(self.state())
    }

    /// Unlocks the vault.
    ///
    /// # Errors
    ///
    /// See [`tw_vault::VaultStore::open`].
    pub fn unlock(&mut self, password: &SecretString) -> AppResult<VaultState> {
        self.session.unlock(password)?;
        Ok(self.state())
    }

    /// Locks the vault.
    pub fn lock(&mut self, reason: LockReason) -> VaultState {
        self.session.lock(reason);
        self.state()
    }

    /// Locks the vault if the idle timeout has elapsed and idle locking is on.
    ///
    /// Returns `true` when this call performed the lock.
    pub fn lock_if_idle(&mut self) -> bool {
        if !self.settings.idle_lock_enabled {
            return false;
        }
        self.session.lock_if_idle(self.settings.idle_lock_seconds)
    }

    /// Locks the vault when the window is minimised, if configured to.
    ///
    /// Returns `true` when this call performed the lock.
    pub fn lock_on_minimize(&mut self) -> bool {
        if !self.settings.lock_on_minimize || !self.session.is_unlocked() {
            return false;
        }
        self.session.lock(LockReason::Minimized);
        true
    }

    /// Records user activity so the idle timer restarts.
    pub fn touch(&mut self) {
        self.session.touch();
    }

    /// Seconds since the last recorded activity, or `None` when locked.
    #[must_use]
    pub fn idle_seconds(&self) -> Option<u64> {
        self.session.idle_seconds()
    }

    /// Stores a secret.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::VaultLocked`] when the vault is locked.
    pub fn store(&mut self, kind: SecretKind, value: SecretString) -> AppResult<SecretRef> {
        Ok(self.session.store_secret(kind, value)?)
    }

    /// Replaces a secret.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::VaultLocked`] or
    /// [`DiagnosticCode::SecretNotFound`].
    pub fn replace(&mut self, reference: SecretRef, value: SecretString) -> AppResult<()> {
        Ok(self.session.replace_secret(reference, value)?)
    }

    /// Reveals a secret. Every path that exposes plaintext goes through here.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::VaultLocked`] or
    /// [`DiagnosticCode::SecretNotFound`].
    pub fn reveal(&mut self, reference: SecretRef) -> AppResult<SecretString> {
        Ok(self.session.reveal(reference)?)
    }

    /// Removes a secret.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::VaultLocked`].
    pub fn forget(&mut self, reference: SecretRef) -> AppResult<()> {
        Ok(self.session.forget_secret(reference)?)
    }

    /// Drops secrets no longer referenced by any metadata record.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::VaultLocked`].
    pub fn collect_orphans(&mut self, live: &BTreeSet<SecretRef>) -> AppResult<usize> {
        Ok(self.session.collect_orphans(live)?)
    }

    /// Changes the master password.
    ///
    /// # Errors
    ///
    /// See [`tw_vault::VaultStore::change_password`].
    pub fn change_password(&mut self, current: &SecretString, next: &SecretString) -> AppResult<()> {
        self.session.change_password(current, next)?;
        Ok(())
    }

    /// Copies the vault file to `<name>.bak`.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::IoFailed`] when the copy fails.
    pub fn backup(&self) -> AppResult<Option<std::path::PathBuf>> {
        Ok(self.session.backup()?)
    }

    /// Counts of stored secrets by kind, for diagnostics.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::VaultLocked`].
    pub fn counts_by_kind(&self) -> AppResult<std::collections::BTreeMap<SecretKind, usize>> {
        Ok(self.session.counts_by_kind()?)
    }

    /// The vault file's header, readable while locked.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::VaultMissing`] or
    /// [`DiagnosticCode::VaultCorrupt`].
    pub fn peek_header(&self) -> AppResult<tw_vault::VaultHeader> {
        Ok(self.session.peek_header()?)
    }

    /// Returns an error unless the vault is unlocked.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::VaultLocked`].
    pub fn require_unlocked(&self) -> AppResult<()> {
        if self.session.is_unlocked() {
            Ok(())
        } else {
            Err(AppError::new(DiagnosticCode::VaultLocked, "the vault is locked")
                .with_remedy("Unlock the vault to work with account secrets."))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service(dir: &tempfile::TempDir, settings: VaultSettings) -> VaultService {
        // Production KDF cost is deliberately expensive; these tests exercise
        // policy, not key derivation, so they use one vault and reuse it.
        VaultService::new(dir.path().join("workspace.twvault"), settings)
    }

    fn password() -> SecretString {
        SecretString::new("correct horse battery staple")
    }

    #[test]
    fn a_locked_vault_refuses_secret_work_with_a_remedy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service = service(&dir, VaultSettings::default());
        let error = service.require_unlocked().expect_err("locked");
        assert_eq!(error.code, DiagnosticCode::VaultLocked);
        assert!(error.remedy.is_some());
    }

    #[test]
    fn idle_locking_honours_the_setting() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut service = service(
            &dir,
            VaultSettings {
                idle_lock_enabled: false,
                idle_lock_seconds: 30,
                lock_on_minimize: false,
            },
        );
        service.create(&password()).expect("create");
        assert!(service.is_unlocked());
        assert!(!service.lock_if_idle(), "idle locking is disabled");
        assert!(service.is_unlocked());

        service.set_settings(VaultSettings {
            idle_lock_enabled: true,
            idle_lock_seconds: 0,
            lock_on_minimize: false,
        });
        assert!(service.lock_if_idle());
        assert!(!service.is_unlocked());
    }

    #[test]
    fn lock_on_minimize_honours_the_setting() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut service = service(&dir, VaultSettings::default());
        service.create(&password()).expect("create");
        assert!(!service.lock_on_minimize(), "off by default");
        assert!(service.is_unlocked());

        service.set_settings(VaultSettings {
            lock_on_minimize: true,
            ..VaultSettings::default()
        });
        assert!(service.lock_on_minimize());
        assert!(!service.is_unlocked());
        assert!(
            !service.lock_on_minimize(),
            "locking an already-locked vault is not a lock event"
        );
    }

    #[test]
    fn the_reported_state_reflects_the_idle_setting() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut service = service(&dir, VaultSettings::default());
        assert!(matches!(service.state(), VaultState::Uninitialized));
        service.create(&password()).expect("create");
        match service.state() {
            VaultState::Unlocked {
                idle_lock_seconds, ..
            } => {
                assert_eq!(
                    idle_lock_seconds,
                    Some(VaultSettings::default().idle_lock_seconds)
                );
            }
            other => panic!("expected Unlocked, got {other:?}"),
        }

        service.set_settings(VaultSettings {
            idle_lock_enabled: false,
            ..VaultSettings::default()
        });
        match service.state() {
            VaultState::Unlocked {
                idle_lock_seconds, ..
            } => assert_eq!(idle_lock_seconds, None),
            other => panic!("expected Unlocked, got {other:?}"),
        }
    }

    #[test]
    fn secrets_round_trip_through_the_service() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut service = service(&dir, VaultSettings::default());
        service.create(&password()).expect("create");
        let reference = service
            .store(SecretKind::Password, SecretString::new("hunter2"))
            .expect("store");
        assert_eq!(service.reveal(reference).expect("reveal").expose(), "hunter2");

        service.lock(LockReason::Manual);
        assert!(service.reveal(reference).is_err());
        service.unlock(&password()).expect("unlock");
        assert_eq!(service.reveal(reference).expect("reveal").expose(), "hunter2");
    }

    #[test]
    fn orphan_collection_removes_only_unreferenced_secrets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut service = service(&dir, VaultSettings::default());
        service.create(&password()).expect("create");
        let keep = service
            .store(SecretKind::Password, SecretString::new("keep"))
            .expect("store");
        let drop_it = service
            .store(SecretKind::Password, SecretString::new("drop"))
            .expect("store");

        let live: BTreeSet<SecretRef> = [keep].into_iter().collect();
        assert_eq!(service.collect_orphans(&live).expect("collect"), 1);
        assert!(service.reveal(keep).is_ok());
        assert!(service.reveal(drop_it).is_err());
    }
}
