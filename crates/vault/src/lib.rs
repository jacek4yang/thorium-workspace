//! Versioned encrypted vault for account secrets.
//!
//! Format: Argon2id key derivation + ChaCha20-Poly1305 authenticated
//! encryption. See `docs/DECISIONS.md` for the KDBX4 evaluation that led to
//! this custom format.

#![forbid(unsafe_code)]
