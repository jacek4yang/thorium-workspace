//! Metadata storage.
//!
//! SQLite holds everything that is *not* a secret: workspace settings, browser
//! profiles, accounts, factor metadata, recovery-code status, Thorium
//! installations and observed runtime state. Passwords, OTP seeds and recovery
//! code text live in the vault; this database stores only the opaque
//! [`tw_domain::SecretRef`] that points at them.
//!
//! The schema is versioned with `PRAGMA user_version` and advanced by explicit,
//! ordered migrations. Opening a database written by a newer build fails
//! loudly rather than corrupting it.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod database;
mod error;
mod migrations;
mod repo;

/// Re-exported so callers can name a `Connection` without depending on a
/// specific `rusqlite` version themselves.
pub use rusqlite;

pub use database::{Database, DatabaseOptions};
pub use error::{StorageError, StorageResult};
pub use migrations::{Migration, SCHEMA_VERSION, migrations};
pub use repo::{
    AccountRecord, AccountRepo, ProfileRepo, RecoveryCodeRepo, RuntimeRepo, RuntimeSession, SecondFactorRepo,
    SettingsRepo, ThoriumRepo,
};
