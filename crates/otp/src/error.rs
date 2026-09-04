//! OTP error type. Errors never include secret material or raw URIs.

use thiserror::Error;

/// Errors produced by OTP computation and `otpauth://` parsing.
///
/// Variants deliberately carry only field names and positions — never the
/// rejected value itself — because rejected values may contain secrets
/// (e.g. a seed embedded in a malformed URI).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OtpError {
    /// The URI was not a well-formed `otpauth://` URI.
    #[error("not a valid otpauth URI")]
    InvalidUri,

    /// The OTP type was neither `totp` nor `hotp`.
    #[error("unsupported OTP type")]
    UnsupportedType,

    /// The secret parameter was missing or not valid base32.
    #[error("invalid secret encoding")]
    InvalidSecret,

    /// The algorithm parameter was not SHA1/SHA256/SHA512.
    #[error("unsupported algorithm")]
    UnsupportedAlgorithm,

    /// A numeric parameter was missing or malformed.
    #[error("invalid numeric parameter: {parameter}")]
    InvalidParameter {
        /// Which parameter was malformed.
        parameter: &'static str,
    },

    /// A parameter had a value outside the supported range.
    #[error("parameter out of range: {parameter}")]
    ParameterOutOfRange {
        /// Which parameter was out of range.
        parameter: &'static str,
    },

    /// HOTP URIs must carry a counter.
    #[error("HOTP requires a counter parameter")]
    MissingCounter,
}

impl OtpError {
    /// Stable diagnostic code.
    pub fn diagnostic_code(&self) -> &'static str {
        match self {
            Self::InvalidUri => "OTP_INVALID_URI",
            Self::UnsupportedType => "OTP_UNSUPPORTED_TYPE",
            Self::InvalidSecret => "OTP_INVALID_SECRET",
            Self::UnsupportedAlgorithm => "OTP_UNSUPPORTED_ALGORITHM",
            Self::InvalidParameter { .. } => "OTP_INVALID_PARAMETER",
            Self::ParameterOutOfRange { .. } => "OTP_PARAMETER_OUT_OF_RANGE",
            Self::MissingCounter => "OTP_MISSING_COUNTER",
        }
    }
}
