//! Application services orchestrating workspace state and behavior.
//!
//! The controller owns the workspace lifecycle: portable bootstrap, storage,
//! vault, Thorium installs, and browser profile sessions. It is the only
//! layer the Tauri commands talk to.

#![forbid(unsafe_code)]
