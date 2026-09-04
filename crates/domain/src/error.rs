//! Typed domain errors with stable diagnostic codes.
//!
//! Diagnostic codes are stable identifiers surfaced to the UI diagnostics
//! page and error displays. They must never embed secret material.

/// Marker trait implemented by error types that expose stable diagnostic
/// codes.
pub trait DiagnosticCode {
    /// Stable, machine-readable code safe for logs and diagnostics.
    fn diagnostic_code(&self) -> &'static str;
}

impl DiagnosticCode for DomainError {
    fn diagnostic_code(&self) -> &'static str {
        DomainError::diagnostic_code(self)
    }
}

/// Error type for all domain-level validation and construction failures.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DomainError {
    /// A required name was empty or contained only whitespace.
    #[error("name must not be empty")]
    EmptyName,

    /// A name exceeded the maximum length.
    #[error("name exceeds {max} characters")]
    NameTooLong { max: usize },

    /// A value contained control characters.
    #[error("value contains control characters")]
    ControlCharacters,

    /// A URL was not a well-formed http(s) URL.
    #[error("URL must be a valid http or https URL")]
    InvalidUrl,

    /// A proxy endpoint was not a usable `scheme://host:port` URL.
    #[error("proxy must be a valid http://, https://, socks5://, or socks5h:// host:port endpoint")]
    InvalidProxyUrl,

    /// A timezone did not look like an IANA identifier.
    #[error("timezone must be an IANA identifier such as America/Los_Angeles")]
    InvalidTimezone,

    /// A locale did not look like a BCP-47 language tag.
    #[error("locale must be a BCP-47 tag such as en-US")]
    InvalidLocale,

    /// An identifier could not be parsed.
    #[error("invalid identifier")]
    InvalidId,

    /// A count or numeric field was out of the allowed range.
    #[error("value out of range: {field}")]
    OutOfRange { field: &'static str },

    /// A tag was not a usable tag token.
    #[error("invalid tag")]
    InvalidTag,
}

impl DomainError {
    /// Stable diagnostic code for this error variant.
    pub fn diagnostic_code(&self) -> &'static str {
        match self {
            Self::EmptyName => "DOMAIN_EMPTY_NAME",
            Self::NameTooLong { .. } => "DOMAIN_NAME_TOO_LONG",
            Self::ControlCharacters => "DOMAIN_CONTROL_CHARACTERS",
            Self::InvalidUrl => "DOMAIN_INVALID_URL",
            Self::InvalidProxyUrl => "DOMAIN_INVALID_PROXY_URL",
            Self::InvalidTimezone => "DOMAIN_INVALID_TIMEZONE",
            Self::InvalidLocale => "DOMAIN_INVALID_LOCALE",
            Self::InvalidId => "DOMAIN_INVALID_ID",
            Self::OutOfRange { .. } => "DOMAIN_OUT_OF_RANGE",
            Self::InvalidTag => "DOMAIN_INVALID_TAG",
        }
    }

    /// Human-oriented message that is safe to display (never contains
    /// secrets; this type never sees secret material).
    pub fn user_message(&self) -> String {
        self.to_string()
    }
}
