//! Windows platform layer for Thorium Workspace.
//!
//! All Win32 FFI in the workspace is confined to this crate. Every unsafe
//! block here must document why unsafe is necessary, pointer/lifetime
//! assumptions, handle ownership, and the cleanup invariant.
//!
//! The crate is Windows-only by design; the product targets Windows 10/11.
