//! Vault references.
//!
//! A `SecretRef` names a secret stored inside the encrypted vault. The
//! reference itself is not secret: it is persisted as plaintext metadata in
//! SQLite (the contract explicitly permits "encrypted password reference"
//! fields). The secret value behind the reference never leaves the vault
//! without an explicit reveal/copy operation.

use std::fmt;
use std::str::FromStr;

use crate::error::DomainError;

/// Reference to a secret stored in the encrypted vault.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct SecretRef(String);

impl SecretRef {
    /// Builds a reference for an account password.
    pub fn for_password(account_id: &crate::ids::AccountId) -> Self {
        Self(format!("account/{}/password", account_id.as_uuid()))
    }

    /// Builds a reference for an OTP seed.
    pub fn for_otp_seed(factor_id: &crate::ids::FactorId) -> Self {
        Self(format!("factor/{}/seed", factor_id.as_uuid()))
    }

    /// Builds a reference for a single recovery code.
    pub fn for_recovery_code(code_id: &crate::ids::RecoveryCodeId) -> Self {
        Self(format!("recovery/{}/value", code_id.as_uuid()))
    }

    /// Returns the raw reference string (non-secret).
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for SecretRef {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() || s.len() > 200 || s.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return Err(DomainError::InvalidId);
        }
        if !s.starts_with("account/")
            && !s.starts_with("factor/")
            && !s.starts_with("recovery/")
        {
            return Err(DomainError::InvalidId);
        }
        Ok(Self(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{AccountId, FactorId, RecoveryCodeId};

    #[test]
    fn refs_are_structured_and_parseable() {
        let account = AccountId::new();
        let factor = FactorId::new();
        let code = RecoveryCodeId::new();

        let refs = [
            SecretRef::for_password(&account),
            SecretRef::for_otp_seed(&factor),
            SecretRef::for_recovery_code(&code),
        ];
        for reference in &refs {
            let parsed: SecretRef = reference.as_str().parse().expect("parseable");
            assert_eq!(&parsed, reference);
        }
    }

    #[test]
    fn refs_reject_unknown_prefixes_and_control_chars() {
        assert!("bogus/ref".parse::<SecretRef>().is_err());
        assert!("account/ x".parse::<SecretRef>().is_err());
        assert!("".parse::<SecretRef>().is_err());
    }

    #[test]
    fn refs_contain_no_secret_material() {
        // References embed only UUIDs; this pins the format so accidental
        // secret embedding (e.g. raw passwords) is caught in review.
        let reference = SecretRef::for_password(&AccountId::new());
        let text = reference.as_str();
        assert!(text.starts_with("account/"));
        assert!(text.ends_with("/password"));
        assert_eq!(text.matches('/').count(), 2);
    }
}
