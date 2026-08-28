//! The shared OTP secret.

use tw_secrets::{SecretBytes, SecretString};
use zeroize::Zeroize;

use crate::uri::OtpUriError;

/// Shortest secret accepted, in bytes.
///
/// RFC 4226 requires at least 128 bits and recommends 160. Shorter secrets are
/// rejected because accepting them would silently weaken a factor the user
/// believes is standard.
pub const MIN_SECRET_BYTES: usize = 16;

/// Longest secret accepted, in bytes. Bounds a hostile QR payload.
pub const MAX_SECRET_BYTES: usize = 1024;

/// A decoded shared secret, kept in zeroizing storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtpSecret(SecretBytes);

impl OtpSecret {
    /// Decodes a Base32 secret as written in an `otpauth://` URI.
    ///
    /// Accepts the padded and unpadded forms, ignores spaces (issuers often
    /// print secrets in groups of four) and is case-insensitive.
    ///
    /// # Errors
    ///
    /// Returns [`OtpUriError::SecretNotBase32`] for undecodable input and
    /// [`OtpUriError::SecretLength`] when the decoded secret is outside
    /// [`MIN_SECRET_BYTES`]..=[`MAX_SECRET_BYTES`].
    pub fn from_base32(encoded: &str) -> Result<Self, OtpUriError> {
        let mut cleaned: String = encoded
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '-')
            .collect::<String>()
            .to_uppercase();
        // Try unpadded first (what issuers actually emit), then padded.
        let decoded = base32::decode(base32::Alphabet::Rfc4648 { padding: false }, &cleaned)
            .or_else(|| base32::decode(base32::Alphabet::Rfc4648 { padding: true }, &cleaned));
        cleaned.zeroize();
        let bytes = decoded.ok_or(OtpUriError::SecretNotBase32)?;
        Self::from_bytes(bytes)
    }

    /// Wraps already-decoded secret bytes.
    ///
    /// # Errors
    ///
    /// Returns [`OtpUriError::SecretLength`] when the length is out of range.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, OtpUriError> {
        if !(MIN_SECRET_BYTES..=MAX_SECRET_BYTES).contains(&bytes.len()) {
            let mut bytes = bytes;
            bytes.zeroize();
            return Err(OtpUriError::SecretLength);
        }
        Ok(Self(SecretBytes::new(bytes)))
    }

    /// Wraps secret bytes without the length check.
    ///
    /// Only for RFC test vectors, whose 20-byte `12345678901234567890` secret is
    /// fine but whose SHA-512 variant is 64 bytes and whose truncated variants
    /// are deliberately unusual.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn from_bytes_unchecked(bytes: Vec<u8>) -> Self {
        Self(SecretBytes::new(bytes))
    }

    /// The decoded secret bytes.
    #[must_use]
    pub fn expose(&self) -> &[u8] {
        self.0.expose()
    }

    /// The secret length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the secret is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Re-encodes the secret as unpadded Base32, for storage in the vault.
    #[must_use]
    pub fn to_base32(&self) -> SecretString {
        SecretString::new(base32::encode(
            base32::Alphabet::Rfc4648 { padding: false },
            self.0.expose(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A synthetic secret. Never a real credential.
    const SYNTHETIC: &str = "JBSWY3DPEHPK3PXPJBSWY3DPEHPK3PXP";

    #[test]
    fn base32_decoding_tolerates_formatting_issuers_actually_use() {
        let plain = OtpSecret::from_base32(SYNTHETIC).expect("decode");
        let spaced = OtpSecret::from_base32("JBSW Y3DP EHPK 3PXP JBSW Y3DP EHPK 3PXP").expect("decode");
        let hyphenated = OtpSecret::from_base32("JBSW-Y3DP-EHPK-3PXP-JBSW-Y3DP-EHPK-3PXP").expect("decode");
        let lowercase = OtpSecret::from_base32(&SYNTHETIC.to_lowercase()).expect("decode");
        assert_eq!(plain, spaced);
        assert_eq!(plain, hyphenated);
        assert_eq!(plain, lowercase);
        assert_eq!(plain.len(), 20);
    }

    #[test]
    fn padded_base32_is_accepted() {
        // "12345678901234567890" padded.
        let padded = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
        assert!(OtpSecret::from_base32(padded).is_ok());
        assert!(
            OtpSecret::from_base32("MZXW6===").is_err(),
            "too short after decoding"
        );
    }

    #[test]
    fn invalid_base32_is_rejected_without_quoting_the_input() {
        let err = OtpSecret::from_base32("not base32 at all!!!").expect_err("must fail");
        assert!(matches!(err, OtpUriError::SecretNotBase32));
        assert!(!err.to_string().contains("not base32 at all"));
    }

    #[test]
    fn short_and_absurd_secrets_are_rejected() {
        assert!(matches!(
            OtpSecret::from_bytes(vec![0u8; MIN_SECRET_BYTES - 1]),
            Err(OtpUriError::SecretLength)
        ));
        assert!(OtpSecret::from_bytes(vec![0u8; MIN_SECRET_BYTES]).is_ok());
        assert!(matches!(
            OtpSecret::from_bytes(vec![0u8; MAX_SECRET_BYTES + 1]),
            Err(OtpUriError::SecretLength)
        ));
    }

    #[test]
    fn base32_round_trips() {
        let secret = OtpSecret::from_base32(SYNTHETIC).expect("decode");
        let encoded = secret.to_base32();
        assert_eq!(encoded.expose(), SYNTHETIC);
        assert_eq!(OtpSecret::from_base32(encoded.expose()).expect("decode"), secret);
    }

    #[test]
    fn the_secret_never_renders_itself() {
        let secret = OtpSecret::from_base32(SYNTHETIC).expect("decode");
        let rendered = format!("{secret:?}");
        assert!(!rendered.contains("JBSW"), "{rendered}");
        assert!(!rendered.contains(SYNTHETIC));
        assert!(!format!("{:?}", secret.to_base32()).contains("JBSW"));
    }
}
