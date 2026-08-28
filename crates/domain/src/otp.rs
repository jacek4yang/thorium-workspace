//! One-time-password parameters as defined by RFC 4226 (HOTP) and RFC 6238
//! (TOTP), plus the `otpauth://` key URI format.
//!
//! This module models parameters only. Code generation lives in `tw-otp` so the
//! domain stays free of cryptography.

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{DomainError, DomainResult};

/// Which OTP standard a factor uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OtpKind {
    /// Time-based, RFC 6238.
    #[default]
    Totp,
    /// Counter-based, RFC 4226.
    Hotp,
}

impl OtpKind {
    /// The `otpauth://` authority segment for this kind.
    #[must_use]
    pub const fn as_uri_type(self) -> &'static str {
        match self {
            Self::Totp => "totp",
            Self::Hotp => "hotp",
        }
    }
}

impl fmt::Display for OtpKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_uri_type())
    }
}

impl FromStr for OtpKind {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "totp" => Ok(Self::Totp),
            "hotp" => Ok(Self::Hotp),
            other => Err(DomainError::new(
                crate::DiagnosticCode::OtpUriInvalid,
                format!("unsupported OTP type '{other}'; expected totp or hotp"),
            )),
        }
    }
}

/// The HMAC hash used to derive codes.
///
/// RFC 6238 defines SHA-1, SHA-256 and SHA-512. SHA-1 remains the default
/// because it is what virtually every issuer provisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum OtpAlgorithm {
    /// HMAC-SHA-1 (RFC 4226 default).
    #[default]
    Sha1,
    /// HMAC-SHA-256.
    Sha256,
    /// HMAC-SHA-512.
    Sha512,
}

impl OtpAlgorithm {
    /// The canonical spelling used in `otpauth://` URIs.
    #[must_use]
    pub const fn as_uri_value(self) -> &'static str {
        match self {
            Self::Sha1 => "SHA1",
            Self::Sha256 => "SHA256",
            Self::Sha512 => "SHA512",
        }
    }
}

impl fmt::Display for OtpAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_uri_value())
    }
}

impl FromStr for OtpAlgorithm {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_uppercase().replace('-', "").as_str() {
            "SHA1" => Ok(Self::Sha1),
            "SHA256" => Ok(Self::Sha256),
            "SHA512" => Ok(Self::Sha512),
            other => Err(DomainError::new(
                crate::DiagnosticCode::OtpParametersInvalid,
                format!("unsupported OTP algorithm '{other}'; expected SHA1, SHA256 or SHA512"),
            )),
        }
    }
}

/// The number of digits in a generated code.
///
/// Restricted to the two values in real-world use; RFC 4226 permits 6-8 but 7 is
/// never provisioned and admitting it only widens the surface for typos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(into = "u8", try_from = "u8")]
pub enum OtpDigits {
    /// Six digits.
    #[default]
    Six,
    /// Eight digits.
    Eight,
}

impl OtpDigits {
    /// Returns the digit count.
    #[must_use]
    pub const fn count(self) -> u32 {
        match self {
            Self::Six => 6,
            Self::Eight => 8,
        }
    }
}

impl From<OtpDigits> for u8 {
    fn from(value: OtpDigits) -> Self {
        match value {
            OtpDigits::Six => 6,
            OtpDigits::Eight => 8,
        }
    }
}

impl TryFrom<u8> for OtpDigits {
    type Error = DomainError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            6 => Ok(Self::Six),
            8 => Ok(Self::Eight),
            other => Err(DomainError::new(
                crate::DiagnosticCode::OtpParametersInvalid,
                format!("{other} digits is not supported; use 6 or 8"),
            )),
        }
    }
}

impl fmt::Display for OtpDigits {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.count())
    }
}

/// The smallest TOTP period accepted, in seconds.
pub const MIN_PERIOD_SECONDS: u32 = 10;
/// The largest TOTP period accepted, in seconds.
pub const MAX_PERIOD_SECONDS: u32 = 600;
/// The TOTP period assumed when a URI omits it (RFC 6238 default).
pub const DEFAULT_PERIOD_SECONDS: u32 = 30;

/// Everything needed to generate codes for one factor, except the shared secret.
///
/// The secret is deliberately absent: it lives in the vault and is only joined
/// with these parameters inside `tw-otp` at generation time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OtpParameters {
    /// TOTP or HOTP.
    pub kind: OtpKind,
    /// HMAC hash.
    pub algorithm: OtpAlgorithm,
    /// Digits per code.
    pub digits: OtpDigits,
    /// TOTP step in seconds. Ignored for HOTP.
    pub period_seconds: u32,
    /// HOTP counter. Ignored for TOTP.
    pub counter: u64,
    /// Issuer label from the URI, if any.
    pub issuer: Option<String>,
    /// Account label from the URI, if any.
    pub account_label: Option<String>,
}

impl Default for OtpParameters {
    fn default() -> Self {
        Self {
            kind: OtpKind::Totp,
            algorithm: OtpAlgorithm::Sha1,
            digits: OtpDigits::Six,
            period_seconds: DEFAULT_PERIOD_SECONDS,
            counter: 0,
            issuer: None,
            account_label: None,
        }
    }
}

impl OtpParameters {
    /// Checks the parameters are within the ranges the standards allow.
    ///
    /// # Errors
    ///
    /// Returns [`crate::DiagnosticCode::OtpParametersInvalid`] when the TOTP
    /// period is outside [`MIN_PERIOD_SECONDS`]..=[`MAX_PERIOD_SECONDS`].
    pub fn validate(&self) -> DomainResult<()> {
        if self.kind == OtpKind::Totp
            && !(MIN_PERIOD_SECONDS..=MAX_PERIOD_SECONDS).contains(&self.period_seconds)
        {
            return Err(DomainError::new(
                crate::DiagnosticCode::OtpParametersInvalid,
                format!("TOTP period must be between {MIN_PERIOD_SECONDS} and {MAX_PERIOD_SECONDS} seconds"),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn algorithms_parse_case_and_hyphen_insensitively() {
        assert_eq!("sha1".parse::<OtpAlgorithm>().expect("parse"), OtpAlgorithm::Sha1);
        assert_eq!(
            "SHA-256".parse::<OtpAlgorithm>().expect("parse"),
            OtpAlgorithm::Sha256
        );
        assert_eq!(
            "Sha512".parse::<OtpAlgorithm>().expect("parse"),
            OtpAlgorithm::Sha512
        );
        assert!("md5".parse::<OtpAlgorithm>().is_err());
    }

    #[test]
    fn digits_accept_only_six_or_eight() {
        assert_eq!(OtpDigits::try_from(6).expect("six").count(), 6);
        assert_eq!(OtpDigits::try_from(8).expect("eight").count(), 8);
        assert!(OtpDigits::try_from(7).is_err());
        assert!(OtpDigits::try_from(0).is_err());
    }

    #[test]
    fn digits_serialize_as_numbers() {
        assert_eq!(serde_json::to_string(&OtpDigits::Eight).expect("serialize"), "8");
        let d: OtpDigits = serde_json::from_str("6").expect("deserialize");
        assert_eq!(d, OtpDigits::Six);
        assert!(serde_json::from_str::<OtpDigits>("7").is_err());
    }

    #[test]
    fn totp_period_bounds_are_enforced() {
        let mut p = OtpParameters::default();
        assert!(p.validate().is_ok());
        p.period_seconds = 5;
        assert!(p.validate().is_err());
        p.period_seconds = 601;
        assert!(p.validate().is_err());
        // HOTP ignores the period entirely.
        p.kind = OtpKind::Hotp;
        assert!(p.validate().is_ok());
    }

    #[test]
    fn kinds_round_trip() {
        assert_eq!("TOTP".parse::<OtpKind>().expect("parse"), OtpKind::Totp);
        assert_eq!("hotp".parse::<OtpKind>().expect("parse"), OtpKind::Hotp);
        assert!("motp".parse::<OtpKind>().is_err());
    }
}
