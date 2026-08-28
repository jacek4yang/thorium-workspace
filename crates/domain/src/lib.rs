//! The Thorium Workspace domain model.
//!
//! This crate is deliberately free of Tauri, React, Win32, SQLite and HTTP: it
//! describes *what* a workspace contains, never *how* it is stored, rendered or
//! launched. Everything here compiles on any platform so the rules can be tested
//! without a Windows host.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod account;
pub mod diagnostics;
pub mod error;
pub mod ids;
pub mod otp;
pub mod profile;
pub mod settings;
pub mod thorium;
pub mod time;
pub mod validation;

pub use account::{
    Account, AccountDraft, RecoveryCode, SecondFactor, SecondFactorDraft, SecondFactorKind, ServiceKind,
    ServicePreset,
};
pub use diagnostics::DiagnosticCode;
pub use error::{DomainError, DomainResult};
pub use ids::{AccountId, FactorId, ProfileId, RecoveryCodeId, SecretRef};
pub use otp::{OtpAlgorithm, OtpDigits, OtpKind, OtpParameters};
pub use profile::{BrowserProfile, BrowserProfileDraft, ProfileRuntimeStatus, ThoriumSelection};
pub use settings::{ClipboardSettings, ThemePreference, VaultSettings, WorkspaceSettings};
pub use thorium::{ThoriumChannel, ThoriumInstallation, ThoriumRelease, ThoriumReleaseAsset};
pub use time::Timestamp;
pub use validation::{
    LocaleTag, TimeZoneId, normalize_tag_list, validate_display_name, validate_login_url,
    validate_startup_url,
};

/// Schema/domain revision. Bumped when persisted domain semantics change.
pub const DOMAIN_VERSION: u32 = 1;
