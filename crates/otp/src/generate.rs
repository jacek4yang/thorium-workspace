//! HOTP and TOTP code generation.

use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::{Sha256, Sha512};
use tw_domain::{OtpAlgorithm, OtpDigits, OtpKind, OtpParameters};
use zeroize::Zeroize;

use crate::secret::OtpSecret;

/// A generated code together with the validity information the UI needs to draw
/// a countdown.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct OtpCode {
    /// The code, zero-padded to the configured digit count.
    ///
    /// A live OTP code is short-lived and the user is about to read it off the
    /// screen, so it is carried as plain text rather than a `SecretString`. It
    /// is never logged and never written to disk.
    pub code: String,
    /// Seconds until this code expires, for TOTP.
    pub valid_for_seconds: Option<u32>,
    /// The TOTP step this code belongs to, or the HOTP counter used.
    pub counter: u64,
}

/// How many steps either side of the current one a TOTP check accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TotpWindow(pub u8);

impl Default for TotpWindow {
    fn default() -> Self {
        // RFC 6238 section 5.2: at most one step of drift.
        Self(1)
    }
}

/// Computes an HOTP value (RFC 4226).
#[must_use]
pub fn hotp(secret: &OtpSecret, counter: u64, algorithm: OtpAlgorithm, digits: OtpDigits) -> String {
    let mut mac = mac_bytes(secret, counter, algorithm);
    let code = dynamic_truncate(&mac, digits.count());
    mac.zeroize();
    format_code(code, digits.count())
}

/// Computes a TOTP value for the current system time (RFC 6238).
#[must_use]
pub fn totp(secret: &OtpSecret, params: &OtpParameters) -> OtpCode {
    totp_at(secret, params, unix_now())
}

/// Computes a TOTP value for an explicit Unix timestamp.
///
/// Split out so RFC 6238's test vectors, which pin specific timestamps, are
/// exercised by the same code path the application uses.
#[must_use]
pub fn totp_at(secret: &OtpSecret, params: &OtpParameters, unix_seconds: u64) -> OtpCode {
    let period = u64::from(params.period_seconds.max(1));
    let counter = unix_seconds / period;
    let elapsed = unix_seconds % period;
    let remaining = u32::try_from(period - elapsed).unwrap_or(u32::MAX);
    OtpCode {
        code: hotp(secret, counter, params.algorithm, params.digits),
        valid_for_seconds: Some(remaining),
        counter,
    }
}

/// Checks `candidate` against the current TOTP step and `window` steps either
/// side of it.
///
/// Comparison is constant time so a caller cannot use timing to learn how much
/// of a guess was correct.
#[must_use]
pub fn verify_totp(
    secret: &OtpSecret,
    params: &OtpParameters,
    candidate: &str,
    unix_seconds: u64,
    window: TotpWindow,
) -> bool {
    let period = u64::from(params.period_seconds.max(1));
    let center = unix_seconds / period;
    let span = i64::from(window.0);
    let mut matched = false;
    for offset in -span..=span {
        let Some(counter) = center.checked_add_signed(offset) else {
            continue;
        };
        let expected = hotp(secret, counter, params.algorithm, params.digits);
        // No early exit: every step is compared so the loop takes the same time
        // whichever step matches.
        matched |= tw_secrets::constant_time_eq(expected.as_bytes(), candidate.as_bytes());
    }
    matched
}

/// Checks `candidate` against HOTP counters `counter..=counter + look_ahead`.
///
/// Returns the counter that matched, so the caller can resynchronise.
#[must_use]
pub fn verify_hotp(
    secret: &OtpSecret,
    params: &OtpParameters,
    candidate: &str,
    counter: u64,
    look_ahead: u8,
) -> Option<u64> {
    let mut found = None;
    for offset in 0..=u64::from(look_ahead) {
        let Some(c) = counter.checked_add(offset) else {
            continue;
        };
        let expected = hotp(secret, c, params.algorithm, params.digits);
        if tw_secrets::constant_time_eq(expected.as_bytes(), candidate.as_bytes()) && found.is_none() {
            found = Some(c);
        }
    }
    found
}

fn mac_bytes(secret: &OtpSecret, counter: u64, algorithm: OtpAlgorithm) -> Vec<u8> {
    let message = counter.to_be_bytes();
    match algorithm {
        OtpAlgorithm::Sha1 => compute::<Sha1>(secret.expose(), &message),
        OtpAlgorithm::Sha256 => compute::<Sha256>(secret.expose(), &message),
        OtpAlgorithm::Sha512 => compute::<Sha512>(secret.expose(), &message),
    }
}

fn compute<D>(key: &[u8], message: &[u8]) -> Vec<u8>
where
    D: sha2::digest::core_api::CoreProxy,
    D::Core: Clone
        + sha2::digest::core_api::UpdateCore
        + sha2::digest::core_api::FixedOutputCore
        + sha2::digest::core_api::BufferKindUser<BufferKind = sha2::digest::block_buffer::Eager>
        + sha2::digest::HashMarker
        + Default,
    <D::Core as sha2::digest::core_api::BlockSizeUser>::BlockSize:
        sha2::digest::typenum::IsLess<sha2::digest::typenum::U256, Output = sha2::digest::typenum::True>,
{
    // `new_from_slice` only fails for key sizes an HMAC cannot take; HMAC accepts
    // any key length, so this branch is unreachable in practice. It is handled
    // rather than unwrapped because this is a runtime path.
    match <Hmac<D> as Mac>::new_from_slice(key) {
        Ok(mut mac) => {
            mac.update(message);
            mac.finalize().into_bytes().to_vec()
        }
        Err(_) => Vec::new(),
    }
}

/// RFC 4226 section 5.3 dynamic truncation.
fn dynamic_truncate(mac: &[u8], digits: u32) -> u32 {
    let Some(&last) = mac.last() else {
        return 0;
    };
    let offset = usize::from(last & 0x0f);
    let Some(slice) = mac.get(offset..offset + 4) else {
        return 0;
    };
    let binary = (u32::from(slice[0]) & 0x7f) << 24
        | (u32::from(slice[1])) << 16
        | (u32::from(slice[2])) << 8
        | u32::from(slice[3]);
    binary % 10u32.pow(digits)
}

fn format_code(value: u32, digits: u32) -> String {
    format!("{value:0width$}", width = digits as usize)
}

fn unix_now() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Convenience: generates a code for either OTP kind from stored parameters.
#[must_use]
pub fn generate(secret: &OtpSecret, params: &OtpParameters) -> OtpCode {
    match params.kind {
        OtpKind::Totp => totp(secret, params),
        OtpKind::Hotp => OtpCode {
            code: hotp(secret, params.counter, params.algorithm, params.digits),
            valid_for_seconds: None,
            counter: params.counter,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 4226 appendix D and RFC 6238 appendix B use the ASCII secret
    /// "12345678901234567890". These are published test vectors, not credentials.
    fn rfc_secret_sha1() -> OtpSecret {
        OtpSecret::from_bytes_unchecked(b"12345678901234567890".to_vec())
    }

    /// RFC 6238 appendix B seeds SHA-256 and SHA-512 by repeating the same ASCII
    /// string to the hash's block size.
    fn rfc_secret(bytes: usize) -> OtpSecret {
        let base = b"12345678901234567890";
        let mut out = Vec::with_capacity(bytes);
        while out.len() < bytes {
            let take = (bytes - out.len()).min(base.len());
            out.extend_from_slice(&base[..take]);
        }
        OtpSecret::from_bytes_unchecked(out)
    }

    #[test]
    fn rfc_4226_appendix_d_hotp_vectors() {
        // Table 1 of RFC 4226: HOTP values for counters 0..9, 6 digits, SHA-1.
        const EXPECTED: [&str; 10] = [
            "755224", "287082", "359152", "969429", "338314", "254676", "287922", "162583", "399871",
            "520489",
        ];
        let secret = rfc_secret_sha1();
        for (counter, expected) in EXPECTED.iter().enumerate() {
            let actual = hotp(&secret, counter as u64, OtpAlgorithm::Sha1, OtpDigits::Six);
            assert_eq!(&actual, expected, "counter {counter}");
        }
    }

    #[test]
    fn rfc_6238_appendix_b_totp_vectors() {
        // Appendix B of RFC 6238: 8-digit codes at fixed timestamps, one row per
        // algorithm, with a 30 second step.
        struct Vector {
            time: u64,
            algorithm: OtpAlgorithm,
            expected: &'static str,
        }
        const VECTORS: &[Vector] = &[
            Vector {
                time: 59,
                algorithm: OtpAlgorithm::Sha1,
                expected: "94287082",
            },
            Vector {
                time: 59,
                algorithm: OtpAlgorithm::Sha256,
                expected: "46119246",
            },
            Vector {
                time: 59,
                algorithm: OtpAlgorithm::Sha512,
                expected: "90693936",
            },
            Vector {
                time: 1_111_111_109,
                algorithm: OtpAlgorithm::Sha1,
                expected: "07081804",
            },
            Vector {
                time: 1_111_111_109,
                algorithm: OtpAlgorithm::Sha256,
                expected: "68084774",
            },
            Vector {
                time: 1_111_111_109,
                algorithm: OtpAlgorithm::Sha512,
                expected: "25091201",
            },
            Vector {
                time: 1_111_111_111,
                algorithm: OtpAlgorithm::Sha1,
                expected: "14050471",
            },
            Vector {
                time: 1_111_111_111,
                algorithm: OtpAlgorithm::Sha256,
                expected: "67062674",
            },
            Vector {
                time: 1_111_111_111,
                algorithm: OtpAlgorithm::Sha512,
                expected: "99943326",
            },
            Vector {
                time: 1_234_567_890,
                algorithm: OtpAlgorithm::Sha1,
                expected: "89005924",
            },
            Vector {
                time: 1_234_567_890,
                algorithm: OtpAlgorithm::Sha256,
                expected: "91819424",
            },
            Vector {
                time: 1_234_567_890,
                algorithm: OtpAlgorithm::Sha512,
                expected: "93441116",
            },
            Vector {
                time: 2_000_000_000,
                algorithm: OtpAlgorithm::Sha1,
                expected: "69279037",
            },
            Vector {
                time: 2_000_000_000,
                algorithm: OtpAlgorithm::Sha256,
                expected: "90698825",
            },
            Vector {
                time: 2_000_000_000,
                algorithm: OtpAlgorithm::Sha512,
                expected: "38618901",
            },
            Vector {
                time: 20_000_000_000,
                algorithm: OtpAlgorithm::Sha1,
                expected: "65353130",
            },
            Vector {
                time: 20_000_000_000,
                algorithm: OtpAlgorithm::Sha256,
                expected: "77737706",
            },
            Vector {
                time: 20_000_000_000,
                algorithm: OtpAlgorithm::Sha512,
                expected: "47863826",
            },
        ];

        for v in VECTORS {
            let secret = match v.algorithm {
                OtpAlgorithm::Sha1 => rfc_secret(20),
                OtpAlgorithm::Sha256 => rfc_secret(32),
                OtpAlgorithm::Sha512 => rfc_secret(64),
            };
            let params = OtpParameters {
                algorithm: v.algorithm,
                digits: OtpDigits::Eight,
                period_seconds: 30,
                ..Default::default()
            };
            let code = totp_at(&secret, &params, v.time);
            assert_eq!(code.code, v.expected, "t={} algorithm={:?}", v.time, v.algorithm);
        }
    }

    #[test]
    fn codes_are_zero_padded_to_the_configured_width() {
        let secret = rfc_secret_sha1();
        for counter in 0..200u64 {
            let six = hotp(&secret, counter, OtpAlgorithm::Sha1, OtpDigits::Six);
            let eight = hotp(&secret, counter, OtpAlgorithm::Sha1, OtpDigits::Eight);
            assert_eq!(six.len(), 6, "{six}");
            assert_eq!(eight.len(), 8, "{eight}");
            assert!(six.chars().all(|c| c.is_ascii_digit()));
            assert!(eight.chars().all(|c| c.is_ascii_digit()));
        }
    }

    #[test]
    fn totp_reports_the_time_remaining_in_the_step() {
        let secret = rfc_secret_sha1();
        let params = OtpParameters {
            period_seconds: 30,
            ..Default::default()
        };
        assert_eq!(totp_at(&secret, &params, 0).valid_for_seconds, Some(30));
        assert_eq!(totp_at(&secret, &params, 1).valid_for_seconds, Some(29));
        assert_eq!(totp_at(&secret, &params, 29).valid_for_seconds, Some(1));
        assert_eq!(totp_at(&secret, &params, 30).valid_for_seconds, Some(30));
        assert_eq!(totp_at(&secret, &params, 30).counter, 1);
    }

    #[test]
    fn a_custom_period_changes_the_step() {
        let secret = rfc_secret_sha1();
        let sixty = OtpParameters {
            period_seconds: 60,
            ..Default::default()
        };
        assert_eq!(totp_at(&secret, &sixty, 59).counter, 0);
        assert_eq!(totp_at(&secret, &sixty, 60).counter, 1);
        assert_eq!(totp_at(&secret, &sixty, 61).valid_for_seconds, Some(59));
    }

    #[test]
    fn totp_verification_accepts_one_step_of_drift() {
        let secret = rfc_secret_sha1();
        let params = OtpParameters {
            digits: OtpDigits::Eight,
            period_seconds: 30,
            ..Default::default()
        };
        let now = 1_111_111_111;
        let current = totp_at(&secret, &params, now).code;
        let previous = totp_at(&secret, &params, now - 30).code;
        let next = totp_at(&secret, &params, now + 30).code;
        let far = totp_at(&secret, &params, now + 120).code;

        assert!(verify_totp(&secret, &params, &current, now, TotpWindow(1)));
        assert!(verify_totp(&secret, &params, &previous, now, TotpWindow(1)));
        assert!(verify_totp(&secret, &params, &next, now, TotpWindow(1)));
        assert!(!verify_totp(&secret, &params, &far, now, TotpWindow(1)));
        assert!(!verify_totp(&secret, &params, "00000000", now, TotpWindow(1)));
        assert!(!verify_totp(&secret, &params, "", now, TotpWindow(1)));
        assert!(!verify_totp(&secret, &params, &previous, now, TotpWindow(0)));
    }

    #[test]
    fn hotp_verification_resynchronises_within_the_look_ahead() {
        let secret = rfc_secret_sha1();
        let params = OtpParameters {
            kind: OtpKind::Hotp,
            ..Default::default()
        };
        // "969429" is counter 3 from the RFC 4226 table.
        assert_eq!(verify_hotp(&secret, &params, "969429", 0, 5), Some(3));
        assert_eq!(verify_hotp(&secret, &params, "969429", 0, 2), None);
        assert_eq!(verify_hotp(&secret, &params, "000000", 0, 10), None);
        assert_eq!(
            verify_hotp(&secret, &params, "755224", u64::MAX, 3),
            None,
            "no overflow panic"
        );
    }

    #[test]
    fn generate_dispatches_on_the_configured_kind() {
        let secret = rfc_secret_sha1();
        let hotp_params = OtpParameters {
            kind: OtpKind::Hotp,
            counter: 3,
            ..Default::default()
        };
        let produced = generate(&secret, &hotp_params);
        assert_eq!(produced.code, "969429");
        assert_eq!(produced.valid_for_seconds, None);
        assert_eq!(produced.counter, 3);

        let totp_params = OtpParameters::default();
        assert!(generate(&secret, &totp_params).valid_for_seconds.is_some());
    }

    #[test]
    fn a_zero_period_cannot_divide_by_zero() {
        let secret = rfc_secret_sha1();
        let params = OtpParameters {
            period_seconds: 0,
            ..Default::default()
        };
        let code = totp_at(&secret, &params, 1234);
        assert_eq!(code.code.len(), 6);
    }
}
