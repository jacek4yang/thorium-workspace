//! QR decoding for 2FA imports.
//!
//! Decodes QR codes from image bytes and returns the payload text. Raw
//! payloads (including `otpauth://` URIs) are never logged by this crate.

#![forbid(unsafe_code)]
