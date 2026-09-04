//! RFC 4226 HOTP and RFC 6238 TOTP with `otpauth://` URI support.
//!
//! Security posture:
//! - seed material crosses boundaries only as
//!   [`thorium_workspace_secrets::SecretBytes`];
//! - this crate never logs URIs, seeds, or codes;
//! - errors never embed rejected values.

#![forbid(unsafe_code)]

pub mod engine;
pub mod error;
pub mod otpauth;

pub use engine::{hotp, seconds_remaining, totp};
pub use error::OtpError;
pub use otpauth::{ParsedOtpAuth, parse_otpauth_uri};
