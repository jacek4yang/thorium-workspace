//! The unlocked-vault session: what is held in memory while the vault is open,
//! and the rules that put it away again.

use std::collections::BTreeSet;
use std::path::PathBuf;

use tw_domain::{SecretRef, Timestamp};
use tw_secrets::{SecretBytes, SecretString};

use crate::document::{SecretKind, SecretRecord, VaultDocument};
use crate::format::{KdfParameters, VaultHeader};
use crate::store::VaultStore;
use crate::{VaultError, VaultResult};

/// Why the vault was locked. Surfaced so the UI can explain itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LockReason {
    /// The vault has not been unlocked in this run.
    NeverUnlocked,
    /// The user locked it.
    Manual,
    /// The idle timeout elapsed.
    Idle,
    /// The window was minimised with lock-on-minimize enabled.
    Minimized,
    /// The application is shutting down.
    Shutdown,
}

/// What the UI is allowed to know about vault state.
///
/// Contains no key material and no secret values, so it can be sent across the
/// Tauri boundary and included in diagnostics as-is.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum VaultState {
    /// No vault file exists; the user must create one.
    Uninitialized,
    /// A vault exists but is locked.
    Locked {
        /// Why it is locked.
        reason: LockReason,
    },
    /// The vault is open.
    Unlocked {
        /// How many secrets it holds.
        secret_count: usize,
        /// When it was unlocked.
        unlocked_at: Timestamp,
        /// Seconds of inactivity before an automatic lock, when enabled.
        idle_lock_seconds: Option<u32>,
    },
}

/// An unlocked vault, or the knowledge that it is locked.
///
/// The derived key and decrypted document live here and nowhere else. Locking
/// drops both; [`SecretBytes`] and [`SecretString`] zeroize on drop, so the key
/// and every secret are scrubbed as the session tears down.
#[derive(Debug)]
pub struct VaultSession {
    store: VaultStore,
    kdf: KdfParameters,
    open: Option<OpenVault>,
    lock_reason: LockReason,
}

#[derive(Debug)]
struct OpenVault {
    header: VaultHeader,
    key: SecretBytes,
    document: VaultDocument,
    unlocked_at: Timestamp,
    last_activity: Timestamp,
}

impl VaultSession {
    /// Creates a locked session for the vault at `path`.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>, kdf: KdfParameters) -> Self {
        Self {
            store: VaultStore::new(path),
            kdf,
            open: None,
            lock_reason: LockReason::NeverUnlocked,
        }
    }

    /// The vault file path.
    #[must_use]
    pub fn path(&self) -> &std::path::Path {
        self.store.path()
    }

    /// Whether a vault file exists on disk.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.store.exists()
    }

    /// Whether the vault is currently open.
    #[must_use]
    pub fn is_unlocked(&self) -> bool {
        self.open.is_some()
    }

    /// A snapshot safe to send to the frontend.
    #[must_use]
    pub fn state(&self, idle_lock_seconds: Option<u32>) -> VaultState {
        match &self.open {
            Some(open) => VaultState::Unlocked {
                secret_count: open.document.len(),
                unlocked_at: open.unlocked_at,
                idle_lock_seconds,
            },
            None if self.store.exists() => VaultState::Locked {
                reason: self.lock_reason,
            },
            None => VaultState::Uninitialized,
        }
    }

    /// Creates the vault and leaves the session unlocked.
    ///
    /// # Errors
    ///
    /// See [`VaultStore::create`].
    pub fn create(&mut self, password: &SecretString) -> VaultResult<()> {
        let document = VaultDocument::new();
        let header = self.store.create(password, &document, self.kdf)?;
        let key = header.derive_key(password)?;
        let now = Timestamp::now();
        self.open = Some(OpenVault {
            header,
            key,
            document,
            unlocked_at: now,
            last_activity: now,
        });
        Ok(())
    }

    /// Unlocks the vault.
    ///
    /// A failed attempt leaves the session locked and holds no partial state.
    ///
    /// # Errors
    ///
    /// See [`VaultStore::open`].
    pub fn unlock(&mut self, password: &SecretString) -> VaultResult<()> {
        let (header, document, key) = self.store.open(password)?;
        let now = Timestamp::now();
        self.open = Some(OpenVault {
            header,
            key,
            document,
            unlocked_at: now,
            last_activity: now,
        });
        Ok(())
    }

    /// Locks the vault, dropping the key and every decrypted secret.
    pub fn lock(&mut self, reason: LockReason) {
        // Dropping `OpenVault` zeroizes the derived key and every SecretString
        // in the document.
        self.open = None;
        self.lock_reason = reason;
    }

    /// Records user activity, deferring the idle timeout.
    pub fn touch(&mut self) {
        if let Some(open) = self.open.as_mut() {
            open.last_activity = Timestamp::now();
        }
    }

    /// Seconds since the last recorded activity, or `None` when locked.
    #[must_use]
    pub fn idle_seconds(&self) -> Option<u64> {
        self.open
            .as_ref()
            .map(|open| open.last_activity.seconds_since(Timestamp::now()))
    }

    /// Locks the vault if it has been idle for at least `timeout_seconds`.
    ///
    /// Returns `true` when this call performed the lock.
    pub fn lock_if_idle(&mut self, timeout_seconds: u32) -> bool {
        let should_lock = self
            .idle_seconds()
            .is_some_and(|idle| idle >= u64::from(timeout_seconds));
        if should_lock {
            self.lock(LockReason::Idle);
        }
        should_lock
    }

    /// Reads a secret.
    ///
    /// Every reveal and copy goes through here, which is why it is the only way
    /// a plaintext secret can leave the vault.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Locked`] or [`VaultError::SecretNotFound`].
    pub fn reveal(&mut self, reference: SecretRef) -> VaultResult<SecretString> {
        let open = self.open.as_mut().ok_or(VaultError::Locked)?;
        open.last_activity = Timestamp::now();
        open.document
            .get(reference)
            .map(|record| SecretString::new(record.value.expose()))
            .ok_or(VaultError::SecretNotFound)
    }

    /// Returns a secret's metadata without its value.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Locked`] or [`VaultError::SecretNotFound`].
    pub fn describe(&self, reference: SecretRef) -> VaultResult<(SecretKind, Timestamp, Timestamp)> {
        let open = self.open.as_ref().ok_or(VaultError::Locked)?;
        open.document
            .get(reference)
            .map(|record: &SecretRecord| (record.kind, record.created_at, record.updated_at))
            .ok_or(VaultError::SecretNotFound)
    }

    /// Stores a secret and persists the vault, returning its reference.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Locked`] or a write failure.
    pub fn store_secret(&mut self, kind: SecretKind, value: SecretString) -> VaultResult<SecretRef> {
        let open = self.open.as_mut().ok_or(VaultError::Locked)?;
        let reference = open.document.insert(kind, value);
        open.last_activity = Timestamp::now();
        self.persist()?;
        Ok(reference)
    }

    /// Replaces an existing secret and persists the vault.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Locked`], [`VaultError::SecretNotFound`] or a write
    /// failure.
    pub fn replace_secret(&mut self, reference: SecretRef, value: SecretString) -> VaultResult<()> {
        let open = self.open.as_mut().ok_or(VaultError::Locked)?;
        if !open.document.replace(reference, value) {
            return Err(VaultError::SecretNotFound);
        }
        open.last_activity = Timestamp::now();
        self.persist()
    }

    /// Removes a secret and persists the vault. Removing an unknown reference
    /// succeeds so callers can delete an account without checking first.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Locked`] or a write failure.
    pub fn forget_secret(&mut self, reference: SecretRef) -> VaultResult<()> {
        let open = self.open.as_mut().ok_or(VaultError::Locked)?;
        let removed = open.document.remove(reference);
        open.last_activity = Timestamp::now();
        if removed { self.persist() } else { Ok(()) }
    }

    /// Drops every secret not referenced by live metadata and persists if
    /// anything changed. Returns how many were removed.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Locked`] or a write failure.
    pub fn collect_orphans(&mut self, live: &BTreeSet<SecretRef>) -> VaultResult<usize> {
        let open = self.open.as_mut().ok_or(VaultError::Locked)?;
        let removed = open.document.retain_only(live);
        if removed > 0 {
            self.persist()?;
        }
        Ok(removed)
    }

    /// Counts of stored secrets by kind, for diagnostics.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Locked`].
    pub fn counts_by_kind(&self) -> VaultResult<std::collections::BTreeMap<SecretKind, usize>> {
        self.open
            .as_ref()
            .map(|open| open.document.counts_by_kind())
            .ok_or(VaultError::Locked)
    }

    /// Changes the master password and re-opens the session under it.
    ///
    /// # Errors
    ///
    /// See [`VaultStore::change_password`].
    pub fn change_password(&mut self, current: &SecretString, next: &SecretString) -> VaultResult<()> {
        let (header, key) = self.store.change_password(current, next, self.kdf)?;
        let (_, document, _) = self.store.open(next)?;
        let now = Timestamp::now();
        self.open = Some(OpenVault {
            header,
            key,
            document,
            unlocked_at: now,
            last_activity: now,
        });
        Ok(())
    }

    /// Copies the vault file to `<name>.bak`.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Io`] when the copy fails.
    pub fn backup(&self) -> VaultResult<Option<PathBuf>> {
        self.store.backup()
    }

    /// Reads the header of a locked vault, for diagnostics.
    ///
    /// # Errors
    ///
    /// See [`VaultStore::peek_header`].
    pub fn peek_header(&self) -> VaultResult<VaultHeader> {
        self.store.peek_header()
    }

    fn persist(&mut self) -> VaultResult<()> {
        let open = self.open.as_mut().ok_or(VaultError::Locked)?;
        let header = self
            .store
            .save_with_key(&open.header, &open.key, &open.document)?;
        open.header = header;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn password() -> SecretString {
        SecretString::new("correct horse battery staple")
    }

    fn session(dir: &tempfile::TempDir) -> VaultSession {
        VaultSession::new(dir.path().join("workspace.twvault"), KdfParameters::testing())
    }

    #[test]
    fn a_fresh_workspace_reports_uninitialized() {
        let dir = tempfile::tempdir().expect("tempdir");
        let s = session(&dir);
        assert_eq!(s.state(None), VaultState::Uninitialized);
        assert!(!s.is_unlocked());
    }

    #[test]
    fn create_leaves_the_session_unlocked_and_persists() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut s = session(&dir);
        s.create(&password()).expect("create");
        assert!(s.is_unlocked());
        match s.state(Some(600)) {
            VaultState::Unlocked {
                secret_count,
                idle_lock_seconds,
                ..
            } => {
                assert_eq!(secret_count, 0);
                assert_eq!(idle_lock_seconds, Some(600));
            }
            other => panic!("expected Unlocked, got {other:?}"),
        }

        // A second session over the same file starts locked and opens.
        let mut reopened = session(&dir);
        assert!(matches!(
            reopened.state(None),
            VaultState::Locked {
                reason: LockReason::NeverUnlocked
            }
        ));
        reopened.unlock(&password()).expect("unlock");
        assert!(reopened.is_unlocked());
    }

    #[test]
    fn secrets_survive_a_lock_and_unlock_cycle() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut s = session(&dir);
        s.create(&password()).expect("create");
        let reference = s
            .store_secret(SecretKind::Password, SecretString::new("hunter2"))
            .expect("store");

        s.lock(LockReason::Manual);
        assert!(!s.is_unlocked());
        assert!(matches!(s.reveal(reference), Err(VaultError::Locked)));
        assert!(matches!(
            s.state(None),
            VaultState::Locked {
                reason: LockReason::Manual
            }
        ));

        s.unlock(&password()).expect("unlock");
        assert_eq!(s.reveal(reference).expect("reveal").expose(), "hunter2");
    }

    #[test]
    fn a_failed_unlock_leaves_the_session_locked() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut s = session(&dir);
        s.create(&password()).expect("create");
        s.lock(LockReason::Manual);
        assert!(s.unlock(&SecretString::new("wrong password entirely")).is_err());
        assert!(!s.is_unlocked());
        // The right password still works afterwards.
        assert!(s.unlock(&password()).is_ok());
    }

    #[test]
    fn every_mutation_is_written_through_immediately() {
        let dir = tempfile::tempdir().expect("tempdir");
        let reference;
        {
            let mut s = session(&dir);
            s.create(&password()).expect("create");
            reference = s
                .store_secret(SecretKind::OtpSeed, SecretString::new("JBSWY3DPEHPK3PXP"))
                .expect("store");
            // No explicit save call: the process could be killed right here.
        }
        let mut reopened = session(&dir);
        reopened.unlock(&password()).expect("unlock");
        assert_eq!(
            reopened.reveal(reference).expect("reveal").expose(),
            "JBSWY3DPEHPK3PXP"
        );
    }

    #[test]
    fn replace_and_forget_are_persisted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut s = session(&dir);
        s.create(&password()).expect("create");
        let reference = s
            .store_secret(SecretKind::Password, SecretString::new("hunter2"))
            .expect("store");
        s.replace_secret(reference, SecretString::new("hunter3"))
            .expect("replace");
        assert!(matches!(
            s.replace_secret(SecretRef::new(), SecretString::new("x")),
            Err(VaultError::SecretNotFound)
        ));

        s.lock(LockReason::Manual);
        s.unlock(&password()).expect("unlock");
        assert_eq!(s.reveal(reference).expect("reveal").expose(), "hunter3");

        s.forget_secret(reference).expect("forget");
        s.forget_secret(reference)
            .expect("forgetting twice is not an error");
        assert!(matches!(s.reveal(reference), Err(VaultError::SecretNotFound)));
    }

    #[test]
    fn idle_locking_uses_recorded_activity() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut s = session(&dir);
        s.create(&password()).expect("create");
        assert_eq!(s.idle_seconds(), Some(0));
        // A timeout longer than the elapsed idle time must not fire.
        assert!(!s.lock_if_idle(3600));
        assert!(s.is_unlocked());
        // A zero timeout always fires.
        assert!(s.lock_if_idle(0));
        assert!(!s.is_unlocked());
        assert!(matches!(
            s.state(None),
            VaultState::Locked {
                reason: LockReason::Idle
            }
        ));
        assert_eq!(s.idle_seconds(), None);
        assert!(
            !s.lock_if_idle(0),
            "locking an already-locked vault is not a lock event"
        );
    }

    #[test]
    fn touch_defers_the_idle_timeout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut s = session(&dir);
        s.create(&password()).expect("create");
        s.touch();
        assert_eq!(s.idle_seconds(), Some(0));
    }

    #[test]
    fn orphaned_secrets_are_collected_and_persisted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut s = session(&dir);
        s.create(&password()).expect("create");
        let keep = s
            .store_secret(SecretKind::Password, SecretString::new("keep this one"))
            .expect("store");
        let orphan = s
            .store_secret(SecretKind::Password, SecretString::new("orphaned"))
            .expect("store");

        let live: BTreeSet<SecretRef> = [keep].into_iter().collect();
        assert_eq!(s.collect_orphans(&live).expect("collect"), 1);
        assert_eq!(s.collect_orphans(&live).expect("collect"), 0);

        s.lock(LockReason::Manual);
        s.unlock(&password()).expect("unlock");
        assert!(s.reveal(keep).is_ok());
        assert!(matches!(s.reveal(orphan), Err(VaultError::SecretNotFound)));
    }

    #[test]
    fn changing_the_password_keeps_the_session_usable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut s = session(&dir);
        s.create(&password()).expect("create");
        let reference = s
            .store_secret(SecretKind::Password, SecretString::new("hunter2"))
            .expect("store");

        let next = SecretString::new("an entirely different passphrase");
        s.change_password(&password(), &next).expect("re-key");
        assert!(s.is_unlocked());
        assert_eq!(s.reveal(reference).expect("reveal").expose(), "hunter2");

        s.lock(LockReason::Manual);
        assert!(s.unlock(&password()).is_err());
        assert!(s.unlock(&next).is_ok());
    }

    #[test]
    fn locked_sessions_refuse_every_secret_operation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut s = session(&dir);
        s.create(&password()).expect("create");
        s.lock(LockReason::Shutdown);
        assert!(matches!(s.reveal(SecretRef::new()), Err(VaultError::Locked)));
        assert!(matches!(s.describe(SecretRef::new()), Err(VaultError::Locked)));
        assert!(matches!(
            s.store_secret(SecretKind::Note, SecretString::new("x")),
            Err(VaultError::Locked)
        ));
        assert!(matches!(
            s.forget_secret(SecretRef::new()),
            Err(VaultError::Locked)
        ));
        assert!(matches!(s.counts_by_kind(), Err(VaultError::Locked)));
        assert!(matches!(
            s.collect_orphans(&BTreeSet::new()),
            Err(VaultError::Locked)
        ));
    }

    #[test]
    fn the_session_debug_output_reveals_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut s = session(&dir);
        s.create(&password()).expect("create");
        s.store_secret(SecretKind::Password, SecretString::new("hunter2"))
            .expect("store");
        let rendered = format!("{s:#?}");
        assert!(!rendered.contains("hunter2"), "{rendered}");
        assert!(!rendered.contains("correct horse"), "{rendered}");
    }

    #[test]
    fn describe_returns_metadata_without_the_value() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut s = session(&dir);
        s.create(&password()).expect("create");
        let reference = s
            .store_secret(SecretKind::RecoveryCode, SecretString::new("abcd-efgh"))
            .expect("store");
        let (kind, created, updated) = s.describe(reference).expect("describe");
        assert_eq!(kind, SecretKind::RecoveryCode);
        assert_eq!(created, updated);
        assert!(matches!(
            s.describe(SecretRef::new()),
            Err(VaultError::SecretNotFound)
        ));
    }
}
