//! Platform-independent domain model for Thorium Workspace.
//!
//! This crate must not depend on Tauri, React, Win32, SQLite, or HTTP
//! clients. It owns the entity types, identifiers, validation rules, and
//! the stable diagnostic-code vocabulary.

#![forbid(unsafe_code)]

pub mod account;
pub mod error;
pub mod factor;
pub mod ids;
pub mod profile;
pub mod recovery;
pub mod secret_ref;
pub mod service;
pub mod settings;
pub mod validation;

pub use account::{Account, AccountInput, ValidatedAccountInput};
pub use error::{DiagnosticCode, DomainError};
pub use factor::{
    FactorKind, MAX_TOTP_PERIOD_SECONDS, OtpAlgorithm, SecondFactor, validate_factor_params,
};
pub use ids::{AccountId, FactorId, ProfileId, RecoveryCodeId};
pub use profile::{BrowserProfile, ProfileInput, ThoriumSelection, ValidatedProfileInput};
pub use recovery::RecoveryCode;
pub use secret_ref::SecretRef;
pub use service::{KNOWN_SERVICES, ServiceKind};
pub use settings::{
    PROXY_SCHEMES, ThemePreference, ThoriumInstall, WorkspaceSettings, validate_proxy_url,
};
pub use validation::{HttpUrl, IanaTimeZone, LocaleTag, Name};
