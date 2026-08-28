//! Secret material wrappers.
//!
//! Every value in this crate is designed around one rule: *a secret must not be
//! observable through any incidental formatting path*. `Debug`, `Display`,
//! `serde::Serialize` and error formatting all render a fixed redaction marker
//! instead of the protected bytes.
//!
//! Secrets are zeroized on drop. Zeroization is best-effort: the operating
//! system may still have copied a page to the swap file, and a `String` that was
//! reallocated while being built may leave a copy behind. Construct secrets from
//! their final value where possible.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod bytes;
mod string;

pub use bytes::SecretBytes;
pub use string::SecretString;

/// The single marker rendered wherever a secret would otherwise be formatted.
pub const REDACTED: &str = "[redacted]";

/// Fills `buf` with cryptographically secure random bytes.
///
/// # Errors
///
/// Returns an error when the operating system entropy source is unavailable.
pub fn fill_random(buf: &mut [u8]) -> Result<(), RandomError> {
    getrandom::fill(buf).map_err(|_| RandomError)
}

/// Returns `len` cryptographically secure random bytes.
///
/// # Errors
///
/// Returns an error when the operating system entropy source is unavailable.
pub fn random_bytes(len: usize) -> Result<SecretBytes, RandomError> {
    let mut buf = vec![0u8; len];
    fill_random(&mut buf)?;
    Ok(SecretBytes::new(buf))
}

/// The operating system entropy source was unavailable.
///
/// Deliberately carries no detail: the underlying error text is not useful to a
/// user and the failure mode is always the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RandomError;

impl core::fmt::Display for RandomError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("the operating system random number generator is unavailable")
    }
}

impl std::error::Error for RandomError {}

/// Constant-time equality for two byte slices of any length.
///
/// Slices of differing length compare unequal without leaking which byte
/// differed.
#[must_use]
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    if a.len() != b.len() {
        return false;
    }
    a.ct_eq(b).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_matches_semantic_equality() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn random_bytes_are_the_requested_length_and_not_all_zero() {
        let a = random_bytes(32).expect("entropy");
        let b = random_bytes(32).expect("entropy");
        assert_eq!(a.expose().len(), 32);
        assert_ne!(a.expose(), &[0u8; 32]);
        assert_ne!(a.expose(), b.expose());
    }
}
