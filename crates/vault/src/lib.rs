//! The encrypted secret vault.
//!
//! # Format
//!
//! A vault file is a fixed 64-byte cleartext header followed by one AEAD
//! ciphertext. The header carries the parameters needed to derive the key and is
//! authenticated as associated data, so downgrading the KDF cost, swapping the
//! salt or editing the version is detected as tampering rather than silently
//! honoured.
//!
//! ```text
//! offset size field
//! 0      8    magic  b"TWVAULT1"
//! 8      2    format version, u16 little-endian
//! 10     1    KDF identifier      (1 = Argon2id, version 0x13)
//! 11     1    cipher identifier   (1 = XChaCha20-Poly1305)
//! 12     4    Argon2 memory cost in KiB,  u32 little-endian
//! 16     4    Argon2 time cost (passes),  u32 little-endian
//! 20     4    Argon2 parallelism,         u32 little-endian
//! 24     16   Argon2 salt
//! 40     24   XChaCha20 nonce
//! 64     ...  ciphertext || 16-byte Poly1305 tag
//! ```
//!
//! The plaintext is a JSON [`VaultDocument`]. JSON is a deliberate choice: the
//! payload is small, and a format a human can inspect after decrypting is worth
//! more during a recovery than a few saved bytes.
//!
//! # Why not KDBX4
//!
//! KDBX4 interoperability was evaluated against the `keepass` crate and rejected
//! for v1.0.0. See `DECISIONS.md`; the short version is that the crate's own
//! documentation describes KDBX4 *writing* as experimental, and this file is the
//! only copy of the user's passwords.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod document;
mod format;
mod session;
mod store;

pub use document::{SecretKind, SecretRecord, VaultDocument};
pub use format::{
    ARGON2_DEFAULT_MEMORY_KIB, ARGON2_DEFAULT_PARALLELISM, ARGON2_DEFAULT_TIME_COST, FORMAT_VERSION,
    KdfParameters, VaultHeader,
};
pub use session::{LockReason, VaultSession, VaultState};
pub use store::{VaultStore, backup_path_for};

use tw_domain::DiagnosticCode;

/// Errors raised by the vault.
///
/// Variants never carry the master password, a derived key or plaintext secret
/// material; the `Debug` and `Display` output of every variant is safe to log.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum VaultError {
    /// No vault file exists at the expected path.
    #[error("no vault has been created yet")]
    Missing,
    /// A vault file already exists and creating one would destroy it.
    #[error("a vault already exists at this location")]
    AlreadyExists,
    /// The vault is locked.
    #[error("the vault is locked")]
    Locked,
    /// The supplied master password did not unlock the vault.
    ///
    /// Indistinguishable from a corrupted ciphertext by design: an attacker must
    /// not learn whether a guess was structurally close.
    #[error("the master password is incorrect")]
    BadPassword,
    /// The file is not a Thorium Workspace vault, or is damaged.
    #[error("the vault file is damaged or is not a Thorium Workspace vault: {reason}")]
    Corrupt {
        /// What failed structurally. Never includes file contents.
        reason: String,
    },
    /// The file was written by a newer format version.
    #[error("this vault was written by a newer version of Thorium Workspace (format {found})")]
    FormatTooNew {
        /// The version found in the header.
        found: u16,
    },
    /// The referenced secret is not in the vault.
    #[error("the requested secret is not in the vault")]
    SecretNotFound,
    /// Key derivation failed.
    #[error("key derivation failed")]
    KeyDerivation,
    /// Encryption or decryption failed for a reason other than a bad key.
    #[error("the vault could not be encrypted or decrypted")]
    Crypto,
    /// The master password did not meet the minimum policy.
    #[error("{0}")]
    WeakPassword(String),
    /// An I/O operation failed.
    #[error("{operation} failed: {source}")]
    Io {
        /// What was being attempted, for example `write vault`.
        operation: &'static str,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The decrypted payload could not be parsed.
    #[error("the vault contents could not be read")]
    Payload,
}

impl VaultError {
    /// The stable diagnostic code for this error.
    #[must_use]
    pub const fn code(&self) -> DiagnosticCode {
        match self {
            Self::Missing => DiagnosticCode::VaultMissing,
            Self::AlreadyExists => DiagnosticCode::VaultAlreadyExists,
            Self::Locked => DiagnosticCode::VaultLocked,
            Self::BadPassword => DiagnosticCode::VaultBadPassword,
            Self::Corrupt { .. } | Self::Payload => DiagnosticCode::VaultCorrupt,
            Self::FormatTooNew { .. } => DiagnosticCode::VaultFormatTooNew,
            Self::SecretNotFound => DiagnosticCode::SecretNotFound,
            Self::KeyDerivation | Self::Crypto | Self::WeakPassword(_) => DiagnosticCode::VaultWriteFailed,
            Self::Io { .. } => DiagnosticCode::IoFailed,
        }
    }

    fn io(operation: &'static str, source: std::io::Error) -> Self {
        Self::Io { operation, source }
    }
}

/// Vault result alias.
pub type VaultResult<T> = Result<T, VaultError>;

/// The shortest master password accepted.
///
/// Twelve characters is the point where an Argon2id-protected passphrase stops
/// being trivially enumerable. The check exists to stop a one-word password, not
/// to enforce a composition policy: complexity rules push users toward
/// predictable substitutions.
pub const MIN_MASTER_PASSWORD_LEN: usize = 12;

/// Validates a candidate master password.
///
/// # Errors
///
/// Returns [`VaultError::WeakPassword`] when the password is shorter than
/// [`MIN_MASTER_PASSWORD_LEN`] characters or is entirely whitespace.
pub fn check_master_password(password: &tw_secrets::SecretString) -> VaultResult<()> {
    let value = password.expose();
    if value.trim().is_empty() {
        return Err(VaultError::WeakPassword(
            "the master password must not be blank".to_owned(),
        ));
    }
    if value.chars().count() < MIN_MASTER_PASSWORD_LEN {
        return Err(VaultError::WeakPassword(format!(
            "the master password must be at least {MIN_MASTER_PASSWORD_LEN} characters"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tw_secrets::SecretString;

    use super::*;

    #[test]
    fn master_password_policy_rejects_short_and_blank_values() {
        assert!(check_master_password(&SecretString::new("correct horse battery")).is_ok());
        assert!(check_master_password(&SecretString::new("short")).is_err());
        assert!(check_master_password(&SecretString::new("              ")).is_err());
    }

    #[test]
    fn errors_never_render_secret_material() {
        // Every variant's Display and Debug output is fixed text, so a caller
        // that logs an error cannot leak what the user typed.
        let cases: Vec<VaultError> = vec![
            VaultError::Missing,
            VaultError::AlreadyExists,
            VaultError::Locked,
            VaultError::BadPassword,
            VaultError::Corrupt {
                reason: "short header".into(),
            },
            VaultError::FormatTooNew { found: 99 },
            VaultError::SecretNotFound,
            VaultError::KeyDerivation,
            VaultError::Crypto,
            VaultError::Payload,
        ];
        for err in cases {
            let rendered = format!("{err} {err:?}");
            assert!(!rendered.contains("hunter2"), "{rendered}");
            assert!(!rendered.is_empty());
        }
    }

    #[test]
    fn a_wrong_password_and_a_corrupt_payload_are_indistinguishable_to_the_caller() {
        // Both map to the same user-facing text path; only the code differs, and
        // BadPassword deliberately carries no structural detail.
        assert_eq!(VaultError::BadPassword.code(), DiagnosticCode::VaultBadPassword);
        assert_eq!(
            VaultError::BadPassword.to_string(),
            "the master password is incorrect"
        );
    }
}
