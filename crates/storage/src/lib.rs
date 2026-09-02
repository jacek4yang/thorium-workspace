//! SQLite persistence and schema migrations for workspace metadata.
//!
//! Only non-secret metadata is stored here. Account secrets live in the
//! vault crate's encrypted format.

#![forbid(unsafe_code)]
