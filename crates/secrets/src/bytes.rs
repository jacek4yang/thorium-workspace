use core::fmt;

use serde::{Serialize, Serializer};
use zeroize::Zeroize;

use crate::REDACTED;

/// Arbitrary secret bytes (derived keys, decoded OTP seeds, nonces in flight).
#[derive(Clone, Default)]
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    /// Wraps `value` as secret bytes.
    #[must_use]
    pub fn new(value: impl Into<Vec<u8>>) -> Self {
        Self(value.into())
    }

    /// Allocates `len` zero bytes, ready to be filled in place.
    #[must_use]
    pub fn zeroed(len: usize) -> Self {
        Self(vec![0u8; len])
    }

    /// Returns the protected bytes.
    #[must_use]
    pub fn expose(&self) -> &[u8] {
        &self.0
    }

    /// Returns the protected bytes mutably so they can be filled in place.
    #[must_use]
    pub fn expose_mut(&mut self) -> &mut [u8] {
        &mut self.0
    }

    /// Returns the length of the protected bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` when there are no protected bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl From<Vec<u8>> for SecretBytes {
    fn from(value: Vec<u8>) -> Self {
        Self(value)
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretBytes({REDACTED}; {} bytes)", self.0.len())
    }
}

impl fmt::Display for SecretBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(REDACTED)
    }
}

impl PartialEq for SecretBytes {
    fn eq(&self, other: &Self) -> bool {
        crate::constant_time_eq(&self.0, &other.0)
    }
}

impl Eq for SecretBytes {}

impl Serialize for SecretBytes {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(REDACTED)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_reveals_only_the_length() {
        let b = SecretBytes::new(vec![0xde, 0xad, 0xbe, 0xef]);
        let rendered = format!("{b:?}");
        assert_eq!(rendered, "SecretBytes([redacted]; 4 bytes)");
        assert!(!rendered.contains("deadbeef"));
        assert!(!rendered.contains("222"));
    }

    #[test]
    fn zeroed_allocates_and_can_be_filled() {
        let mut b = SecretBytes::zeroed(8);
        assert_eq!(b.expose(), &[0u8; 8]);
        b.expose_mut()[0] = 1;
        assert_eq!(b.expose()[0], 1);
    }

    #[test]
    fn serialization_emits_the_redaction_marker() {
        let json = serde_json::to_string(&SecretBytes::new(vec![1, 2, 3])).expect("serialize");
        assert_eq!(json, "\"[redacted]\"");
    }
}
