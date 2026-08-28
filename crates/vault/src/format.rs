//! The on-disk vault header and the key derivation it describes.

use argon2::{Algorithm, Argon2, Params, Version};
use tw_secrets::SecretBytes;
use zeroize::Zeroize;

use crate::{VaultError, VaultResult};

/// File magic. Changing this invalidates every existing vault, so it never
/// changes; the version field carries format evolution instead.
pub const MAGIC: &[u8; 8] = b"TWVAULT1";

/// Current vault format version.
pub const FORMAT_VERSION: u16 = 1;

/// Size of the cleartext header in bytes. Also the AEAD associated data length.
pub const HEADER_LEN: usize = 64;

/// Argon2 salt length in bytes.
pub const SALT_LEN: usize = 16;

/// XChaCha20-Poly1305 nonce length in bytes.
pub const NONCE_LEN: usize = 24;

/// Derived key length in bytes.
pub const KEY_LEN: usize = 32;

/// Poly1305 authentication tag length in bytes.
pub const TAG_LEN: usize = 16;

/// KDF identifier for Argon2id, Argon2 version 0x13.
pub const KDF_ARGON2ID: u8 = 1;

/// Cipher identifier for XChaCha20-Poly1305.
pub const CIPHER_XCHACHA20_POLY1305: u8 = 1;

/// Default Argon2id memory cost in KiB (64 MiB).
///
/// Chosen to sit above the OWASP minimum while still unlocking in well under a
/// second on a low-end laptop. Stored per-vault so it can be raised later
/// without breaking existing files.
pub const ARGON2_DEFAULT_MEMORY_KIB: u32 = 64 * 1024;

/// Default Argon2id time cost (passes).
pub const ARGON2_DEFAULT_TIME_COST: u32 = 3;

/// Default Argon2id parallelism.
pub const ARGON2_DEFAULT_PARALLELISM: u32 = 1;

/// Lowest memory cost accepted when reading a vault, in KiB (8 MiB).
///
/// A file claiming a cheaper KDF than this is rejected rather than honoured: the
/// header is authenticated, but an attacker who has the file *and* can persuade
/// the user to keep using it should not be able to weaken future re-saves.
pub const ARGON2_MIN_MEMORY_KIB: u32 = 8 * 1024;

/// Lowest time cost accepted when reading a vault.
pub const ARGON2_MIN_TIME_COST: u32 = 1;

/// Highest memory cost accepted, in KiB (2 GiB).
///
/// Bounds a hostile header that would otherwise make the app allocate until the
/// machine dies.
pub const ARGON2_MAX_MEMORY_KIB: u32 = 2 * 1024 * 1024;

/// Highest time cost accepted.
pub const ARGON2_MAX_TIME_COST: u32 = 64;

/// Highest parallelism accepted.
pub const ARGON2_MAX_PARALLELISM: u32 = 16;

/// Argon2id cost parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParameters {
    /// Memory cost in KiB.
    pub memory_kib: u32,
    /// Number of passes.
    pub time_cost: u32,
    /// Degree of parallelism.
    pub parallelism: u32,
}

impl Default for KdfParameters {
    fn default() -> Self {
        Self {
            memory_kib: ARGON2_DEFAULT_MEMORY_KIB,
            time_cost: ARGON2_DEFAULT_TIME_COST,
            parallelism: ARGON2_DEFAULT_PARALLELISM,
        }
    }
}

impl KdfParameters {
    /// Cheap parameters for tests only.
    ///
    /// Never used by the application: [`VaultHeader::validate`] rejects anything
    /// this weak when reading a file, so a vault written with these parameters
    /// cannot be opened by the shipped product.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn testing() -> Self {
        Self {
            memory_kib: ARGON2_MIN_MEMORY_KIB,
            time_cost: 1,
            parallelism: 1,
        }
    }

    fn validate(self) -> VaultResult<()> {
        let ok = (ARGON2_MIN_MEMORY_KIB..=ARGON2_MAX_MEMORY_KIB).contains(&self.memory_kib)
            && (ARGON2_MIN_TIME_COST..=ARGON2_MAX_TIME_COST).contains(&self.time_cost)
            && (1..=ARGON2_MAX_PARALLELISM).contains(&self.parallelism);
        if ok {
            Ok(())
        } else {
            Err(VaultError::Corrupt {
                reason: "key derivation parameters are out of range".to_owned(),
            })
        }
    }
}

/// The cleartext, authenticated header of a vault file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultHeader {
    /// Format version.
    pub version: u16,
    /// KDF identifier.
    pub kdf_id: u8,
    /// Cipher identifier.
    pub cipher_id: u8,
    /// Argon2id cost parameters.
    pub kdf: KdfParameters,
    /// Argon2id salt.
    pub salt: [u8; SALT_LEN],
    /// XChaCha20 nonce.
    pub nonce: [u8; NONCE_LEN],
}

impl VaultHeader {
    /// Builds a header for a fresh save, with a new random salt and nonce.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::KeyDerivation`] when the system entropy source is
    /// unavailable.
    pub fn new_random(kdf: KdfParameters) -> VaultResult<Self> {
        let mut salt = [0u8; SALT_LEN];
        let mut nonce = [0u8; NONCE_LEN];
        tw_secrets::fill_random(&mut salt).map_err(|_| VaultError::KeyDerivation)?;
        tw_secrets::fill_random(&mut nonce).map_err(|_| VaultError::KeyDerivation)?;
        Ok(Self {
            version: FORMAT_VERSION,
            kdf_id: KDF_ARGON2ID,
            cipher_id: CIPHER_XCHACHA20_POLY1305,
            kdf,
            salt,
            nonce,
        })
    }

    /// Returns a copy with a freshly generated nonce.
    ///
    /// Every save must use a new nonce: reusing one under the same key would
    /// destroy XChaCha20-Poly1305's confidentiality guarantees.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::KeyDerivation`] when the system entropy source is
    /// unavailable.
    pub fn with_fresh_nonce(&self) -> VaultResult<Self> {
        let mut nonce = [0u8; NONCE_LEN];
        tw_secrets::fill_random(&mut nonce).map_err(|_| VaultError::KeyDerivation)?;
        Ok(Self {
            nonce,
            ..self.clone()
        })
    }

    /// Serializes the header to its exact 64-byte on-disk form.
    #[must_use]
    pub fn to_bytes(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[0..8].copy_from_slice(MAGIC);
        out[8..10].copy_from_slice(&self.version.to_le_bytes());
        out[10] = self.kdf_id;
        out[11] = self.cipher_id;
        out[12..16].copy_from_slice(&self.kdf.memory_kib.to_le_bytes());
        out[16..20].copy_from_slice(&self.kdf.time_cost.to_le_bytes());
        out[20..24].copy_from_slice(&self.kdf.parallelism.to_le_bytes());
        out[24..40].copy_from_slice(&self.salt);
        out[40..64].copy_from_slice(&self.nonce);
        out
    }

    /// Parses a header from the first [`HEADER_LEN`] bytes of a vault file.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Corrupt`] when the input is too short, has the
    /// wrong magic or names an unsupported algorithm, and
    /// [`VaultError::FormatTooNew`] when the version is beyond this build.
    pub fn parse(bytes: &[u8]) -> VaultResult<Self> {
        if bytes.len() < HEADER_LEN {
            return Err(VaultError::Corrupt {
                reason: "file is shorter than the vault header".to_owned(),
            });
        }
        if &bytes[0..8] != MAGIC {
            return Err(VaultError::Corrupt {
                reason: "file does not start with the vault magic".to_owned(),
            });
        }
        let version = u16::from_le_bytes([bytes[8], bytes[9]]);
        if version > FORMAT_VERSION {
            return Err(VaultError::FormatTooNew { found: version });
        }
        if version == 0 {
            return Err(VaultError::Corrupt {
                reason: "vault format version 0 is not valid".to_owned(),
            });
        }
        let kdf_id = bytes[10];
        if kdf_id != KDF_ARGON2ID {
            return Err(VaultError::Corrupt {
                reason: format!("unsupported key derivation id {kdf_id}"),
            });
        }
        let cipher_id = bytes[11];
        if cipher_id != CIPHER_XCHACHA20_POLY1305 {
            return Err(VaultError::Corrupt {
                reason: format!("unsupported cipher id {cipher_id}"),
            });
        }
        let kdf = KdfParameters {
            memory_kib: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            time_cost: u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
            parallelism: u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]),
        };
        let mut salt = [0u8; SALT_LEN];
        salt.copy_from_slice(&bytes[24..40]);
        let mut nonce = [0u8; NONCE_LEN];
        nonce.copy_from_slice(&bytes[40..64]);
        let header = Self {
            version,
            kdf_id,
            cipher_id,
            kdf,
            salt,
            nonce,
        };
        header.validate()?;
        Ok(header)
    }

    /// Checks the header's parameters are within accepted bounds.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::Corrupt`] when a cost parameter is out of range.
    pub fn validate(&self) -> VaultResult<()> {
        self.kdf.validate()
    }

    /// Derives the file encryption key from `password` and this header's salt.
    ///
    /// # Errors
    ///
    /// Returns [`VaultError::KeyDerivation`] when Argon2 rejects the parameters
    /// or fails to allocate.
    pub fn derive_key(&self, password: &tw_secrets::SecretString) -> VaultResult<SecretBytes> {
        let params = Params::new(
            self.kdf.memory_kib,
            self.kdf.time_cost,
            self.kdf.parallelism,
            Some(KEY_LEN),
        )
        .map_err(|_| VaultError::KeyDerivation)?;
        let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut key = [0u8; KEY_LEN];
        let result = argon.hash_password_into(password.expose_bytes(), &self.salt, &mut key);
        match result {
            Ok(()) => {
                let derived = SecretBytes::new(key.to_vec());
                key.zeroize();
                Ok(derived)
            }
            Err(_) => {
                key.zeroize();
                Err(VaultError::KeyDerivation)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> VaultHeader {
        VaultHeader::new_random(KdfParameters::testing()).expect("entropy")
    }

    #[test]
    fn headers_round_trip_byte_for_byte() {
        let original = header();
        let bytes = original.to_bytes();
        assert_eq!(bytes.len(), HEADER_LEN);
        assert_eq!(&bytes[0..8], MAGIC);
        assert_eq!(VaultHeader::parse(&bytes).expect("parse"), original);
    }

    #[test]
    fn a_short_file_is_corrupt_not_a_panic() {
        for len in 0..HEADER_LEN {
            let err = VaultHeader::parse(&vec![0u8; len]).expect_err("too short");
            assert!(matches!(err, VaultError::Corrupt { .. }));
        }
    }

    #[test]
    fn foreign_files_are_rejected_by_magic() {
        let mut bytes = header().to_bytes();
        bytes[0] = b'X';
        let err = VaultHeader::parse(&bytes).expect_err("bad magic");
        assert!(matches!(err, VaultError::Corrupt { .. }));
    }

    #[test]
    fn a_newer_format_version_is_reported_distinctly() {
        let mut bytes = header().to_bytes();
        bytes[8..10].copy_from_slice(&(FORMAT_VERSION + 1).to_le_bytes());
        match VaultHeader::parse(&bytes) {
            Err(VaultError::FormatTooNew { found }) => assert_eq!(found, FORMAT_VERSION + 1),
            other => panic!("expected FormatTooNew, got {other:?}"),
        }
    }

    #[test]
    fn unsupported_algorithms_are_rejected() {
        let mut bytes = header().to_bytes();
        bytes[10] = 42;
        assert!(matches!(
            VaultHeader::parse(&bytes),
            Err(VaultError::Corrupt { .. })
        ));

        let mut bytes = header().to_bytes();
        bytes[11] = 42;
        assert!(matches!(
            VaultHeader::parse(&bytes),
            Err(VaultError::Corrupt { .. })
        ));
    }

    #[test]
    fn a_downgraded_kdf_cost_is_rejected() {
        let mut bytes = header().to_bytes();
        bytes[12..16].copy_from_slice(&1024u32.to_le_bytes()); // 1 MiB
        assert!(matches!(
            VaultHeader::parse(&bytes),
            Err(VaultError::Corrupt { .. })
        ));

        let mut bytes = header().to_bytes();
        bytes[16..20].copy_from_slice(&0u32.to_le_bytes()); // zero passes
        assert!(matches!(
            VaultHeader::parse(&bytes),
            Err(VaultError::Corrupt { .. })
        ));
    }

    #[test]
    fn an_absurd_memory_cost_is_rejected_before_allocation() {
        let mut bytes = header().to_bytes();
        bytes[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            VaultHeader::parse(&bytes),
            Err(VaultError::Corrupt { .. })
        ));
    }

    #[test]
    fn every_header_gets_a_distinct_salt_and_nonce() {
        let a = header();
        let b = header();
        assert_ne!(a.salt, b.salt);
        assert_ne!(a.nonce, b.nonce);
        assert_ne!(a.nonce, a.with_fresh_nonce().expect("entropy").nonce);
        assert_eq!(
            a.salt,
            a.with_fresh_nonce().expect("entropy").salt,
            "a re-save keeps the salt"
        );
    }

    #[test]
    fn the_same_password_and_salt_derive_the_same_key() {
        let h = header();
        let pw = tw_secrets::SecretString::new("correct horse battery staple");
        let k1 = h.derive_key(&pw).expect("derive");
        let k2 = h.derive_key(&pw).expect("derive");
        assert_eq!(k1, k2);
        assert_eq!(k1.len(), KEY_LEN);

        let other = tw_secrets::SecretString::new("incorrect horse battery staple");
        assert_ne!(k1, h.derive_key(&other).expect("derive"));

        let h2 = header();
        assert_ne!(
            k1,
            h2.derive_key(&pw).expect("derive"),
            "a different salt yields a different key"
        );
    }

    #[test]
    fn production_defaults_satisfy_the_read_side_bounds() {
        let defaults = KdfParameters::default();
        assert!(defaults.validate().is_ok());
        assert!(defaults.memory_kib >= ARGON2_MIN_MEMORY_KIB);
        assert!(defaults.time_cost >= ARGON2_MIN_TIME_COST);
    }
}
