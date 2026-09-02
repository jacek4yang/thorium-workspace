//! Browser profile launching, isolation, and CDP-based timezone/locale
//! overrides.
//!
//! Each profile launches Thorium with an explicit absolute
//! `--user-data-dir`. Duplicate launches of the same profile are refused or
//! focused instead of creating a conflicting session.

#![forbid(unsafe_code)]
