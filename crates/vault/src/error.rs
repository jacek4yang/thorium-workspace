//! Typed vault errors with stable diagnostic codes.
//!
//! Error text never contains secret material: no variant embeds a master
//! password, seed, or entry value. File paths are included because they
//! are needed to diagnose portable-workspace problems.

use std::path::PathBuf;

use thorium_workspace_domain::DiagnosticCode;

/// Error type for all vault operations.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    /// A filesystem operation on the vault file failed.
    #[error("vault file operation failed for {path}: {source}")]
    Io {
        /// Vault file path.
        path: PathBuf,
        /// Underlying filesystem error.
        source: std::io::Error,
    },

    /// A vault already exists at the target path.
    #[error("a vault already exists; unlock it instead of creating a new one")]
    AlreadyExists,

    /// The operation requires an unlocked vault.
    #[error("the vault is locked; unlock it first")]
    Locked,

    /// The operation requires a locked vault (or no vault at all).
    #[error("the vault is already unlocked")]
    AlreadyUnlocked,

    /// The vault file does not parse (bad magic, truncated, or unknown
    /// format version).
    #[error("vault file is corrupt: {detail}")]
    Corrupt {
        /// Why the file was rejected.
        detail: String,
    },

    /// Unlocking failed. A wrong master password and a damaged vault file
    /// are cryptographically indistinguishable (AEAD tag mismatch), so
    /// both map here.
    #[error("unlocking failed: the master password is wrong or the vault file is damaged")]
    UnlockFailed,

    /// The encrypted payload did not deserialize.
    #[error("vault payload is invalid: {0}")]
    Payload(String),

    /// Key derivation parameters are invalid.
    #[error("key derivation parameters are invalid: {0}")]
    Kdf(String),

    /// The operating system random source failed.
    #[error("secure random source failed: {0}")]
    RandomSource(String),
}

impl DiagnosticCode for VaultError {
    fn diagnostic_code(&self) -> &'static str {
        match self {
            Self::Io { .. } => "VAULT_IO_FAILED",
            Self::AlreadyExists => "VAULT_ALREADY_EXISTS",
            Self::Locked => "VAULT_LOCKED",
            Self::AlreadyUnlocked => "VAULT_ALREADY_UNLOCKED",
            Self::Corrupt { .. } => "VAULT_CORRUPT",
            Self::UnlockFailed => "VAULT_UNLOCK_FAILED",
            Self::Payload(_) => "VAULT_PAYLOAD_INVALID",
            Self::Kdf(_) => "VAULT_KDF_INVALID",
            Self::RandomSource(_) => "VAULT_RANDOM_FAILED",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlock_failure_does_not_embed_material() {
        let error = VaultError::UnlockFailed;
        let rendered = format!("{error} ({error:?})");
        assert!(!rendered.to_lowercase().contains("password="));
        assert_eq!(error.diagnostic_code(), "VAULT_UNLOCK_FAILED");
    }
}
