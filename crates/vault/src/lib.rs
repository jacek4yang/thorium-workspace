//! Versioned encrypted vault for account secrets.
//!
//! Format (see [`format`]): Argon2id key derivation + ChaCha20-Poly1305
//! authenticated encryption with the file header as additional data.
//! See `docs/DECISIONS.md` for the KDBX4 evaluation that led to this
//! custom format.
//!
//! Security posture:
//! - the master password is taken only as [`SecretText`] and is never
//!   stored or logged;
//! - entry values live in [`SecretBytes`] (redacted `Debug`, no
//!   `Serialize`, zeroized on drop);
//! - plaintext payload bytes exist only transiently in memory during
//!   save/load and are scrubbed;
//! - saves are atomic (temp file + rename) and keep the previous good
//!   file as `<vault>.bak`;
//! - locking zeroizes the derived key; unlocking is required for any
//!   content operation.
//!
//! Policy like idle auto-lock and lock-on-minimize lives in the
//! controller, which observes activity and calls [`Vault::lock`].

#![forbid(unsafe_code)]

mod crypto;
mod error;
mod format;
mod payload;

pub use error::VaultError;
pub use payload::{VaultEntry, VaultEntryKind, VaultPayload};

use std::path::{Path, PathBuf};

use chrono::Utc;
use thorium_workspace_secrets::SecretText;
use zeroize::Zeroize;

/// Lock state of an opened vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultLockState {
    /// No vault file exists at the path yet.
    Missing,
    /// A vault file exists but no key is derived.
    Locked,
    /// A key is derived and content operations are possible.
    Unlocked,
}

enum VaultState {
    Missing,
    Locked,
    Unlocked { key: crypto::VaultKey },
}

/// An opened vault bound to one file path.
pub struct Vault {
    path: PathBuf,
    state: VaultState,
}

impl Vault {
    /// Opens the vault at `path`. Reads nothing but the file's existence;
    /// deriving a key requires [`Vault::unlock`] (or [`Vault::create`]).
    pub fn open(path: &Path) -> Result<Self, VaultError> {
        let state = if path.exists() {
            VaultState::Locked
        } else {
            VaultState::Missing
        };
        Ok(Self {
            path: path.to_owned(),
            state,
        })
    }

    /// Vault file path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Current lock state (cheap; no file I/O).
    pub fn lock_state(&self) -> VaultLockState {
        match self.state {
            VaultState::Missing => VaultLockState::Missing,
            VaultState::Locked => VaultLockState::Locked,
            VaultState::Unlocked { .. } => VaultLockState::Unlocked,
        }
    }

    /// Whether a vault file exists.
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    /// Whether a key is derived.
    pub fn is_unlocked(&self) -> bool {
        matches!(self.state, VaultState::Unlocked { .. })
    }

    /// Creates a new vault with `master_password`, stores an initially
    /// empty payload, and leaves the vault unlocked. Fails with
    /// [`VaultError::AlreadyExists`] when a vault file already exists.
    pub fn create(&mut self, master_password: &SecretText) -> Result<(), VaultError> {
        if self.exists() {
            return Err(VaultError::AlreadyExists);
        }
        if !matches!(self.state, VaultState::Missing) {
            return Err(VaultError::AlreadyUnlocked);
        }
        let payload = VaultPayload::empty(Utc::now());
        self.write_locked_file(master_password, &payload)?;
        let header = self.read_header()?;
        let key = crypto::VaultKey::derive(master_password, &header.salt, &header.kdf)?;
        self.state = VaultState::Unlocked { key };
        Ok(())
    }

    /// Derives the key from `master_password` and verifies it against the
    /// stored file. Fails with [`VaultError::UnlockFailed`] on a wrong
    /// password or a damaged file.
    pub fn unlock(&mut self, master_password: &SecretText) -> Result<(), VaultError> {
        if matches!(self.state, VaultState::Unlocked { .. }) {
            return Err(VaultError::AlreadyUnlocked);
        }
        let header = self.read_header()?;
        let key = crypto::VaultKey::derive(master_password, &header.salt, &header.kdf)?;
        // Verify by decrypting now rather than trusting the tag check
        // later: a wrong password must fail here, deterministically.
        let verified = self.decrypt_payload_with(&key, &header);
        match verified {
            Ok(_) => {
                self.state = VaultState::Unlocked { key };
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    /// Drops the derived key (zeroized via its wrapper). Idempotent.
    pub fn lock(&mut self) {
        if !matches!(self.state, VaultState::Missing) {
            self.state = VaultState::Locked;
        }
    }

    /// Persists `payload` atomically. The previous good file is kept as
    /// `<vault>.bak` before the replacement is promoted.
    pub fn save(&self, payload: &VaultPayload) -> Result<(), VaultError> {
        let VaultState::Unlocked { key } = &self.state else {
            return Err(VaultError::Locked);
        };
        let header = self.read_header()?;
        let mut json = payload::serialize_payload(payload)?;
        let ciphertext = crypto::encrypt(key, &header.nonce, &header.encoded(), &json);
        json.zeroize();
        let ciphertext = ciphertext?;
        self.atomic_write(&header.encoded(), &ciphertext)
    }

    /// Loads and decrypts the current file content.
    pub fn load(&self) -> Result<VaultPayload, VaultError> {
        let VaultState::Unlocked { key } = &self.state else {
            return Err(VaultError::Locked);
        };
        let header = self.read_header()?;
        self.decrypt_payload_with(key, &header)
    }

    /// Changes the master password: verifies `current`, then re-derives a
    /// fresh salt/key for `new_password` and rewrites the file atomically.
    /// Content is preserved.
    pub fn change_master_password(
        &mut self,
        current: &SecretText,
        new_password: &SecretText,
    ) -> Result<(), VaultError> {
        if !matches!(self.state, VaultState::Unlocked { .. }) {
            return Err(VaultError::Locked);
        }
        let old_header = self.read_header()?;
        let old_key = crypto::VaultKey::derive(current, &old_header.salt, &old_header.kdf)?;
        let payload = self.decrypt_payload_with(&old_key, &old_header)?;
        self.write_locked_file(new_password, &payload)?;
        let header = self.read_header()?;
        let key = crypto::VaultKey::derive(new_password, &header.salt, &header.kdf)?;
        self.state = VaultState::Unlocked { key };
        Ok(())
    }

    /// Reads and parses the file header.
    fn read_header(&self) -> Result<format::VaultHeader, VaultError> {
        let bytes = std::fs::read(&self.path).map_err(|source| VaultError::Io {
            path: self.path.clone(),
            source,
        })?;
        format::VaultHeader::from_bytes(&bytes)
    }

    /// Decrypts the payload using an explicit key (used both for unlock
    /// verification and for [`Vault::load`]).
    fn decrypt_payload_with(
        &self,
        key: &crypto::VaultKey,
        header: &format::VaultHeader,
    ) -> Result<VaultPayload, VaultError> {
        let bytes = std::fs::read(&self.path).map_err(|source| VaultError::Io {
            path: self.path.clone(),
            source,
        })?;
        if bytes.len() < format::HEADER_LEN {
            return Err(VaultError::Corrupt {
                detail: "file too small to contain a payload".to_owned(),
            });
        }
        let json = crypto::decrypt(
            key,
            &header.nonce,
            &header.encoded(),
            &bytes[format::HEADER_LEN..],
        )?;
        payload::deserialize_payload(json)
    }

    /// Encrypts `payload` under a freshly derived key for
    /// create/password-change and writes the file.
    fn write_locked_file(
        &self,
        master_password: &SecretText,
        payload: &VaultPayload,
    ) -> Result<(), VaultError> {
        let salt = crypto::fresh_salt()?;
        let kdf = format::KdfParams::default();
        let nonce = crypto::fresh_nonce()?;
        let key = crypto::VaultKey::derive(master_password, &salt, &kdf)?;
        let mut json = payload::serialize_payload(payload)?;
        let header = format::VaultHeader { salt, kdf, nonce }.encoded();
        let ciphertext = crypto::encrypt(&key, &nonce, &header, &json);
        json.zeroize();
        let ciphertext = ciphertext?;
        self.atomic_write(&header, &ciphertext)
    }

    /// Writes header+ciphertext to a temp file and renames it over the
    /// target, keeping the previous file as `.bak`.
    fn atomic_write(&self, header: &[u8], ciphertext: &[u8]) -> Result<(), VaultError> {
        let tmp_path = tmp_path_for(&self.path);
        let bak_path = backup_path_for(&self.path);
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&tmp_path).map_err(|source| VaultError::Io {
                path: tmp_path.clone(),
                source,
            })?;
            file.write_all(header).map_err(|source| VaultError::Io {
                path: tmp_path.clone(),
                source,
            })?;
            file.write_all(ciphertext)
                .map_err(|source| VaultError::Io {
                    path: tmp_path.clone(),
                    source,
                })?;
            file.sync_all().map_err(|source| VaultError::Io {
                path: tmp_path.clone(),
                source,
            })?;
        }
        if self.path.exists() {
            std::fs::copy(&self.path, &bak_path).map_err(|source| VaultError::Io {
                path: bak_path.clone(),
                source,
            })?;
        }
        std::fs::rename(&tmp_path, &self.path).map_err(|source| VaultError::Io {
            path: self.path.clone(),
            source,
        })?;
        Ok(())
    }
}

fn tmp_path_for(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| format!("{}.tmp", name.to_string_lossy()))
        .unwrap_or_else(|| "vault.tmp".to_owned());
    path.with_file_name(name)
}

fn backup_path_for(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| format!("{}.bak", name.to_string_lossy()))
        .unwrap_or_else(|| "vault.bak".to_owned());
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use thorium_workspace_secrets::SecretBytes;

    const SYNTHETIC_MASTER: &str = "synthetic-master-1234567890";
    const SYNTHETIC_VALUE: &[u8] = b"synthetic-entry-value";

    fn temp_vault(tag: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(format!("{tag}.bin"));
        (dir, path)
    }

    fn entry(reference: &str, kind: VaultEntryKind) -> VaultEntry {
        VaultEntry {
            secret_ref: reference.parse().expect("valid ref"),
            kind,
            value: SecretBytes::new(SYNTHETIC_VALUE),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn create_unlock_save_load_roundtrip() {
        let (_dir, path) = temp_vault("roundtrip");
        let mut vault = Vault::open(&path).expect("open");
        assert_eq!(vault.lock_state(), VaultLockState::Missing);

        vault
            .create(&SecretText::new(SYNTHETIC_MASTER))
            .expect("create");
        assert!(vault.exists());
        assert!(vault.is_unlocked());

        let mut payload = VaultPayload::empty(Utc::now());
        payload.put(entry(
            "account/aaaaaa01-0000-0000-0000-000000000001/password",
            VaultEntryKind::Password,
        ));
        payload.put(entry(
            "factor/aaaaaa01-0000-0000-0000-000000000002/seed",
            VaultEntryKind::OtpSeed,
        ));
        vault.save(&payload).expect("save");

        let loaded = vault.load().expect("load");
        assert_eq!(loaded.entries.len(), 2);
        let reference: thorium_workspace_domain::SecretRef =
            "account/aaaaaa01-0000-0000-0000-000000000001/password"
                .parse()
                .expect("ref");
        assert_eq!(
            loaded.get(&reference).expect("entry").value.expose(),
            SYNTHETIC_VALUE
        );
    }

    #[test]
    fn create_on_existing_vault_fails() {
        let (_dir, path) = temp_vault("exists");
        let mut vault = Vault::open(&path).expect("open");
        vault
            .create(&SecretText::new(SYNTHETIC_MASTER))
            .expect("create");
        let mut second = Vault::open(&path).expect("open second");
        let error = second
            .create(&SecretText::new(SYNTHETIC_MASTER))
            .expect_err("must refuse");
        assert!(matches!(error, VaultError::AlreadyExists));
    }

    #[test]
    fn wrong_password_is_rejected_and_leaks_nothing() {
        let (_dir, path) = temp_vault("wrong");
        let mut vault = Vault::open(&path).expect("open");
        vault
            .create(&SecretText::new(SYNTHETIC_MASTER))
            .expect("create");
        vault.lock();
        assert_eq!(vault.lock_state(), VaultLockState::Locked);

        let error = vault
            .unlock(&SecretText::new("definitely-not-the-password"))
            .expect_err("wrong password must fail");
        assert!(matches!(error, VaultError::UnlockFailed));
        let rendered = format!("{error}");
        assert!(!rendered.contains(SYNTHETIC_MASTER));
        assert!(!vault.is_unlocked());

        vault
            .unlock(&SecretText::new(SYNTHETIC_MASTER))
            .expect("correct");
        assert!(vault.is_unlocked());
    }

    #[test]
    fn locked_vault_refuses_content_operations() {
        let (_dir, path) = temp_vault("locked");
        let mut vault = Vault::open(&path).expect("open");
        vault
            .create(&SecretText::new(SYNTHETIC_MASTER))
            .expect("create");
        vault.lock();
        assert!(matches!(vault.load(), Err(VaultError::Locked)));
        assert!(matches!(
            vault.save(&VaultPayload::empty(Utc::now())),
            Err(VaultError::Locked)
        ));
        // Lock is idempotent.
        vault.lock();
        assert_eq!(vault.lock_state(), VaultLockState::Locked);
    }

    #[test]
    fn reopen_requires_unlock_and_keeps_data() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("reopen.bin");
        let mut payload = VaultPayload::empty(Utc::now());
        payload.put(entry(
            "recovery/aaaaaa01-0000-0000-0000-000000000003/value",
            VaultEntryKind::RecoveryCode,
        ));

        {
            let mut vault = Vault::open(&path).expect("open");
            vault
                .create(&SecretText::new(SYNTHETIC_MASTER))
                .expect("create");
            vault.save(&payload).expect("save");
        }
        {
            let mut vault = Vault::open(&path).expect("reopen");
            assert_eq!(vault.lock_state(), VaultLockState::Locked);
            vault
                .unlock(&SecretText::new(SYNTHETIC_MASTER))
                .expect("unlock");
            let loaded = vault.load().expect("load");
            assert_eq!(loaded.entries.len(), 1);
            assert_eq!(loaded.entries[0].value.expose(), SYNTHETIC_VALUE);
        }
    }

    #[test]
    fn corrupted_ciphertext_and_truncation_are_detected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("corrupt.bin");
        {
            let mut vault = Vault::open(&path).expect("open");
            vault
                .create(&SecretText::new(SYNTHETIC_MASTER))
                .expect("create");
        }
        let mut bytes = std::fs::read(&path).expect("read");
        let last = bytes.len() - 1;
        bytes[last] ^= 0x42;
        std::fs::write(&path, &bytes).expect("write corrupted");
        let mut vault = Vault::open(&path).expect("open");
        let error = vault
            .unlock(&SecretText::new(SYNTHETIC_MASTER))
            .expect_err("corrupted file must fail");
        assert!(matches!(error, VaultError::UnlockFailed));

        // Truncated payload region.
        let mut bytes = std::fs::read(&path).expect("read");
        bytes.truncate(format::HEADER_LEN + 4);
        std::fs::write(&path, &bytes).expect("write truncated");
        let mut vault = Vault::open(&path).expect("open");
        assert!(vault.unlock(&SecretText::new(SYNTHETIC_MASTER)).is_err());
    }

    #[test]
    fn garbage_file_is_rejected_as_corrupt() {
        use thorium_workspace_domain::DiagnosticCode;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("garbage.bin");
        std::fs::write(&path, b"this is not a vault at all").expect("write");
        let mut vault = Vault::open(&path).expect("open");
        let error = vault
            .unlock(&SecretText::new(SYNTHETIC_MASTER))
            .expect_err("garbage must fail");
        assert!(matches!(error, VaultError::Corrupt { .. }));
        assert_eq!(error.diagnostic_code(), "VAULT_CORRUPT");
    }

    #[test]
    fn atomic_save_leaves_no_tmp_and_keeps_backup() {
        let (_dir, path) = temp_vault("atomic");
        let mut vault = Vault::open(&path).expect("open");
        vault
            .create(&SecretText::new(SYNTHETIC_MASTER))
            .expect("create");

        let mut first = VaultPayload::empty(Utc::now());
        first.put(entry(
            "account/aaaaaa01-0000-0000-0000-000000000004/password",
            VaultEntryKind::Password,
        ));
        vault.save(&first).expect("first save");
        let first_updated = vault.load().expect("load").updated_at;

        let mut second = VaultPayload::empty(Utc::now());
        second.created_at = first.created_at;
        second.updated_at += chrono::Duration::seconds(1);
        vault.save(&second).expect("second save");

        assert!(!tmp_path_for(&path).exists(), "temp file must be renamed");
        let bak = backup_path_for(&path);
        assert!(bak.exists(), "previous good file must be kept as .bak");

        // The backup decrypts to the first generation.
        let backup_bytes = std::fs::read(&bak).expect("read bak");
        let header = format::VaultHeader::from_bytes(&backup_bytes).expect("bak header");
        let key = crypto::VaultKey::derive(
            &SecretText::new(SYNTHETIC_MASTER),
            &header.salt,
            &header.kdf,
        )
        .expect("derive");
        let json = crypto::decrypt(
            &key,
            &header.nonce,
            &header.encoded(),
            &backup_bytes[format::HEADER_LEN..],
        )
        .expect("bak decrypts");
        let backup_payload = payload::deserialize_payload(json).expect("bak payload");
        assert!(
            backup_payload
                .get(
                    &"account/aaaaaa01-0000-0000-0000-000000000004/password"
                        .parse()
                        .expect("ref")
                )
                .is_some()
        );
        assert_eq!(backup_payload.updated_at, first_updated);
    }

    #[test]
    fn change_master_password_rotates_salt_and_key() {
        let (_dir, path) = temp_vault("rotate");
        let mut vault = Vault::open(&path).expect("open");
        vault
            .create(&SecretText::new(SYNTHETIC_MASTER))
            .expect("create");
        let mut payload = VaultPayload::empty(Utc::now());
        payload.put(entry(
            "account/aaaaaa01-0000-0000-0000-000000000005/password",
            VaultEntryKind::Password,
        ));
        vault.save(&payload).expect("save");

        let new_master = "synthetic-rotated-master";
        vault
            .change_master_password(
                &SecretText::new(SYNTHETIC_MASTER),
                &SecretText::new(new_master),
            )
            .expect("rotate");
        vault.lock();
        assert!(
            vault.unlock(&SecretText::new(SYNTHETIC_MASTER)).is_err(),
            "old password must stop working"
        );
        vault
            .unlock(&SecretText::new(new_master))
            .expect("new password works");
        let loaded = vault.load().expect("load");
        assert_eq!(loaded.entries.len(), 1);

        // Wrong current password is rejected and leaves the vault usable.
        let error = vault
            .change_master_password(
                &SecretText::new("wrong-current"),
                &SecretText::new("unused"),
            )
            .expect_err("wrong current");
        assert!(matches!(error, VaultError::UnlockFailed));
        assert!(
            vault.load().is_ok(),
            "failed rotation must not damage state"
        );
    }

    #[test]
    fn file_never_contains_plaintext_secrets() {
        let (_dir, path) = temp_vault("plaintext");
        let mut vault = Vault::open(&path).expect("open");
        vault
            .create(&SecretText::new(SYNTHETIC_MASTER))
            .expect("create");
        let mut payload = VaultPayload::empty(Utc::now());
        payload.put(entry(
            "account/aaaaaa01-0000-0000-0000-000000000006/password",
            VaultEntryKind::Password,
        ));
        vault.save(&payload).expect("save");
        let mut payload = vault.load().expect("load");
        payload.put(entry(
            "recovery/aaaaaa01-0000-0000-0000-000000000007/value",
            VaultEntryKind::RecoveryCode,
        ));
        vault.save(&payload).expect("second save");

        for file in [path.clone(), backup_path_for(&path)] {
            let bytes = std::fs::read(&file).expect("read");
            let as_text = bytes.iter().map(|b| *b as char).collect::<String>();
            assert!(
                !as_text.contains(SYNTHETIC_MASTER),
                "master password leaked in {}",
                file.display()
            );
            assert!(
                !bytes
                    .windows(SYNTHETIC_VALUE.len())
                    .any(|w| w == SYNTHETIC_VALUE),
                "entry value leaked in {}",
                file.display()
            );
        }
    }

    #[test]
    fn entry_debug_is_redacted() {
        let value = entry(
            "account/aaaaaa01-0000-0000-0000-000000000008/password",
            VaultEntryKind::Password,
        );
        let rendered = format!("{value:?}");
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("synthetic-entry-value"));
    }
}
