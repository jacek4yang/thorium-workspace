//! Windows platform layer for Thorium Workspace.
//!
//! All Win32 FFI in the workspace is confined to this crate. Every unsafe
//! block here must document why unsafe is necessary, pointer/lifetime
//! assumptions, handle ownership, and the cleanup invariant.
//!
//! The crate is Windows-only by design; the product targets Windows 10/11.
//! Modules that need unsafe FFI keep it local; pure-Rust modules opt into
//! `#![forbid(unsafe_code)]` individually.
//!
//! Contents:
//! - [`paths`]: portable workspace bootstrap (exe-relative layout)
//! - [`mutex_name`] / [`mutex`]: single-instance named mutexes
//! - [`job`]: Job Objects with `KILL_ON_JOB_CLOSE`
//! - [`process`]: hidden-console spawning into a job
//! - [`clipboard`]: copy + conditional clear for secret values

pub mod clipboard;
pub mod error;
pub mod job;
pub mod mutex;
pub mod mutex_name;
pub mod paths;
pub mod process;
pub mod wide;

pub use error::PlatformError;
