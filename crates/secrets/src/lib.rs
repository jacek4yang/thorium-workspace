//! Redacting secret wrapper and secret-handling policy for Thorium Workspace.
//!
//! Types in this crate guarantee that secret material never renders through
//! `Debug`/`Display` and is zeroized when dropped.

#![forbid(unsafe_code)]
