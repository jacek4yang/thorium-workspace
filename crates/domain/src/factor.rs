//! Second factors attached to an account.
//!
//! Standards-based TOTP/HOTP parameters are metadata (non-secret) and are
//! persisted in SQLite. The seed itself lives in the encrypted vault behind
//! a [`crate::SecretRef`].

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{AccountId, FactorId};
use crate::secret_ref::SecretRef;

/// Hash algorithm for HOTP/TOTP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum OtpAlgorithm {
    /// HMAC-SHA-1 (RFC 4226 default).
    Sha1,
    /// HMAC-SHA-256 (RFC 6238 extension).
    Sha256,
    /// HMAC-SHA-512 (RFC 6238 extension).
    Sha512,
}

impl OtpAlgorithm {
    /// Stable storage identifier.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Sha1 => "SHA1",
            Self::Sha256 => "SHA256",
            Self::Sha512 => "SHA512",
        }
    }

    /// Reconstructs the algorithm from its storage identifier.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "SHA1" => Some(Self::Sha1),
            "SHA256" => Some(Self::Sha256),
            "SHA512" => Some(Self::Sha512),
            _ => None,
        }
    }
}

/// Kind of a second factor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactorKind {
    /// RFC 6238 time-based OTP.
    Totp,
    /// RFC 4226 counter-based OTP.
    Hotp,
    /// An external authenticator (e.g. a hardware key or a phone-based
    /// push/number-matching system such as Microsoft Authenticator). The
    /// workspace never emulates these; it only records their existence.
    ExternalAuthenticator,
}

impl FactorKind {
    /// Stable storage identifier.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Totp => "totp",
            Self::Hotp => "hotp",
            Self::ExternalAuthenticator => "external",
        }
    }

    /// Reconstructs the kind from its storage identifier.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "totp" => Some(Self::Totp),
            "hotp" => Some(Self::Hotp),
            "external" => Some(Self::ExternalAuthenticator),
            _ => None,
        }
    }
}

/// A second factor record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecondFactor {
    /// Unique identifier.
    pub id: FactorId,
    /// Owning account.
    pub account_id: AccountId,
    /// Kind of factor.
    pub kind: FactorKind,
    /// Optional user-facing label.
    pub label: Option<String>,
    /// Issuer label from the otpauth URI, when imported.
    pub issuer: Option<String>,
    /// Account label from the otpauth URI, when imported.
    pub account_label: Option<String>,
    /// Hash algorithm (TOTP/HOTP only).
    pub algorithm: Option<OtpAlgorithm>,
    /// Code digit count (TOTP/HOTP only): 6 or 8.
    pub digits: Option<u8>,
    /// TOTP period in seconds (TOTP only).
    pub period_seconds: Option<u32>,
    /// HOTP counter (HOTP only).
    pub counter: Option<u64>,
    /// Reference to the encrypted seed in the vault (TOTP/HOTP only).
    pub secret_ref: Option<SecretRef>,
    /// Free-form note describing an external authenticator.
    pub external_note: Option<String>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
}

/// Maximum TOTP period the workspace will honor.
pub const MAX_TOTP_PERIOD_SECONDS: u32 = 300;

/// Validates TOTP/HOTP metadata fields.
pub fn validate_factor_params(
    kind: FactorKind,
    algorithm: Option<OtpAlgorithm>,
    digits: Option<u8>,
    period_seconds: Option<u32>,
) -> Result<(), crate::error::DomainError> {
    use crate::error::DomainError;
    match kind {
        FactorKind::ExternalAuthenticator => Ok(()),
        FactorKind::Totp | FactorKind::Hotp => {
            let algorithm = algorithm.ok_or(DomainError::OutOfRange { field: "algorithm" })?;
            if !matches!(
                algorithm,
                OtpAlgorithm::Sha1 | OtpAlgorithm::Sha256 | OtpAlgorithm::Sha512
            ) {
                return Err(DomainError::OutOfRange { field: "algorithm" });
            }
            if !matches!(digits, Some(6) | Some(8)) {
                return Err(DomainError::OutOfRange { field: "digits" });
            }
            if kind == FactorKind::Totp {
                let period = period_seconds.ok_or(DomainError::OutOfRange { field: "period" })?;
                if period == 0 || period > MAX_TOTP_PERIOD_SECONDS {
                    return Err(DomainError::OutOfRange { field: "period" });
                }
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factor_kinds_roundtrip() {
        for kind in [
            FactorKind::Totp,
            FactorKind::Hotp,
            FactorKind::ExternalAuthenticator,
        ] {
            let rebuilt = FactorKind::from_id(kind.id()).expect("roundtrip");
            assert_eq!(kind, rebuilt);
        }
    }

    #[test]
    fn algorithms_roundtrip() {
        for algorithm in [
            OtpAlgorithm::Sha1,
            OtpAlgorithm::Sha256,
            OtpAlgorithm::Sha512,
        ] {
            let rebuilt = OtpAlgorithm::from_id(algorithm.id()).expect("roundtrip");
            assert_eq!(algorithm, rebuilt);
        }
        assert!(OtpAlgorithm::from_id("MD5").is_none());
    }

    #[test]
    fn totp_params_validate() {
        assert!(
            validate_factor_params(
                FactorKind::Totp,
                Some(OtpAlgorithm::Sha1),
                Some(6),
                Some(30)
            )
            .is_ok()
        );
        assert!(
            validate_factor_params(
                FactorKind::Totp,
                Some(OtpAlgorithm::Sha512),
                Some(8),
                Some(60)
            )
            .is_ok()
        );
        // Missing algorithm, bad digits, bad period.
        assert!(validate_factor_params(FactorKind::Totp, None, Some(6), Some(30)).is_err());
        assert!(
            validate_factor_params(
                FactorKind::Totp,
                Some(OtpAlgorithm::Sha1),
                Some(7),
                Some(30)
            )
            .is_err()
        );
        assert!(
            validate_factor_params(FactorKind::Totp, Some(OtpAlgorithm::Sha1), Some(6), Some(0))
                .is_err()
        );
        assert!(
            validate_factor_params(
                FactorKind::Totp,
                Some(OtpAlgorithm::Sha1),
                Some(6),
                Some(MAX_TOTP_PERIOD_SECONDS + 1)
            )
            .is_err()
        );
    }

    #[test]
    fn hotp_ignores_period_but_needs_core_fields() {
        assert!(
            validate_factor_params(FactorKind::Hotp, Some(OtpAlgorithm::Sha1), Some(6), None)
                .is_ok()
        );
        assert!(validate_factor_params(FactorKind::Hotp, None, Some(6), None).is_err());
    }

    #[test]
    fn external_authenticator_has_no_parameter_requirements() {
        assert!(
            validate_factor_params(FactorKind::ExternalAuthenticator, None, None, None).is_ok()
        );
    }
}
