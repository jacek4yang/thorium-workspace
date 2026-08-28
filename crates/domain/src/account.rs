//! Accounts, their second factors and their recovery codes.
//!
//! The account type is generic. GitHub and Microsoft appear only as *presets*
//! that pre-fill a form; nothing in the core branches on them.

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{DomainError, DomainResult};
use crate::ids::{AccountId, FactorId, RecoveryCodeId, SecretRef};
use crate::otp::OtpParameters;
use crate::time::Timestamp;
use crate::validation::{MAX_NOTES, normalize_tag_list, validate_display_name, validate_login_url};

/// What kind of service an account belongs to.
///
/// `Other` carries the user's own label so the model never has to grow a variant
/// for every service someone stores credentials for.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "label", rename_all = "snake_case")]
pub enum ServiceKind {
    /// A GitHub account.
    GitHub,
    /// A Microsoft account (personal, work or school).
    Microsoft,
    /// A generic account with a user-provided service label.
    Other(String),
}

impl ServiceKind {
    /// The label shown in lists.
    #[must_use]
    pub fn label(&self) -> &str {
        match self {
            Self::GitHub => "GitHub",
            Self::Microsoft => "Microsoft",
            Self::Other(label) => label,
        }
    }

    /// The stable storage discriminant.
    #[must_use]
    pub const fn discriminant(&self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::Microsoft => "microsoft",
            Self::Other(_) => "other",
        }
    }

    /// Rebuilds a value from its stored discriminant and label.
    ///
    /// # Errors
    ///
    /// Returns [`crate::DiagnosticCode::InvalidInput`] for an unknown
    /// discriminant.
    pub fn from_parts(discriminant: &str, label: &str) -> DomainResult<Self> {
        match discriminant {
            "github" => Ok(Self::GitHub),
            "microsoft" => Ok(Self::Microsoft),
            "other" => Ok(Self::Other(validate_display_name(label)?)),
            other => Err(DomainError::invalid(format!("unknown service kind '{other}'"))),
        }
    }
}

impl fmt::Display for ServiceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A form pre-fill for a well-known service.
///
/// Presets are pure data: adding one must never require a change anywhere else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicePreset {
    /// Stable preset identifier used by the frontend.
    pub id: &'static str,
    /// Name shown in the preset picker.
    pub name: &'static str,
    /// The service kind the preset selects.
    pub kind: ServiceKind,
    /// Sign-in page pre-filled into the account form.
    pub login_url: &'static str,
    /// Where the user manages two-factor authentication for this service.
    pub two_factor_url: &'static str,
    /// Short note explaining anything unusual about the service's 2FA.
    pub note: &'static str,
}

/// Returns the built-in service presets.
#[must_use]
pub fn service_presets() -> Vec<ServicePreset> {
    vec![
        ServicePreset {
            id: "github",
            name: "GitHub",
            kind: ServiceKind::GitHub,
            login_url: "https://github.com/login",
            two_factor_url: "https://github.com/settings/security",
            note: "GitHub provisions standard TOTP and downloadable recovery codes.",
        },
        ServicePreset {
            id: "microsoft",
            name: "Microsoft",
            kind: ServiceKind::Microsoft,
            login_url: "https://login.microsoftonline.com/",
            two_factor_url: "https://account.microsoft.com/security",
            note: "Only the standard TOTP option is supported here. Microsoft Authenticator \
                   push approval, number matching and passwordless sign-in are not TOTP and \
                   are not emulated; record those as an external authenticator instead.",
        },
        ServicePreset {
            id: "generic",
            name: "Other service",
            kind: ServiceKind::Other(String::new()),
            login_url: "",
            two_factor_url: "",
            note: "Any service that provisions a standard otpauth:// TOTP or HOTP secret.",
        },
    ]
}

/// The kind of second factor recorded against an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecondFactorKind {
    /// A standards-based `otpauth://` TOTP or HOTP factor this app generates
    /// codes for.
    Otp,
    /// A factor handled entirely by another application or device: a vendor push
    /// approval, a hardware security key, an SMS code.
    ///
    /// Recorded so the user knows the factor exists. This app never attempts to
    /// satisfy it.
    ExternalAuthenticator,
}

impl SecondFactorKind {
    /// The stable storage discriminant.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Otp => "otp",
            Self::ExternalAuthenticator => "external_authenticator",
        }
    }
}

impl FromStr for SecondFactorKind {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "otp" => Ok(Self::Otp),
            "external_authenticator" => Ok(Self::ExternalAuthenticator),
            other => Err(DomainError::invalid(format!(
                "unknown second factor kind '{other}'"
            ))),
        }
    }
}

/// A second factor attached to an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecondFactor {
    /// Identity.
    pub id: FactorId,
    /// Owning account.
    pub account_id: AccountId,
    /// User-visible label, for example `Authenticator app`.
    pub label: String,
    /// Which kind of factor this is.
    pub kind: SecondFactorKind,
    /// Generation parameters. `None` for external authenticators.
    pub otp: Option<OtpParameters>,
    /// Vault reference for the shared secret. `None` for external
    /// authenticators.
    pub seed_ref: Option<SecretRef>,
    /// Creation time.
    pub created_at: Timestamp,
    /// Last modification time.
    pub updated_at: Timestamp,
}

impl SecondFactor {
    /// Returns `true` when this app can generate codes for the factor.
    #[must_use]
    pub const fn generates_codes(&self) -> bool {
        matches!(self.kind, SecondFactorKind::Otp) && self.seed_ref.is_some()
    }

    /// Checks the factor's internal consistency.
    ///
    /// # Errors
    ///
    /// Returns [`crate::DiagnosticCode::InvalidInput`] when an OTP factor is
    /// missing parameters or a seed, or an external factor carries either.
    pub fn validate(&self) -> DomainResult<()> {
        match self.kind {
            SecondFactorKind::Otp => {
                let params = self
                    .otp
                    .as_ref()
                    .ok_or_else(|| DomainError::invalid("an OTP factor requires OTP parameters"))?;
                params.validate()?;
                if self.seed_ref.is_none() {
                    return Err(DomainError::invalid("an OTP factor requires a stored secret"));
                }
            }
            SecondFactorKind::ExternalAuthenticator => {
                if self.otp.is_some() || self.seed_ref.is_some() {
                    return Err(DomainError::invalid(
                        "an external authenticator factor must not carry OTP parameters or a secret",
                    ));
                }
            }
        }
        Ok(())
    }
}

/// Fields supplied when creating or updating a second factor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecondFactorDraft {
    /// User-visible label.
    pub label: String,
    /// Which kind of factor.
    pub kind: SecondFactorKind,
    /// Generation parameters, for OTP factors.
    pub otp: Option<OtpParameters>,
}

/// One recovery (backup) code.
///
/// The code itself is never stored here: `code_ref` points into the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryCode {
    /// Identity.
    pub id: RecoveryCodeId,
    /// Owning account.
    pub account_id: AccountId,
    /// Vault reference for the code text.
    pub code_ref: SecretRef,
    /// Stable ordering within the account's code list.
    pub position: u32,
    /// Whether the user has marked this code as spent.
    pub used: bool,
    /// When the code was marked used.
    pub used_at: Option<Timestamp>,
    /// Creation time.
    pub created_at: Timestamp,
}

/// An account: metadata plus references to its protected material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// Identity.
    pub id: AccountId,
    /// Name shown in lists.
    pub display_name: String,
    /// Which service the account belongs to.
    pub service: ServiceKind,
    /// Sign-in username, if distinct from the email address.
    pub username: Option<String>,
    /// Email address associated with the account.
    pub email: Option<String>,
    /// Sign-in page.
    pub login_url: Option<String>,
    /// Normalized tags.
    pub tags: Vec<String>,
    /// Free-text notes. Not a place for secrets; the UI says so.
    pub notes: String,
    /// Vault reference for the password.
    pub password_ref: Option<SecretRef>,
    /// Creation time.
    pub created_at: Timestamp,
    /// Last modification time.
    pub updated_at: Timestamp,
}

/// Fields supplied when creating or updating an account.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountDraft {
    /// Name shown in lists.
    pub display_name: String,
    /// Which service the account belongs to.
    pub service: Option<ServiceKind>,
    /// Sign-in username.
    pub username: Option<String>,
    /// Email address.
    pub email: Option<String>,
    /// Sign-in page.
    pub login_url: Option<String>,
    /// Raw tags; normalized on validation.
    pub tags: Vec<String>,
    /// Free-text notes.
    pub notes: String,
}

impl AccountDraft {
    /// Validates and normalizes the draft into the fields an [`Account`] needs.
    ///
    /// # Errors
    ///
    /// Returns [`crate::DiagnosticCode::InvalidInput`] when the display name,
    /// tags, notes or login URL are unacceptable.
    pub fn normalize(&self) -> DomainResult<NormalizedAccount> {
        let display_name = validate_display_name(&self.display_name)?;
        let tags = normalize_tag_list(&self.tags)?;
        if self.notes.chars().count() > MAX_NOTES {
            return Err(DomainError::invalid(format!(
                "notes must be at most {MAX_NOTES} characters"
            )));
        }
        let login_url = match self.login_url.as_deref().map(str::trim) {
            None | Some("") => None,
            Some(raw) => Some(validate_login_url(raw)?),
        };
        let username = non_empty(self.username.as_deref());
        let email = non_empty(self.email.as_deref());
        let service = self
            .service
            .clone()
            .unwrap_or(ServiceKind::Other("Other".to_owned()));
        if let ServiceKind::Other(label) = &service {
            validate_display_name(label)?;
        }
        Ok(NormalizedAccount {
            display_name,
            service,
            username,
            email,
            login_url,
            tags,
            notes: self.notes.trim().to_owned(),
        })
    }
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned)
}

/// The validated result of normalizing an [`AccountDraft`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedAccount {
    /// Validated display name.
    pub display_name: String,
    /// Resolved service kind.
    pub service: ServiceKind,
    /// Trimmed username.
    pub username: Option<String>,
    /// Trimmed email.
    pub email: Option<String>,
    /// Validated login URL.
    pub login_url: Option<String>,
    /// Normalized tags.
    pub tags: Vec<String>,
    /// Trimmed notes.
    pub notes: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> AccountDraft {
        AccountDraft {
            display_name: "  Build bot  ".to_owned(),
            service: Some(ServiceKind::GitHub),
            username: Some("  bot ".to_owned()),
            email: Some("   ".to_owned()),
            login_url: Some(" https://github.com/login ".to_owned()),
            tags: vec!["CI".to_owned(), "ci".to_owned(), " ".to_owned()],
            notes: "  keep me  ".to_owned(),
        }
    }

    #[test]
    fn drafts_normalize_every_field() {
        let n = draft().normalize().expect("valid");
        assert_eq!(n.display_name, "Build bot");
        assert_eq!(n.username.as_deref(), Some("bot"));
        assert_eq!(n.email, None, "whitespace-only email becomes absent");
        assert_eq!(n.login_url.as_deref(), Some("https://github.com/login"));
        assert_eq!(n.tags, vec!["ci".to_owned()]);
        assert_eq!(n.notes, "keep me");
    }

    #[test]
    fn drafts_reject_bad_login_urls() {
        let mut d = draft();
        d.login_url = Some("file:///etc/passwd".to_owned());
        assert!(d.normalize().is_err());
    }

    #[test]
    fn drafts_default_to_a_generic_service() {
        let mut d = draft();
        d.service = None;
        assert_eq!(
            d.normalize().expect("valid").service,
            ServiceKind::Other("Other".to_owned())
        );
    }

    #[test]
    fn service_kind_round_trips_through_storage_parts() {
        for kind in [
            ServiceKind::GitHub,
            ServiceKind::Microsoft,
            ServiceKind::Other("Fastmail".into()),
        ] {
            let restored = ServiceKind::from_parts(kind.discriminant(), kind.label()).expect("round trip");
            assert_eq!(restored, kind);
        }
        assert!(ServiceKind::from_parts("wat", "").is_err());
    }

    #[test]
    fn otp_factors_require_parameters_and_a_secret() {
        let now = Timestamp::now();
        let account_id = AccountId::new();
        let mut factor = SecondFactor {
            id: FactorId::new(),
            account_id,
            label: "Authenticator".to_owned(),
            kind: SecondFactorKind::Otp,
            otp: Some(OtpParameters::default()),
            seed_ref: Some(SecretRef::new()),
            created_at: now,
            updated_at: now,
        };
        assert!(factor.validate().is_ok());
        assert!(factor.generates_codes());

        factor.seed_ref = None;
        assert!(factor.validate().is_err());

        factor.kind = SecondFactorKind::ExternalAuthenticator;
        assert!(
            factor.validate().is_err(),
            "external factors must not carry OTP parameters"
        );
        factor.otp = None;
        assert!(factor.validate().is_ok());
        assert!(!factor.generates_codes());
    }

    #[test]
    fn presets_are_data_only_and_include_a_generic_option() {
        let presets = service_presets();
        assert!(presets.iter().any(|p| p.id == "github"));
        assert!(presets.iter().any(|p| p.id == "microsoft"));
        assert!(presets.iter().any(|p| matches!(p.kind, ServiceKind::Other(_))));
        let microsoft = presets.iter().find(|p| p.id == "microsoft").expect("preset");
        assert!(microsoft.note.contains("not emulated"));
    }
}
