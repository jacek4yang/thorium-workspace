//! `otpauth://` URI parsing.
//!
//! Format (Google Authenticator key URI convention):
//!
//! ```text
//! otpauth://TOTP/Issuer:Account?secret=BASE32&issuer=Issuer&algorithm=SHA1&digits=6&period=30
//! otpauth://HOTP/Account?secret=BASE32&counter=0
//! ```
//!
//! Security rules for this module:
//! - parsed secrets are returned only as [`SecretBytes`];
//! - errors never embed the URI or any parameter value;
//! - this module performs no logging.

use data_encoding::BASE32_NOPAD;
use thorium_workspace_domain::{FactorKind, MAX_TOTP_PERIOD_SECONDS, OtpAlgorithm};
use thorium_workspace_secrets::SecretBytes;

use crate::error::OtpError;

/// A parsed `otpauth://` factor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedOtpAuth {
    /// TOTP or HOTP.
    pub kind: FactorKind,
    /// Hash algorithm (defaults to SHA-1 when absent).
    pub algorithm: OtpAlgorithm,
    /// Digit count (defaults to 6).
    pub digits: u8,
    /// TOTP period in seconds (defaults to 30).
    pub period_seconds: u32,
    /// HOTP counter (required for HOTP).
    pub counter: Option<u64>,
    /// Issuer label, from the label prefix or the `issuer` parameter.
    pub issuer: Option<String>,
    /// Account label.
    pub account_label: Option<String>,
    /// The decoded seed. Secret material: never log, never serialize.
    pub secret: SecretBytes,
}

impl ParsedOtpAuth {
    /// Computes the current code for this factor at `unix_time`.
    ///
    /// Returns `(code, seconds_remaining_in_window)` for TOTP, or the
    /// current-counter code for HOTP.
    pub fn code_at(&self, unix_time: u64) -> Result<(String, u32), OtpError> {
        match self.kind {
            FactorKind::Totp => {
                let code = crate::engine::totp(
                    self.secret.expose(),
                    unix_time,
                    self.period_seconds,
                    self.algorithm,
                    self.digits,
                )?;
                let remaining = crate::engine::seconds_remaining(unix_time, self.period_seconds);
                Ok((code, remaining))
            }
            FactorKind::Hotp => {
                let counter = self.counter.unwrap_or(0);
                let code = crate::engine::hotp(
                    self.secret.expose(),
                    counter,
                    self.algorithm,
                    self.digits,
                )?;
                Ok((code, 0))
            }
            FactorKind::ExternalAuthenticator => Err(OtpError::UnsupportedType),
        }
    }
}

/// Parses an `otpauth://` URI.
pub fn parse_otpauth_uri(uri: &str) -> Result<ParsedOtpAuth, OtpError> {
    let rest = uri.strip_prefix("otpauth://").ok_or(OtpError::InvalidUri)?;

    let (type_and_label, query) = match rest.split_once('?') {
        Some((left, right)) => (left, Some(right)),
        None => (rest, None),
    };

    let (type_str, label_part) = type_and_label.split_once('/').ok_or(OtpError::InvalidUri)?;

    let kind = match type_str.to_ascii_lowercase().as_str() {
        "totp" => FactorKind::Totp,
        "hotp" => FactorKind::Hotp,
        _ => return Err(OtpError::UnsupportedType),
    };

    let (issuer_from_label, account_label) = split_label(&percent_decode(label_part));

    let mut secret: Option<SecretBytes> = None;
    let mut algorithm = OtpAlgorithm::Sha1;
    let mut digits: u8 = 6;
    let mut period_seconds: u32 = 30;
    let mut counter: Option<u64> = None;
    let mut issuer_param: Option<String> = None;

    if let Some(query) = query {
        for pair in query.split('&') {
            let (key, value) = pair.split_once('=').ok_or(OtpError::InvalidUri)?;
            let value = percent_decode(value);
            match key.to_ascii_lowercase().as_str() {
                "secret" => {
                    let decoded = decode_base32_secret(&value)?;
                    secret = Some(SecretBytes::new(&decoded));
                }
                "issuer" => issuer_param = Some(value),
                "algorithm" => {
                    algorithm = match value.to_ascii_uppercase().as_str() {
                        "SHA1" | "SHA-1" => OtpAlgorithm::Sha1,
                        "SHA256" | "SHA-256" => OtpAlgorithm::Sha256,
                        "SHA512" | "SHA-512" => OtpAlgorithm::Sha512,
                        _ => return Err(OtpError::UnsupportedAlgorithm),
                    };
                }
                "digits" => {
                    digits = value.parse().map_err(|_| OtpError::InvalidParameter {
                        parameter: "digits",
                    })?;
                }
                "period" => {
                    period_seconds = value.parse().map_err(|_| OtpError::InvalidParameter {
                        parameter: "period",
                    })?;
                }
                "counter" => {
                    counter = Some(value.parse().map_err(|_| OtpError::InvalidParameter {
                        parameter: "counter",
                    })?);
                }
                _ => {}
            }
        }
    }

    let secret = secret.ok_or(OtpError::InvalidSecret)?;
    if !matches!(digits, 6 | 8) {
        return Err(OtpError::ParameterOutOfRange {
            parameter: "digits",
        });
    }
    if kind == FactorKind::Totp && (period_seconds == 0 || period_seconds > MAX_TOTP_PERIOD_SECONDS)
    {
        return Err(OtpError::ParameterOutOfRange {
            parameter: "period",
        });
    }
    if kind == FactorKind::Hotp && counter.is_none() {
        return Err(OtpError::MissingCounter);
    }

    Ok(ParsedOtpAuth {
        kind,
        algorithm,
        digits,
        period_seconds,
        counter,
        issuer: issuer_param.or(issuer_from_label),
        account_label,
        secret,
    })
}

/// Splits a label of the form `Issuer:Account` (or bare `Account`).
fn split_label(label: &str) -> (Option<String>, Option<String>) {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return (None, None);
    }
    match trimmed.split_once(':') {
        Some((issuer, account)) => {
            let issuer = issuer.trim();
            let account = account.trim();
            (
                (!issuer.is_empty()).then(|| issuer.to_owned()),
                (!account.is_empty()).then(|| account.to_owned()),
            )
        }
        None => (None, Some(trimmed.to_owned())),
    }
}

/// Decodes an otpauth secret parameter: case-insensitive base32 with or
/// without padding. Returns raw seed bytes; callers must treat them as
/// secret.
fn decode_base32_secret(value: &str) -> Result<Vec<u8>, OtpError> {
    let cleaned: String = value
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let unpadded = cleaned.trim_end_matches('=');
    if unpadded.is_empty() {
        return Err(OtpError::InvalidSecret);
    }
    BASE32_NOPAD
        .decode(unpadded.as_bytes())
        .map_err(|_| OtpError::InvalidSecret)
}

/// Minimal percent-decoder for URI label and parameter segments.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            let hex = &value[index + 1..index + 3];
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        // Replace '+' with space only in query-style decoding contexts;
        // otpauth labels rarely use it, but some generators do.
        if bytes[index] == b'+' {
            out.push(b' ');
            index += 1;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic RFC 4226 seed, base32-encoded: "12345678901234567890" ->
    // GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ
    const SYNTHETIC_SECRET_B32: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    #[test]
    fn parses_minimal_totp_uri() {
        let uri = format!("otpauth://totp/alice@example.com?secret={SYNTHETIC_SECRET_B32}");
        let parsed = parse_otpauth_uri(&uri).expect("valid");
        assert_eq!(parsed.kind, FactorKind::Totp);
        assert_eq!(parsed.algorithm, OtpAlgorithm::Sha1);
        assert_eq!(parsed.digits, 6);
        assert_eq!(parsed.period_seconds, 30);
        assert_eq!(parsed.account_label.as_deref(), Some("alice@example.com"));
        assert_eq!(parsed.secret.expose(), b"12345678901234567890");
    }

    #[test]
    fn parses_full_totp_uri_with_issuer() {
        let uri = format!(
            "otpauth://totp/GitHub:octocat?secret={SYNTHETIC_SECRET_B32}&issuer=GitHub&algorithm=SHA256&digits=8&period=60"
        );
        let parsed = parse_otpauth_uri(&uri).expect("valid");
        assert_eq!(parsed.issuer.as_deref(), Some("GitHub"));
        assert_eq!(parsed.account_label.as_deref(), Some("octocat"));
        assert_eq!(parsed.algorithm, OtpAlgorithm::Sha256);
        assert_eq!(parsed.digits, 8);
        assert_eq!(parsed.period_seconds, 60);
    }

    #[test]
    fn parses_hotp_uri_with_counter() {
        let uri = format!("otpauth://hotp/bob?secret={SYNTHETIC_SECRET_B32}&counter=5&digits=6");
        let parsed = parse_otpauth_uri(&uri).expect("valid");
        assert_eq!(parsed.kind, FactorKind::Hotp);
        assert_eq!(parsed.counter, Some(5));
    }

    #[test]
    fn hotp_requires_counter() {
        let uri = format!("otpauth://hotp/bob?secret={SYNTHETIC_SECRET_B32}");
        assert!(matches!(
            parse_otpauth_uri(&uri),
            Err(OtpError::MissingCounter)
        ));
    }

    #[test]
    fn accepts_lowercase_secret_and_padding() {
        let lower = SYNTHETIC_SECRET_B32.to_ascii_lowercase();
        let padded = format!("{SYNTHETIC_SECRET_B32}=");
        for variant in [
            format!("otpauth://totp/a?secret={lower}"),
            format!("otpauth://totp/a?secret={padded}"),
        ] {
            let parsed = parse_otpauth_uri(&variant).expect("valid");
            assert_eq!(parsed.secret.expose(), b"12345678901234567890");
        }
    }

    #[test]
    fn percent_decodes_labels() {
        let uri =
            format!("otpauth://totp/ACME%20Co:john%40example.com?secret={SYNTHETIC_SECRET_B32}");
        let parsed = parse_otpauth_uri(&uri).expect("valid");
        assert_eq!(parsed.issuer.as_deref(), Some("ACME Co"));
        assert_eq!(parsed.account_label.as_deref(), Some("john@example.com"));
    }

    #[test]
    fn rejects_broken_uris_without_leaking_them() {
        let cases = [
            "https://github.com",
            "otpauth://push/alice?secret=AAAA",
            "otpauth://totp/alice",
            "otpauth://totp/alice?secret=!!!not-base32!!!",
            "otpauth://totp/alice?secret=AAAA&algorithm=MD5",
            "otpauth://totp/alice?secret=AAAA&digits=7",
            "otpauth://totp/alice?secret=AAAA&period=0",
        ];
        for uri in cases {
            let error = parse_otpauth_uri(uri).expect_err("must fail");
            // The rendered error must not embed the rejected URI.
            let rendered = format!("{error:?} {error}");
            assert!(!rendered.contains("secret=AAAA"), "leaked in: {rendered}");
        }
    }

    #[test]
    fn parsed_secrets_render_redacted_through_debug() {
        let uri = format!("otpauth://totp/a?secret={SYNTHETIC_SECRET_B32}");
        let parsed = parse_otpauth_uri(&uri).expect("valid");
        let rendered = format!("{parsed:?}");
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains(SYNTHETIC_SECRET_B32));
    }

    #[test]
    fn parsed_factor_produces_rfc_correct_codes() {
        // Seed = RFC 4226 key; TOTP at t=59 (counter 1) must match the
        // RFC 4226 vector for counter 1: 287082.
        let uri = format!("otpauth://totp/a?secret={SYNTHETIC_SECRET_B32}");
        let parsed = parse_otpauth_uri(&uri).expect("valid");
        let (code, remaining) = parsed.code_at(59).expect("computable");
        assert_eq!(code, "287082");
        assert_eq!(remaining, 1);
    }
}
