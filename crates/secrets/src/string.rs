use core::fmt;

use serde::{Serialize, Serializer};
use zeroize::{Zeroize, Zeroizing};

use crate::REDACTED;

/// A UTF-8 secret (password, OTP seed, recovery code) that never formats itself.
///
/// Cloning duplicates the protected bytes; both copies zeroize independently.
#[derive(Clone, Default)]
pub struct SecretString(String);

impl SecretString {
    /// Wraps `value` as a secret.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Returns the protected value.
    ///
    /// Every call site is an explicit, reviewable decision to handle plaintext.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Returns the protected value as bytes.
    #[must_use]
    pub fn expose_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    /// Returns the length in bytes of the protected value.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` when the protected value is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Consumes the secret and returns the plaintext in a zeroizing container.
    #[must_use]
    pub fn into_zeroizing(mut self) -> Zeroizing<String> {
        Zeroizing::new(core::mem::take(&mut self.0))
    }
}

impl From<String> for SecretString {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for SecretString {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(")?;
        f.write_str(REDACTED)?;
        f.write_str(")")
    }
}

impl fmt::Display for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

/// Equality is *not* constant time and is only used by tests and de-duplication
/// of values the caller already holds in plaintext.
impl PartialEq for SecretString {
    fn eq(&self, other: &Self) -> bool {
        crate::constant_time_eq(self.expose_bytes(), other.expose_bytes())
    }
}

impl Eq for SecretString {}

/// Serialization emits the redaction marker so a secret cannot escape through an
/// accidentally-serialized struct. Vault payloads serialize the exposed value
/// explicitly instead of relying on this impl.
impl Serialize for SecretString {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(REDACTED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CANARY: &str = "canary-plaintext-value";

    #[test]
    fn debug_does_not_contain_the_secret() {
        let s = SecretString::new(CANARY);
        let rendered = format!("{s:?}");
        assert!(!rendered.contains(CANARY), "{rendered}");
        assert_eq!(rendered, "SecretString([redacted])");
    }

    #[test]
    fn display_does_not_contain_the_secret() {
        let s = SecretString::new(CANARY);
        assert_eq!(format!("{s}"), REDACTED);
    }

    #[test]
    fn nested_debug_does_not_contain_the_secret() {
        #[derive(Debug)]
        #[allow(dead_code)]
        struct Holder {
            name: &'static str,
            password: SecretString,
        }
        let h = Holder {
            name: "acct",
            password: SecretString::new(CANARY),
        };
        let rendered = format!("{h:#?}");
        assert!(!rendered.contains(CANARY), "{rendered}");
    }

    #[test]
    fn serialization_emits_the_redaction_marker() {
        let json = serde_json::to_string(&SecretString::new(CANARY)).expect("serialize");
        assert_eq!(json, "\"[redacted]\"");
    }

    #[test]
    fn expose_returns_the_plaintext() {
        assert_eq!(SecretString::new(CANARY).expose(), CANARY);
    }

    #[test]
    fn equality_is_by_value() {
        assert_eq!(SecretString::new("a"), SecretString::new("a"));
        assert_ne!(SecretString::new("a"), SecretString::new("b"));
        assert_ne!(SecretString::new("a"), SecretString::new("ab"));
    }
}
