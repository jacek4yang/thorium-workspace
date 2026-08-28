//! Reading and writing the encrypted vault file.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use tw_secrets::{SecretBytes, SecretString};

use crate::document::VaultDocument;
use crate::format::{HEADER_LEN, KdfParameters, TAG_LEN, VaultHeader};
use crate::{VaultError, VaultResult, check_master_password};

/// The path of the backup written before a risky rewrite of `path`.
#[must_use]
pub fn backup_path_for(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".bak");
    path.with_file_name(name)
}

fn temp_path_for(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

/// The vault file on disk.
///
/// A `VaultStore` holds no key material: it converts between a
/// [`VaultDocument`] and an encrypted file, and every operation takes the master
/// password or a derived key explicitly. Unlocked state lives in
/// [`crate::VaultSession`].
#[derive(Debug, Clone)]
pub struct VaultStore {
    path: PathBuf,
}

impl VaultStore {
    /// Points a store at `path`. Does not touch the filesystem.
    #[must_use]
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The vault file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether a vault file exists.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.path.is_file()
    }

    /// Creates a new vault containing `document`.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::AlreadyExists`] rather than overwriting an existing
    /// file, [`VaultError::WeakPassword`] when the password fails policy, and
    /// [`VaultError::Io`] when the file cannot be written.
    pub fn create(
        &self,
        password: &SecretString,
        document: &VaultDocument,
        kdf: KdfParameters,
    ) -> VaultResult<VaultHeader> {
        if self.exists() {
            return Err(VaultError::AlreadyExists);
        }
        check_master_password(password)?;
        let header = VaultHeader::new_random(kdf)?;
        let key = header.derive_key(password)?;
        self.write_with_key(&header, &key, document)?;
        Ok(header)
    }

    /// Opens the vault, returning its header and decrypted document.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Missing`] when there is no file,
    /// [`VaultError::BadPassword`] when authentication fails, and
    /// [`VaultError::Corrupt`] for a structurally invalid file.
    pub fn open(&self, password: &SecretString) -> VaultResult<(VaultHeader, VaultDocument, SecretBytes)> {
        let bytes = self.read_file()?;
        let header = VaultHeader::parse(&bytes)?;
        let key = header.derive_key(password)?;
        let document = decrypt_document(&header, &key, &bytes)?;
        Ok((header, document, key))
    }

    /// Writes `document` using an already-derived key.
    ///
    /// The caller supplies the key so a save during an unlocked session does not
    /// have to re-run Argon2id, which would add a visible pause to every edit.
    /// A fresh nonce is generated for each write.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Crypto`] on an encryption failure and
    /// [`VaultError::Io`] when the file cannot be written.
    pub fn save_with_key(
        &self,
        header: &VaultHeader,
        key: &SecretBytes,
        document: &VaultDocument,
    ) -> VaultResult<VaultHeader> {
        let fresh = header.with_fresh_nonce()?;
        self.write_with_key(&fresh, key, document)?;
        Ok(fresh)
    }

    /// Re-encrypts the vault under a new master password.
    ///
    /// A backup of the current file is taken first: this is the operation where
    /// a crash could otherwise leave the user with a file whose password they do
    /// not know.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::BadPassword`] when `current` is wrong,
    /// [`VaultError::WeakPassword`] when `next` fails policy, and
    /// [`VaultError::Io`] on a filesystem failure.
    pub fn change_password(
        &self,
        current: &SecretString,
        next: &SecretString,
        kdf: KdfParameters,
    ) -> VaultResult<(VaultHeader, SecretBytes)> {
        let (_, document, _) = self.open(current)?;
        check_master_password(next)?;
        self.backup()?;
        let header = VaultHeader::new_random(kdf)?;
        let key = header.derive_key(next)?;
        self.write_with_key(&header, &key, &document)?;
        Ok((header, key))
    }

    /// Copies the current vault file to `<name>.bak`, replacing any previous
    /// backup. Does nothing when there is no vault yet.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Io`] when the copy fails.
    pub fn backup(&self) -> VaultResult<Option<PathBuf>> {
        if !self.exists() {
            return Ok(None);
        }
        let backup = backup_path_for(&self.path);
        fs::copy(&self.path, &backup).map_err(|e| VaultError::io("back up the vault", e))?;
        Ok(Some(backup))
    }

    /// Reads the vault header without needing the master password.
    ///
    /// Used by diagnostics to report the format version and KDF cost of a locked
    /// vault.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Missing`] or [`VaultError::Corrupt`].
    pub fn peek_header(&self) -> VaultResult<VaultHeader> {
        let bytes = self.read_file()?;
        VaultHeader::parse(&bytes)
    }

    fn read_file(&self) -> VaultResult<Vec<u8>> {
        match fs::read(&self.path) {
            Ok(bytes) => Ok(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(VaultError::Missing),
            Err(e) => Err(VaultError::io("read the vault", e)),
        }
    }

    /// Encrypts and writes atomically: full contents to a sibling temporary
    /// file, flushed and synced, then renamed over the target.
    ///
    /// A crash therefore leaves either the previous complete vault or the new
    /// complete vault, never a half-written one.
    fn write_with_key(
        &self,
        header: &VaultHeader,
        key: &SecretBytes,
        document: &VaultDocument,
    ) -> VaultResult<()> {
        let plaintext = serde_json::to_vec(document).map_err(|_| VaultError::Payload)?;
        let header_bytes = header.to_bytes();
        let cipher = XChaCha20Poly1305::new_from_slice(key.expose()).map_err(|_| VaultError::Crypto)?;
        let nonce = XNonce::from_slice(&header.nonce);
        let ciphertext = cipher
            .encrypt(
                nonce,
                Payload {
                    msg: &plaintext,
                    aad: &header_bytes,
                },
            )
            .map_err(|_| VaultError::Crypto)?;

        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent).map_err(|e| VaultError::io("create the vault directory", e))?;
        }

        let temp = temp_path_for(&self.path);
        {
            let mut file = File::create(&temp).map_err(|e| VaultError::io("create the vault file", e))?;
            file.write_all(&header_bytes)
                .map_err(|e| VaultError::io("write the vault", e))?;
            file.write_all(&ciphertext)
                .map_err(|e| VaultError::io("write the vault", e))?;
            file.flush().map_err(|e| VaultError::io("write the vault", e))?;
            file.sync_all()
                .map_err(|e| VaultError::io("flush the vault to disk", e))?;
        }
        fs::rename(&temp, &self.path).map_err(|e| VaultError::io("replace the vault", e))?;
        Ok(())
    }
}

fn decrypt_document(header: &VaultHeader, key: &SecretBytes, file: &[u8]) -> VaultResult<VaultDocument> {
    let body = file.get(HEADER_LEN..).unwrap_or_default();
    if body.len() < TAG_LEN {
        return Err(VaultError::Corrupt {
            reason: "the encrypted body is truncated".to_owned(),
        });
    }
    let cipher = XChaCha20Poly1305::new_from_slice(key.expose()).map_err(|_| VaultError::Crypto)?;
    let nonce = XNonce::from_slice(&header.nonce);
    let header_bytes = header.to_bytes();
    // A failure here means the key was wrong, the header was edited or the body
    // was altered. All three are reported as BadPassword: the AEAD cannot tell
    // them apart, and neither should an attacker.
    let plaintext = cipher
        .decrypt(
            nonce,
            Payload {
                msg: body,
                aad: &header_bytes,
            },
        )
        .map_err(|_| VaultError::BadPassword)?;
    serde_json::from_slice(&plaintext).map_err(|_| VaultError::Payload)
}

#[cfg(test)]
mod tests {
    use tw_domain::SecretRef;

    use super::*;
    use crate::document::SecretKind;

    fn password() -> SecretString {
        SecretString::new("correct horse battery staple")
    }

    fn seeded_document() -> (VaultDocument, SecretRef) {
        let mut doc = VaultDocument::new();
        let reference = doc.insert(SecretKind::Password, SecretString::new("hunter2"));
        doc.insert(SecretKind::OtpSeed, SecretString::new("JBSWY3DPEHPK3PXP"));
        (doc, reference)
    }

    fn store(dir: &tempfile::TempDir) -> VaultStore {
        VaultStore::new(dir.path().join("vault").join("workspace.twvault"))
    }

    #[test]
    fn create_unlock_and_read_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store(&dir);
        assert!(!store.exists());
        let (doc, reference) = seeded_document();
        store
            .create(&password(), &doc, KdfParameters::testing())
            .expect("create");
        assert!(store.exists());

        let (header, reopened, _key) = store.open(&password()).expect("open");
        assert_eq!(header.version, crate::FORMAT_VERSION);
        assert_eq!(
            reopened.get(reference).expect("present").value.expose(),
            "hunter2"
        );
        assert_eq!(reopened.len(), 2);
    }

    #[test]
    fn the_file_never_contains_plaintext_secrets() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store(&dir);
        let (doc, _) = seeded_document();
        store
            .create(&password(), &doc, KdfParameters::testing())
            .expect("create");

        let bytes = fs::read(store.path()).expect("read");
        for needle in [
            b"hunter2".as_slice(),
            b"JBSWY3DPEHPK3PXP".as_slice(),
            b"password".as_slice(),
        ] {
            assert!(
                !bytes.windows(needle.len()).any(|w| w == needle),
                "found plaintext {:?} in the vault file",
                String::from_utf8_lossy(needle)
            );
        }
        assert_eq!(&bytes[0..8], crate::format::MAGIC);
    }

    #[test]
    fn a_wrong_password_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store(&dir);
        let (doc, _) = seeded_document();
        store
            .create(&password(), &doc, KdfParameters::testing())
            .expect("create");
        let err = store
            .open(&SecretString::new("wrong horse battery staple"))
            .expect_err("must fail");
        assert!(matches!(err, VaultError::BadPassword), "{err:?}");
    }

    #[test]
    fn creating_over_an_existing_vault_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store(&dir);
        let (doc, _) = seeded_document();
        store
            .create(&password(), &doc, KdfParameters::testing())
            .expect("create");
        let err = store
            .create(&password(), &doc, KdfParameters::testing())
            .expect_err("must refuse");
        assert!(matches!(err, VaultError::AlreadyExists));
        // The original file must survive the refused call.
        assert!(store.open(&password()).is_ok());
    }

    #[test]
    fn opening_a_missing_vault_is_reported_distinctly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = store(&dir).open(&password()).expect_err("must fail");
        assert!(matches!(err, VaultError::Missing));
    }

    #[test]
    fn a_short_password_is_refused_before_a_file_is_written() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store(&dir);
        let (doc, _) = seeded_document();
        assert!(
            store
                .create(&SecretString::new("short"), &doc, KdfParameters::testing())
                .is_err()
        );
        assert!(
            !store.exists(),
            "no file may be left behind by a rejected password"
        );
    }

    #[test]
    fn tampering_with_the_ciphertext_is_detected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store(&dir);
        let (doc, _) = seeded_document();
        store
            .create(&password(), &doc, KdfParameters::testing())
            .expect("create");

        let mut bytes = fs::read(store.path()).expect("read");
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        fs::write(store.path(), &bytes).expect("write");
        assert!(matches!(store.open(&password()), Err(VaultError::BadPassword)));
    }

    #[test]
    fn tampering_with_the_authenticated_header_is_detected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store(&dir);
        let (doc, _) = seeded_document();
        store
            .create(&password(), &doc, KdfParameters::testing())
            .expect("create");

        // Flip one bit of the salt. The header parses, the derived key changes,
        // and the AEAD rejects it.
        let mut bytes = fs::read(store.path()).expect("read");
        bytes[24] ^= 0x01;
        fs::write(store.path(), &bytes).expect("write");
        assert!(matches!(store.open(&password()), Err(VaultError::BadPassword)));

        // Flip one bit of the nonce: same outcome, via the AAD check.
        let mut bytes = fs::read(store.path()).expect("read");
        bytes[40] ^= 0x01;
        fs::write(store.path(), &bytes).expect("write");
        assert!(matches!(store.open(&password()), Err(VaultError::BadPassword)));
    }

    #[test]
    fn a_truncated_file_is_corrupt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store(&dir);
        let (doc, _) = seeded_document();
        store
            .create(&password(), &doc, KdfParameters::testing())
            .expect("create");

        let bytes = fs::read(store.path()).expect("read");
        fs::write(store.path(), &bytes[..HEADER_LEN + 4]).expect("write");
        assert!(matches!(store.open(&password()), Err(VaultError::Corrupt { .. })));

        fs::write(store.path(), &bytes[..10]).expect("write");
        assert!(matches!(store.open(&password()), Err(VaultError::Corrupt { .. })));

        fs::write(store.path(), b"").expect("write");
        assert!(matches!(store.open(&password()), Err(VaultError::Corrupt { .. })));
    }

    #[test]
    fn an_unrelated_file_is_corrupt_not_a_bad_password() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store(&dir);
        fs::create_dir_all(store.path().parent().expect("parent")).expect("mkdir");
        fs::write(store.path(), vec![0x42u8; 4096]).expect("write");
        assert!(matches!(store.open(&password()), Err(VaultError::Corrupt { .. })));
    }

    #[test]
    fn each_save_uses_a_fresh_nonce_and_keeps_the_salt() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store(&dir);
        let (mut doc, _) = seeded_document();
        let header = store
            .create(&password(), &doc, KdfParameters::testing())
            .expect("create");
        let (_, _, key) = store.open(&password()).expect("open");

        doc.insert(SecretKind::Note, SecretString::new("another"));
        let next = store.save_with_key(&header, &key, &doc).expect("save");
        assert_ne!(
            header.nonce, next.nonce,
            "nonce reuse would break confidentiality"
        );
        assert_eq!(
            header.salt, next.salt,
            "the salt is stable so the key stays valid"
        );

        let (_, reopened, _) = store.open(&password()).expect("reopen");
        assert_eq!(reopened.len(), 3);
    }

    #[test]
    fn saving_leaves_no_temporary_file_behind() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store(&dir);
        let (doc, _) = seeded_document();
        let header = store
            .create(&password(), &doc, KdfParameters::testing())
            .expect("create");
        let (_, _, key) = store.open(&password()).expect("open");
        store.save_with_key(&header, &key, &doc).expect("save");
        assert!(!temp_path_for(store.path()).exists());
    }

    #[test]
    fn changing_the_password_backs_up_and_re_keys() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store(&dir);
        let (doc, reference) = seeded_document();
        store
            .create(&password(), &doc, KdfParameters::testing())
            .expect("create");

        let next = SecretString::new("a different long passphrase");
        store
            .change_password(&password(), &next, KdfParameters::testing())
            .expect("re-key");

        assert!(matches!(store.open(&password()), Err(VaultError::BadPassword)));
        let (_, reopened, _) = store.open(&next).expect("open with the new password");
        assert_eq!(
            reopened.get(reference).expect("present").value.expose(),
            "hunter2"
        );

        let backup = backup_path_for(store.path());
        assert!(backup.is_file(), "a backup must exist before a re-key");
        let restored = VaultStore::new(&backup);
        assert!(
            restored.open(&password()).is_ok(),
            "the backup still opens with the old password"
        );
    }

    #[test]
    fn changing_the_password_refuses_a_wrong_current_password() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store(&dir);
        let (doc, _) = seeded_document();
        store
            .create(&password(), &doc, KdfParameters::testing())
            .expect("create");
        let err = store
            .change_password(
                &SecretString::new("not the password"),
                &SecretString::new("a different long passphrase"),
                KdfParameters::testing(),
            )
            .expect_err("must fail");
        assert!(matches!(err, VaultError::BadPassword));
        assert!(store.open(&password()).is_ok(), "the vault is untouched");
        assert!(
            !backup_path_for(store.path()).exists(),
            "no backup for a rejected attempt"
        );
    }

    #[test]
    fn the_header_can_be_inspected_while_locked() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store = store(&dir);
        let (doc, _) = seeded_document();
        store
            .create(&password(), &doc, KdfParameters::testing())
            .expect("create");
        let header = store.peek_header().expect("peek");
        assert_eq!(header.version, crate::FORMAT_VERSION);
        assert_eq!(header.kdf, KdfParameters::testing());
    }
}
