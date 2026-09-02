//! Portable Thorium release discovery, download, and version management.
//!
//! Uses official upstream Windows portable releases (GitHub-hosted). Installs
//! are staged, validated, and promoted atomically under
//! `browsers/thorium/versions/<version>/`.

#![forbid(unsafe_code)]
