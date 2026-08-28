//! One-time passwords.
//!
//! Implements HOTP (RFC 4226) and TOTP (RFC 6238) over HMAC-SHA-1, HMAC-SHA-256
//! and HMAC-SHA-512, plus parsing and generation of `otpauth://` key URIs
//! (Google Authenticator's de-facto format).
//!
//! Nothing in this crate logs. A raw `otpauth://` URI contains the shared
//! secret, so it must never reach a log line, an error message or a panic
//! payload; [`OtpUriError`] therefore describes *what* was wrong without
//! quoting the input.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod generate;
mod secret;
mod uri;

pub use generate::{OtpCode, TotpWindow, generate, hotp, totp, totp_at, verify_hotp, verify_totp};
pub use secret::{MAX_SECRET_BYTES, MIN_SECRET_BYTES, OtpSecret};
pub use uri::{OtpCredential, OtpUriError, build_otpauth_uri, parse_otpauth_uri};
