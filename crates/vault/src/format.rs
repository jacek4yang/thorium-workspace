//! On-disk vault file format (version 1).
//!
//! Layout (all integers little-endian):
//!
//! ```text
//! offset  size  field
//! 0       8     magic  ("THWSVLT!")
//! 8       4     format version (u32)
//! 12      32    Argon2id salt
//! 44      4     KDF memory cost (KiB)
//! 48      4     KDF time cost (iterations)
//! 52      4     KDF parallelism
//! 56      12    ChaCha20-Poly1305 nonce
//! 68      ...   ciphertext (payload JSON + 16-byte Poly1305 tag)
//! ```
//!
//! Bytes 0..68 form the header and are authenticated as AEAD additional
//! data. Tampering with any header byte (including KDF parameters — a
//! downgrade attempt) breaks authentication.

use crate::error::VaultError;

/// File magic identifying a Thorium Workspace vault.
pub(crate) const MAGIC: &[u8; 8] = b"THWSVLT!";

/// Current on-disk format version.
pub(crate) const FORMAT_VERSION: u32 = 1;

/// Argon2id salt length.
pub(crate) const SALT_LEN: usize = 32;

/// ChaCha20-Poly1305 nonce length.
pub(crate) const NONCE_LEN: usize = 12;

/// Size of the fixed header (magic..nonce, before ciphertext).
pub(crate) const HEADER_LEN: usize = 8 + 4 + SALT_LEN + 4 + 4 + 4 + NONCE_LEN;

/// Argon2id parameters stored in the header.
///
/// Defaults follow current OWASP guidance for Argon2id on a desktop
/// machine (64 MiB, 3 iterations, parallelism 1): fast enough for a
/// once-per-session unlock, memory-hard against GPU cracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KdfParams {
    /// Memory cost in KiB.
    pub m_cost_kib: u32,
    /// Time cost in iterations.
    pub t_cost: u32,
    /// Degree of parallelism.
    pub parallelism: u32,
}

impl Default for KdfParams {
    fn default() -> Self {
        Self {
            m_cost_kib: 65_536,
            t_cost: 3,
            parallelism: 1,
        }
    }
}

/// Parsed vault header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VaultHeader {
    /// Random per-vault KDF salt.
    pub salt: [u8; SALT_LEN],
    /// Stored KDF parameters (authenticated).
    pub kdf: KdfParams,
    /// Random per-save AEAD nonce.
    pub nonce: [u8; NONCE_LEN],
}

impl VaultHeader {
    /// Serializes the header; the returned bytes are also the AEAD AAD.
    pub(crate) fn encoded(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&self.salt);
        out.extend_from_slice(&self.kdf.m_cost_kib.to_le_bytes());
        out.extend_from_slice(&self.kdf.t_cost.to_le_bytes());
        out.extend_from_slice(&self.kdf.parallelism.to_le_bytes());
        out.extend_from_slice(&self.nonce);
        debug_assert_eq!(out.len(), HEADER_LEN);
        out
    }

    /// Parses and validates a header from the front of `bytes`.
    pub(crate) fn from_bytes(bytes: &[u8]) -> Result<Self, VaultError> {
        if bytes.len() < HEADER_LEN {
            return Err(VaultError::Corrupt {
                detail: format!("file too small for a vault header: {} bytes", bytes.len()),
            });
        }
        if &bytes[0..8] != MAGIC {
            return Err(VaultError::Corrupt {
                detail: "bad magic; not a Thorium Workspace vault".to_owned(),
            });
        }
        let version = u32::from_le_bytes(bytes[8..12].try_into().expect("fixed slice"));
        if version != FORMAT_VERSION {
            return Err(VaultError::Corrupt {
                detail: format!("unsupported format version {version}"),
            });
        }
        let mut header = Self {
            salt: [0; SALT_LEN],
            kdf: KdfParams::default(),
            nonce: [0; NONCE_LEN],
        };
        header.salt.copy_from_slice(&bytes[12..12 + SALT_LEN]);
        header.kdf.m_cost_kib = u32::from_le_bytes(bytes[44..48].try_into().expect("fixed slice"));
        header.kdf.t_cost = u32::from_le_bytes(bytes[48..52].try_into().expect("fixed slice"));
        header.kdf.parallelism = u32::from_le_bytes(bytes[52..56].try_into().expect("fixed slice"));
        header.nonce.copy_from_slice(&bytes[56..56 + NONCE_LEN]);
        Ok(header)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_roundtrips() {
        let header = VaultHeader {
            salt: [7; SALT_LEN],
            kdf: KdfParams {
                m_cost_kib: 32_768,
                t_cost: 2,
                parallelism: 4,
            },
            nonce: [9; NONCE_LEN],
        };
        let bytes = header.encoded();
        assert_eq!(bytes.len(), HEADER_LEN);
        assert_eq!(VaultHeader::from_bytes(&bytes).expect("parse"), header);
    }

    #[test]
    fn bad_magic_and_truncation_are_rejected() {
        let header = VaultHeader {
            salt: [1; SALT_LEN],
            kdf: KdfParams::default(),
            nonce: [2; NONCE_LEN],
        };
        let mut bytes = header.encoded();
        bytes[0] = b'X';
        assert!(matches!(
            VaultHeader::from_bytes(&bytes),
            Err(VaultError::Corrupt { .. })
        ));

        let truncated = &header.encoded()[..HEADER_LEN - 5];
        assert!(matches!(
            VaultHeader::from_bytes(truncated),
            Err(VaultError::Corrupt { .. })
        ));
    }

    #[test]
    fn unknown_versions_are_rejected() {
        let header = VaultHeader {
            salt: [3; SALT_LEN],
            kdf: KdfParams::default(),
            nonce: [4; NONCE_LEN],
        };
        let mut bytes = header.encoded();
        bytes[8..12].copy_from_slice(&99u32.to_le_bytes());
        let error = VaultHeader::from_bytes(&bytes).expect_err("version must be rejected");
        let rendered = format!("{error}");
        assert!(rendered.contains("99"), "got: {rendered}");
    }
}
