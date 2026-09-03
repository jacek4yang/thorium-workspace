//! Portable Thorium release discovery, download, and version management.
//!
//! Uses official upstream Windows portable releases (GitHub-hosted). Installs
//! are staged, validated, and promoted atomically under
//! `browsers/thorium/versions/<version>/`.

#![forbid(unsafe_code)]

pub mod catalog;
pub mod error;
pub mod install;
pub mod proxy;
pub mod releases;

pub use catalog::{Variant, WINDOWS_PORTABLE_ZIP_PATTERN};
pub use error::ThoriumError;
pub use install::InstallLayout;
pub use releases::{Client, Release};
