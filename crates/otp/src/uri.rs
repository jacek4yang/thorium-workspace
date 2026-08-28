//! `otpauth://` key URI parsing and generation.
//!
//! The format is Google Authenticator's Key Uri Format, which is what every
//! issuer's QR code encodes:
//!
//! ```text
//! otpauth://TYPE/LABEL?secret=BASE32&issuer=...&algorithm=...&digits=...&period=...&counter=...
//! ```
//!
//! A URI contains the shared secret, so no function here ever places the input,
//! or any part of it, into an error message.

use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
use tw_domain::{OtpAlgorithm, OtpDigits, OtpKind, OtpParameters};
use tw_secrets::SecretString;

use crate::secret::OtpSecret;

/// Why an `otpauth://` URI could not be used.
///
/// No variant carries any part of the input: the input contains the secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum OtpUriError {
    /// The text is not a URI at all.
    #[error("this is not a valid otpauth:// URI")]
    NotAUri,
    /// The scheme was not `otpauth`.
    #[error("this QR code is not an otpauth:// two-factor URI")]
    WrongScheme,
    /// The type segment was not `totp` or `hotp`.
    #[error("the URI type must be totp or hotp")]
    UnsupportedType,
    /// The `secret` parameter was absent.
    #[error("the URI does not contain a secret")]
    MissingSecret,
    /// The `secret` parameter was not valid Base32.
    #[error("the secret in the URI is not valid Base32")]
    SecretNotBase32,
    /// The decoded secret was too short or implausibly long.
    #[error("the secret in the URI is not a usable length")]
    SecretLength,
    /// The `algorithm` parameter named an unsupported hash.
    #[error("the URI names an unsupported algorithm; only SHA1, SHA256 and SHA512 are supported")]
    UnsupportedAlgorithm,
    /// The `digits` parameter was not 6 or 8.
    #[error("the URI requests an unsupported digit count; only 6 and 8 are supported")]
    UnsupportedDigits,
    /// The `period` parameter was outside the accepted range.
    #[error("the URI requests a period outside the supported range")]
    UnsupportedPeriod,
    /// A HOTP URI omitted the required `counter` parameter.
    #[error("a hotp:// URI must include a counter")]
    MissingCounter,
    /// A numeric parameter was not a number.
    #[error("a numeric parameter in the URI is not a number")]
    NotANumber,
}

/// A parsed `otpauth://` credential: parameters plus the decoded secret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtpCredential {
    /// Everything except the secret.
    pub parameters: OtpParameters,
    /// The decoded shared secret.
    pub secret: OtpSecret,
}

impl OtpCredential {
    /// A label for the imported factor, derived from the issuer and account.
    ///
    /// Issuer and account labels are not secret; they are the human-readable
    /// part of the URI and are what the user sees in the factor list.
    #[must_use]
    pub fn suggested_label(&self) -> String {
        match (&self.parameters.issuer, &self.parameters.account_label) {
            (Some(issuer), Some(account)) => format!("{issuer} ({account})"),
            (Some(issuer), None) => issuer.clone(),
            (None, Some(account)) => account.clone(),
            (None, None) => "Authenticator".to_owned(),
        }
    }
}

/// Parses an `otpauth://` URI.
///
/// # Errors
///
/// Returns the [`OtpUriError`] describing what was structurally wrong. The
/// error never contains the input.
pub fn parse_otpauth_uri(raw: &str) -> Result<OtpCredential, OtpUriError> {
    let parsed = url::Url::parse(raw.trim()).map_err(|_| OtpUriError::NotAUri)?;
    if !parsed.scheme().eq_ignore_ascii_case("otpauth") {
        return Err(OtpUriError::WrongScheme);
    }

    let kind = match parsed
        .host_str()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "totp" => OtpKind::Totp,
        "hotp" => OtpKind::Hotp,
        _ => return Err(OtpUriError::UnsupportedType),
    };

    // The label is `Issuer:Account` or just `Account`, percent-encoded.
    let label = percent_encoding::percent_decode_str(parsed.path().trim_start_matches('/'))
        .decode_utf8()
        .map(|s| s.into_owned())
        .unwrap_or_default();
    let (label_issuer, account_label) = split_label(&label);

    let mut secret_param: Option<String> = None;
    let mut issuer_param: Option<String> = None;
    let mut algorithm = OtpAlgorithm::Sha1;
    let mut digits = OtpDigits::Six;
    let mut period = tw_domain::otp::DEFAULT_PERIOD_SECONDS;
    let mut counter: Option<u64> = None;

    for (key, value) in parsed.query_pairs() {
        match key.to_ascii_lowercase().as_str() {
            "secret" => secret_param = Some(value.into_owned()),
            "issuer" => issuer_param = Some(value.into_owned()),
            "algorithm" => {
                algorithm = value.parse().map_err(|_| OtpUriError::UnsupportedAlgorithm)?;
            }
            "digits" => {
                let n: u8 = value.parse().map_err(|_| OtpUriError::NotANumber)?;
                digits = OtpDigits::try_from(n).map_err(|_| OtpUriError::UnsupportedDigits)?;
            }
            "period" => {
                period = value.parse().map_err(|_| OtpUriError::NotANumber)?;
            }
            "counter" => {
                counter = Some(value.parse().map_err(|_| OtpUriError::NotANumber)?);
            }
            // Unknown parameters (image, lock, and vendor extensions) are
            // ignored rather than rejected: issuers add them freely.
            _ => {}
        }
    }

    let secret_text = secret_param.ok_or(OtpUriError::MissingSecret)?;
    let secret = OtpSecret::from_base32(&secret_text)?;

    if kind == OtpKind::Hotp && counter.is_none() {
        return Err(OtpUriError::MissingCounter);
    }

    let parameters = OtpParameters {
        kind,
        algorithm,
        digits,
        period_seconds: period,
        counter: counter.unwrap_or(0),
        // An explicit `issuer=` parameter wins over the label prefix; the Key Uri
        // Format says they should agree and that the parameter is authoritative.
        issuer: issuer_param.or(label_issuer).filter(|s| !s.is_empty()),
        account_label: account_label.filter(|s| !s.is_empty()),
    };
    parameters
        .validate()
        .map_err(|_| OtpUriError::UnsupportedPeriod)?;

    Ok(OtpCredential { parameters, secret })
}

fn split_label(label: &str) -> (Option<String>, Option<String>) {
    match label.split_once(':') {
        Some((issuer, account)) => (Some(issuer.trim().to_owned()), Some(account.trim().to_owned())),
        None => {
            let trimmed = label.trim();
            if trimmed.is_empty() {
                (None, None)
            } else {
                (None, Some(trimmed.to_owned()))
            }
        }
    }
}

/// Characters that must be percent-encoded inside a URI path or query value.
const URI_ESCAPE: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'&')
    .add(b'/')
    .add(b'<')
    .add(b'=')
    .add(b'>')
    .add(b'?')
    .add(b'@')
    .add(b'`')
    .add(b'{')
    .add(b'}');

/// Rebuilds an `otpauth://` URI from stored parameters and a secret.
///
/// Used for export and for regenerating a QR code. The result contains the
/// secret, so it is returned as a [`SecretString`] and must be handled like any
/// other secret.
#[must_use]
pub fn build_otpauth_uri(params: &OtpParameters, secret: &OtpSecret) -> SecretString {
    let label = match (&params.issuer, &params.account_label) {
        (Some(issuer), Some(account)) => format!("{issuer}:{account}"),
        (Some(issuer), None) => issuer.clone(),
        (None, Some(account)) => account.clone(),
        (None, None) => "Account".to_owned(),
    };
    let mut uri = format!(
        "otpauth://{}/{}?secret={}",
        params.kind.as_uri_type(),
        utf8_percent_encode(&label, URI_ESCAPE),
        secret.to_base32().expose()
    );
    if let Some(issuer) = &params.issuer {
        uri.push_str("&issuer=");
        uri.push_str(&utf8_percent_encode(issuer, URI_ESCAPE).to_string());
    }
    uri.push_str("&algorithm=");
    uri.push_str(params.algorithm.as_uri_value());
    uri.push_str("&digits=");
    uri.push_str(&params.digits.count().to_string());
    match params.kind {
        OtpKind::Totp => {
            uri.push_str("&period=");
            uri.push_str(&params.period_seconds.to_string());
        }
        OtpKind::Hotp => {
            uri.push_str("&counter=");
            uri.push_str(&params.counter.to_string());
        }
    }
    SecretString::new(uri)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Synthetic 20-byte secret. Never a real credential.
    const SECRET: &str = "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP";

    #[test]
    fn a_minimal_totp_uri_takes_the_rfc_defaults() {
        let uri = format!("otpauth://totp/Example:alice@example.com?secret={SECRET}");
        let cred = parse_otpauth_uri(&uri).expect("parse");
        assert_eq!(cred.parameters.kind, OtpKind::Totp);
        assert_eq!(cred.parameters.algorithm, OtpAlgorithm::Sha1);
        assert_eq!(cred.parameters.digits, OtpDigits::Six);
        assert_eq!(cred.parameters.period_seconds, 30);
        assert_eq!(cred.parameters.issuer.as_deref(), Some("Example"));
        assert_eq!(
            cred.parameters.account_label.as_deref(),
            Some("alice@example.com")
        );
        assert_eq!(cred.secret.len(), 20);
    }

    #[test]
    fn every_optional_parameter_is_honoured() {
        let uri = format!(
            "otpauth://totp/ACME%20Co:jane@acme.test?secret={SECRET}&issuer=ACME%20Co&algorithm=SHA512&digits=8&period=60"
        );
        let cred = parse_otpauth_uri(&uri).expect("parse");
        assert_eq!(cred.parameters.algorithm, OtpAlgorithm::Sha512);
        assert_eq!(cred.parameters.digits, OtpDigits::Eight);
        assert_eq!(cred.parameters.period_seconds, 60);
        assert_eq!(cred.parameters.issuer.as_deref(), Some("ACME Co"));
        assert_eq!(cred.parameters.account_label.as_deref(), Some("jane@acme.test"));
        assert_eq!(cred.suggested_label(), "ACME Co (jane@acme.test)");
    }

    #[test]
    fn hotp_requires_a_counter() {
        let without = format!("otpauth://hotp/Example:alice?secret={SECRET}");
        assert_eq!(parse_otpauth_uri(&without), Err(OtpUriError::MissingCounter));

        let with = format!("otpauth://hotp/Example:alice?secret={SECRET}&counter=7");
        let cred = parse_otpauth_uri(&with).expect("parse");
        assert_eq!(cred.parameters.kind, OtpKind::Hotp);
        assert_eq!(cred.parameters.counter, 7);
    }

    #[test]
    fn the_issuer_parameter_wins_over_the_label_prefix() {
        let uri = format!("otpauth://totp/Stale:alice?secret={SECRET}&issuer=Current");
        let cred = parse_otpauth_uri(&uri).expect("parse");
        assert_eq!(cred.parameters.issuer.as_deref(), Some("Current"));
        assert_eq!(cred.parameters.account_label.as_deref(), Some("alice"));
    }

    #[test]
    fn a_label_without_an_issuer_becomes_the_account() {
        let uri = format!("otpauth://totp/alice@example.com?secret={SECRET}");
        let cred = parse_otpauth_uri(&uri).expect("parse");
        assert_eq!(cred.parameters.issuer, None);
        assert_eq!(
            cred.parameters.account_label.as_deref(),
            Some("alice@example.com")
        );
        assert_eq!(cred.suggested_label(), "alice@example.com");
    }

    #[test]
    fn unknown_parameters_are_ignored_not_rejected() {
        let uri =
            format!("otpauth://totp/Example:alice?secret={SECRET}&image=https://x.test/i.png&lock=false");
        assert!(parse_otpauth_uri(&uri).is_ok());
    }

    #[test]
    fn malformed_uris_are_rejected_with_a_specific_reason() {
        assert_eq!(parse_otpauth_uri("not a uri"), Err(OtpUriError::NotAUri));
        assert_eq!(
            parse_otpauth_uri("https://example.com"),
            Err(OtpUriError::WrongScheme)
        );
        assert_eq!(
            parse_otpauth_uri("otpauth://motp/a?secret=AAAA"),
            Err(OtpUriError::UnsupportedType)
        );
        assert_eq!(
            parse_otpauth_uri("otpauth://totp/a"),
            Err(OtpUriError::MissingSecret)
        );
        assert_eq!(
            parse_otpauth_uri("otpauth://totp/a?secret=!!!!"),
            Err(OtpUriError::SecretNotBase32)
        );
        assert_eq!(
            parse_otpauth_uri("otpauth://totp/a?secret=MZXW6"),
            Err(OtpUriError::SecretLength)
        );
        assert_eq!(
            parse_otpauth_uri(&format!("otpauth://totp/a?secret={SECRET}&algorithm=MD5")),
            Err(OtpUriError::UnsupportedAlgorithm)
        );
        assert_eq!(
            parse_otpauth_uri(&format!("otpauth://totp/a?secret={SECRET}&digits=7")),
            Err(OtpUriError::UnsupportedDigits)
        );
        assert_eq!(
            parse_otpauth_uri(&format!("otpauth://totp/a?secret={SECRET}&digits=x")),
            Err(OtpUriError::NotANumber)
        );
        assert_eq!(
            parse_otpauth_uri(&format!("otpauth://totp/a?secret={SECRET}&period=1")),
            Err(OtpUriError::UnsupportedPeriod)
        );
    }

    #[test]
    fn errors_never_echo_the_uri_or_the_secret() {
        let uri = format!("otpauth://totp/a?secret={SECRET}&digits=7");
        let err = parse_otpauth_uri(&uri).expect_err("must fail");
        let rendered = format!("{err} {err:?}");
        assert!(!rendered.contains(SECRET), "{rendered}");
        assert!(!rendered.contains("otpauth"), "{rendered}");
    }

    #[test]
    fn the_scheme_is_matched_case_insensitively() {
        let uri = format!("OTPAUTH://TOTP/Example:alice?secret={SECRET}");
        assert!(parse_otpauth_uri(&uri).is_ok());
    }

    #[test]
    fn generated_uris_round_trip() {
        let original = format!(
            "otpauth://totp/ACME%20Co:jane@acme.test?secret={SECRET}&issuer=ACME%20Co&algorithm=SHA256&digits=8&period=45"
        );
        let cred = parse_otpauth_uri(&original).expect("parse");
        let rebuilt = build_otpauth_uri(&cred.parameters, &cred.secret);
        let reparsed = parse_otpauth_uri(rebuilt.expose()).expect("re-parse");
        assert_eq!(reparsed.parameters, cred.parameters);
        assert_eq!(reparsed.secret, cred.secret);
    }

    #[test]
    fn generated_hotp_uris_carry_the_counter() {
        let cred = parse_otpauth_uri(&format!(
            "otpauth://hotp/Example:alice?secret={SECRET}&counter=42"
        ))
        .expect("parse");
        let rebuilt = build_otpauth_uri(&cred.parameters, &cred.secret);
        assert!(rebuilt.expose().contains("counter=42"));
        assert!(!rebuilt.expose().contains("period="));
        assert_eq!(
            parse_otpauth_uri(rebuilt.expose())
                .expect("re-parse")
                .parameters
                .counter,
            42
        );
    }

    #[test]
    fn a_generated_uri_is_a_secret_and_says_so() {
        let cred = parse_otpauth_uri(&format!("otpauth://totp/a:b?secret={SECRET}")).expect("parse");
        let uri = build_otpauth_uri(&cred.parameters, &cred.secret);
        assert!(!format!("{uri:?}").contains(SECRET));
        assert_eq!(format!("{uri}"), "[redacted]");
    }

    #[test]
    fn labels_with_separators_survive_the_round_trip() {
        let cred = parse_otpauth_uri(&format!(
            "otpauth://totp/a:b?secret={SECRET}&issuer=Big%20Co%20%2F%20Div"
        ))
        .expect("parse");
        let rebuilt = build_otpauth_uri(&cred.parameters, &cred.secret);
        let reparsed = parse_otpauth_uri(rebuilt.expose()).expect("re-parse");
        assert_eq!(reparsed.parameters.issuer.as_deref(), Some("Big Co / Div"));
    }
}
