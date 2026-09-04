//! RFC 4226 / RFC 6238 OTP computation.

use thorium_workspace_domain::OtpAlgorithm;

use crate::error::OtpError;

/// Computes an HOTP value (RFC 4226) for the given key and counter.
///
/// `key` is the raw seed bytes (already base32-decoded when imported from
/// an `otpauth://` URI). The result is a zero-padded decimal string.
pub fn hotp(
    key: &[u8],
    counter: u64,
    algorithm: OtpAlgorithm,
    digits: u8,
) -> Result<String, OtpError> {
    if !matches!(digits, 6 | 8) {
        return Err(OtpError::ParameterOutOfRange {
            parameter: "digits",
        });
    }
    let message = counter.to_be_bytes();
    let digest = hmac_digest(algorithm, key, &message)?;
    let code = dynamic_truncate(&digest) % 10u32.pow(digits as u32);
    Ok(format!("{code:0width$}", width = digits as usize))
}

/// Computes a TOTP value (RFC 6238) at a specific Unix time.
pub fn totp(
    key: &[u8],
    unix_time: u64,
    period_seconds: u32,
    algorithm: OtpAlgorithm,
    digits: u8,
) -> Result<String, OtpError> {
    if period_seconds == 0 {
        return Err(OtpError::ParameterOutOfRange {
            parameter: "period",
        });
    }
    let counter = unix_time / u64::from(period_seconds);
    hotp(key, counter, algorithm, digits)
}

/// Seconds remaining in the current TOTP window at a given Unix time.
pub fn seconds_remaining(unix_time: u64, period_seconds: u32) -> u32 {
    if period_seconds == 0 {
        return 0;
    }
    let elapsed_in_window = unix_time % u64::from(period_seconds);
    (u64::from(period_seconds) - elapsed_in_window).min(u64::from(u32::MAX)) as u32
}

fn hmac_digest(algorithm: OtpAlgorithm, key: &[u8], message: &[u8]) -> Result<Vec<u8>, OtpError> {
    use hmac::{KeyInit, Mac};
    match algorithm {
        OtpAlgorithm::Sha1 => {
            let mut mac = <hmac::Hmac<sha1::Sha1>>::new_from_slice(key)
                .map_err(|_| OtpError::InvalidSecret)?;
            mac.update(message);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        OtpAlgorithm::Sha256 => {
            let mut mac = <hmac::Hmac<sha2::Sha256>>::new_from_slice(key)
                .map_err(|_| OtpError::InvalidSecret)?;
            mac.update(message);
            Ok(mac.finalize().into_bytes().to_vec())
        }
        OtpAlgorithm::Sha512 => {
            let mut mac = <hmac::Hmac<sha2::Sha512>>::new_from_slice(key)
                .map_err(|_| OtpError::InvalidSecret)?;
            mac.update(message);
            Ok(mac.finalize().into_bytes().to_vec())
        }
    }
}

/// RFC 4226 dynamic truncation.
fn dynamic_truncate(digest: &[u8]) -> u32 {
    debug_assert!(
        digest.len() >= 20,
        "HMAC-SHA1 digest is the shortest supported"
    );
    let offset = (digest[digest.len() - 1] & 0x0f) as usize;
    let word = [
        digest[offset],
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ];
    u32::from_be_bytes(word) & 0x7fff_ffff
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 4226 Appendix D test vectors (ASCII key, 6 digits, HMAC-SHA-1).
    const RFC4226_KEY: &[u8] = b"12345678901234567890";
    const RFC4226_VECTORS: [(&u64, &str); 10] = [
        (&0, "755224"),
        (&1, "287082"),
        (&2, "359152"),
        (&3, "969429"),
        (&4, "338314"),
        (&5, "254676"),
        (&6, "287922"),
        (&7, "162583"),
        (&8, "399871"),
        (&9, "520489"),
    ];

    #[test]
    fn rfc4226_hotp_vectors() {
        for (counter, expected) in RFC4226_VECTORS {
            let code = hotp(RFC4226_KEY, *counter, OtpAlgorithm::Sha1, 6).expect("computable");
            assert_eq!(&code, expected, "counter {counter}");
        }
    }

    #[test]
    fn hotp_rejects_unsupported_digit_counts() {
        assert!(hotp(RFC4226_KEY, 0, OtpAlgorithm::Sha1, 7).is_err());
        assert!(hotp(RFC4226_KEY, 0, OtpAlgorithm::Sha1, 0).is_err());
    }

    // RFC 6238 Appendix B test vectors (8 digits, T0=0, X=30s).
    const RFC6238_SHA1_KEY: &[u8] = b"12345678901234567890";
    const RFC6238_SHA256_KEY: &[u8] = b"12345678901234567890123456789012";
    const RFC6238_SHA512_KEY: &[u8] =
        b"1234567890123456789012345678901234567890123456789012345678901234";

    #[test]
    fn rfc6238_sha1_vectors() {
        let cases = [
            (59u64, "94287082"),
            (1111111109, "07081804"),
            (1111111111, "14050471"),
            (1234567890, "89005924"),
            (2000000000, "69279037"),
            (20000000000, "65353130"),
        ];
        for (time, expected) in cases {
            let code = totp(RFC6238_SHA1_KEY, time, 30, OtpAlgorithm::Sha1, 8).expect("computable");
            assert_eq!(code, expected, "time {time}");
        }
    }

    #[test]
    fn rfc6238_sha256_vectors() {
        let cases = [
            (59u64, "46119246"),
            (1111111109, "68084774"),
            (1111111111, "67062674"),
            (1234567890, "91819424"),
            (2000000000, "90698825"),
            (20000000000, "77737706"),
        ];
        for (time, expected) in cases {
            let code =
                totp(RFC6238_SHA256_KEY, time, 30, OtpAlgorithm::Sha256, 8).expect("computable");
            assert_eq!(code, expected, "time {time}");
        }
    }

    #[test]
    fn rfc6238_sha512_vectors() {
        let cases = [
            (59u64, "90693936"),
            (1111111109, "25091201"),
            (1111111111, "99943326"),
            (1234567890, "93441116"),
            (2000000000, "38618901"),
            (20000000000, "47863826"),
        ];
        for (time, expected) in cases {
            let code =
                totp(RFC6238_SHA512_KEY, time, 30, OtpAlgorithm::Sha512, 8).expect("computable");
            assert_eq!(code, expected, "time {time}");
        }
    }

    #[test]
    fn six_digit_totp_is_supported() {
        // Same vector set truncated to 6 digits (RFC 6238 also documents
        // these for SHA-1 at T=59: 287082, matching RFC 4226 counter 1).
        let code = totp(RFC6238_SHA1_KEY, 59, 30, OtpAlgorithm::Sha1, 6).expect("computable");
        assert_eq!(code, "287082");
    }

    #[test]
    fn totp_rejects_zero_period() {
        assert!(totp(RFC6238_SHA1_KEY, 0, 0, OtpAlgorithm::Sha1, 6).is_err());
    }

    #[test]
    fn seconds_remaining_wraps_within_window() {
        // At t=59 with period 30 we are 29s into the window: 1s remains.
        assert_eq!(seconds_remaining(59, 30), 1);
        // Exactly on a window boundary: a full period remains.
        assert_eq!(seconds_remaining(60, 30), 30);
        assert_eq!(seconds_remaining(0, 30), 30);
    }
}
