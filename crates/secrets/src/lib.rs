//! Redacting secret wrapper and secret-handling policy for Thorium Workspace.
//!
//! Types in this crate guarantee that secret material:
//!
//! - never renders through `Debug` (always shown as `<redacted>`);
//! - has no `Display` or `Serialize` implementation, so it cannot leak
//!   through formatting, logs, or IPC by accident;
//! - is zeroized when dropped (via [`secrecy::SecretBox`]);
//! - is only readable through an explicit `expose_*` call.
//!
//! ## Why a custom wrapper instead of raw `secrecy` types
//!
//! `secrecy::SecretBox` redacts `Debug` but *does* implement `Serialize`
//! when its `serde` feature is enabled (it serializes the plaintext value).
//! Wrapping it in our own types removes that implementation surface
//! entirely, so a secret cannot be serialized into a DTO, event payload, or
//! log line without going through an explicit reveal.

#![forbid(unsafe_code)]

use secrecy::{ExposeSecret, SecretBox};
use zeroize::Zeroize;

/// Secret string material (passwords, OTP seeds, recovery codes).
#[derive(Clone)]
pub struct SecretText {
    inner: SecretBox<str>,
}

/// Secret byte material.
#[derive(Clone)]
pub struct SecretBytes {
    inner: SecretBox<[u8]>,
}

impl SecretText {
    /// Wraps a string as secret material.
    pub fn new(value: &str) -> Self {
        Self {
            inner: SecretBox::new(Box::<str>::from(value)),
        }
    }

    /// Explicitly exposes the secret for a deliberate read.
    ///
    /// Every call site of this function is a place a human must justify in
    /// review. There is no implicit access path.
    pub fn expose(&self) -> &str {
        self.inner.expose_secret()
    }

    /// Converts to secret bytes (e.g. an OTP seed for HMAC).
    pub fn expose_as_bytes(&self) -> SecretBytes {
        SecretBytes {
            inner: SecretBox::new(Box::<[u8]>::from(self.inner.expose_secret().as_bytes())),
        }
    }

    /// Produces an intentional second copy (e.g. moving material into the
    /// vault writer while the original stays alive). Copies are also
    /// zeroized on drop.
    pub fn replicate(&self) -> Self {
        Self::new(self.inner.expose_secret())
    }
}

impl SecretBytes {
    /// Wraps bytes as secret material.
    pub fn new(value: &[u8]) -> Self {
        Self {
            inner: SecretBox::new(Box::<[u8]>::from(value)),
        }
    }

    /// Explicitly exposes the secret for a deliberate read.
    pub fn expose(&self) -> &[u8] {
        self.inner.expose_secret()
    }

    /// Produces an intentional second copy.
    pub fn replicate(&self) -> Self {
        Self::new(self.inner.expose_secret())
    }
}

impl From<&str> for SecretText {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for SecretText {
    fn from(value: String) -> Self {
        Self::new(&value)
    }
}

impl From<&[u8]> for SecretBytes {
    fn from(value: &[u8]) -> Self {
        Self::new(value)
    }
}

// Deliberate absence of trait impls:
// - No `Debug` beyond the redacting impls below.
// - No `Display`: prevents accidental interpolation into logs/UI.
// - No `Serialize`/`Deserialize`: prevents accidental IPC/DTO leakage.

impl core::fmt::Debug for SecretText {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SecretText(<redacted>)")
    }
}

impl core::fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SecretBytes(<redacted>)")
    }
}

// Equality is provided for local in-memory comparisons (tests, dedupe).
// It is NOT constant-time: never use it to verify material received from
// an untrusted party. Use [`constant_time_eq`] for that.
impl PartialEq for SecretText {
    fn eq(&self, other: &Self) -> bool {
        self.expose() == other.expose()
    }
}

impl Eq for SecretText {}

impl PartialEq for SecretBytes {
    fn eq(&self, other: &Self) -> bool {
        self.expose() == other.expose()
    }
}

impl Eq for SecretBytes {}

/// Compares a candidate value against secret material without exposing the
/// secret through the comparison itself. Uses a constant-time equality
/// check when the values have equal length (falling back to an early
/// mismatch on length differences, which is acceptable here because length
/// is not treated as sensitive metadata in this threat model).
pub fn constant_time_eq(candidate: &[u8], secret: &SecretBytes) -> bool {
    let exposed = secret.expose();
    if candidate.len() != exposed.len() {
        return false;
    }
    let mut difference: u8 = 0;
    for (a, b) in candidate.iter().zip(exposed.iter()) {
        difference |= a ^ b;
    }
    difference == 0
}

/// Overwrites the contents of a mutable buffer with zeros, for callers that
/// must scrub temporary plaintext buffers themselves.
pub fn scrub(buffer: &mut [u8]) {
    buffer.zeroize();
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYNTHETIC_PASSWORD: &str = "synthetic-test-password-123";

    #[test]
    fn debug_never_reveals_secret_text() {
        let secret = SecretText::new(SYNTHETIC_PASSWORD);
        let rendered = format!("{secret:?}");
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains(SYNTHETIC_PASSWORD));
    }

    #[test]
    fn debug_never_reveals_secret_bytes() {
        let secret = SecretBytes::new(SYNTHETIC_PASSWORD.as_bytes());
        let rendered = format!("{secret:?}");
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains(SYNTHETIC_PASSWORD));
    }

    #[test]
    fn nested_debug_rendering_stays_redacted() {
        #[derive(Debug)]
        #[allow(dead_code)] // only Debug-rendered in this test
        struct Wrapper {
            label: &'static str,
            secret: SecretText,
        }
        let wrapper = Wrapper {
            label: "account-password",
            secret: SecretText::new(SYNTHETIC_PASSWORD),
        };
        let rendered = format!("{wrapper:?}");
        assert!(rendered.contains("account-password"));
        assert!(!rendered.contains(SYNTHETIC_PASSWORD));
    }

    #[test]
    fn expose_returns_original_material() {
        let secret = SecretText::new(SYNTHETIC_PASSWORD);
        assert_eq!(secret.expose(), SYNTHETIC_PASSWORD);
        let bytes = secret.expose_as_bytes();
        assert_eq!(bytes.expose(), SYNTHETIC_PASSWORD.as_bytes());
    }

    #[test]
    fn replicate_keeps_material_isolated() {
        let original = SecretText::new(SYNTHETIC_PASSWORD);
        let copy = original.replicate();
        assert_eq!(copy.expose(), SYNTHETIC_PASSWORD);
    }

    #[test]
    fn constant_time_eq_matches_only_identical_material() {
        let secret = SecretBytes::new(b"synthetic-seed-value");
        assert!(constant_time_eq(b"synthetic-seed-value", &secret));
        assert!(!constant_time_eq(b"wrong-seed-value!!", &secret));
        assert!(!constant_time_eq(b"short", &secret));
    }

    #[test]
    fn scrub_zeros_buffers() {
        let mut buffer = *b"sensitive-temporary";
        scrub(&mut buffer);
        assert!(buffer.iter().all(|byte| *byte == 0));
    }
}
