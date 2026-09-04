//! The generic account model.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::factor::SecondFactor;
use crate::ids::{AccountId, ProfileId};
use crate::recovery::RecoveryCode;
use crate::secret_ref::SecretRef;
use crate::service::ServiceKind;
use crate::validation::{self, HttpUrl, Name};

/// An account record belonging to a Browser Profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// Unique identifier.
    pub id: AccountId,
    /// Owning Browser Profile.
    pub profile_id: ProfileId,
    /// Display name.
    pub display_name: String,
    /// Service kind (preset or custom).
    pub service_kind: ServiceKind,
    /// Username on the service, if any.
    pub username: Option<String>,
    /// Email used to sign in, if any.
    pub email: Option<String>,
    /// Login URL, if any.
    pub login_url: Option<String>,
    /// Non-secret tags.
    pub tags: Vec<String>,
    /// Free-form notes (non-secret by policy).
    pub notes: String,
    /// Reference to the encrypted password in the vault, if stored.
    pub password_ref: Option<SecretRef>,
    /// Second factors attached to this account.
    pub factors: Vec<SecondFactor>,
    /// Recovery code slots attached to this account.
    pub recovery_codes: Vec<RecoveryCode>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last modification timestamp.
    pub updated_at: DateTime<Utc>,
}

/// Input for creating or updating an account's metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInput {
    /// Display name.
    pub display_name: String,
    /// Service kind.
    pub service_kind: ServiceKind,
    /// Username on the service, if any.
    pub username: Option<String>,
    /// Email, if any.
    pub email: Option<String>,
    /// Login URL, if any.
    pub login_url: Option<String>,
    /// Tags.
    pub tags: Vec<String>,
    /// Notes.
    pub notes: String,
}

impl AccountInput {
    /// Validates the input, returning normalized values.
    pub fn validate(&self) -> Result<ValidatedAccountInput, DomainError> {
        let display_name = Name::new(&self.display_name)?;
        let service_label = match &self.service_kind {
            ServiceKind::Custom { label } => {
                let name = Name::new(label)?;
                name.as_str().to_owned()
            }
            _ => self.service_kind.label(),
        };
        let username = self
            .username
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if username.is_some_and(|s| s.chars().any(|c| c.is_control()) || s.len() > 200) {
            return Err(DomainError::ControlCharacters);
        }
        let email = self
            .email
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if email.is_some_and(|s| s.chars().any(|c| c.is_control()) || s.len() > 320) {
            return Err(DomainError::ControlCharacters);
        }
        let login_url = self
            .login_url
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let login_url = match login_url {
            Some(url) => Some(HttpUrl::new(url)?.as_str().to_owned()),
            None => None,
        };
        let tags = validation::normalize_tags(&self.tags)?;
        validation::validate_notes(&self.notes)?;

        Ok(ValidatedAccountInput {
            display_name: display_name.as_str().to_owned(),
            service_kind: match &self.service_kind {
                ServiceKind::Custom { .. } => ServiceKind::Custom {
                    label: service_label,
                },
                other => other.clone(),
            },
            username: username.map(str::to_owned),
            email: email.map(str::to_owned),
            login_url,
            tags,
            notes: self.notes.clone(),
        })
    }
}

/// Normalized, validated account input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedAccountInput {
    /// Validated display name.
    pub display_name: String,
    /// Validated service kind.
    pub service_kind: ServiceKind,
    /// Validated username.
    pub username: Option<String>,
    /// Validated email.
    pub email: Option<String>,
    /// Validated login URL.
    pub login_url: Option<String>,
    /// Normalized tags.
    pub tags: Vec<String>,
    /// Validated notes.
    pub notes: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> AccountInput {
        AccountInput {
            display_name: "  Work GitHub  ".into(),
            service_kind: ServiceKind::GitHub,
            username: Some(" octocat ".into()),
            email: Some("octocat@example.com".into()),
            login_url: Some("https://github.com/login".into()),
            tags: vec![" work ".into(), "work".into()],
            notes: "primary account".into(),
        }
    }

    #[test]
    fn account_input_normalizes() {
        let validated = input().validate().expect("valid");
        assert_eq!(validated.display_name, "Work GitHub");
        assert_eq!(validated.username.as_deref(), Some("octocat"));
        assert_eq!(validated.tags, vec!["work"]);
        assert_eq!(
            validated.login_url.as_deref(),
            Some("https://github.com/login")
        );
    }

    #[test]
    fn account_input_rejects_bad_urls_and_empty_names() {
        let mut bad_url = input();
        bad_url.login_url = Some("ftp://nope".into());
        assert!(bad_url.validate().is_err());

        let mut no_name = input();
        no_name.display_name = "  ".into();
        assert!(no_name.validate().is_err());
    }

    #[test]
    fn custom_services_require_a_label() {
        let mut custom = input();
        custom.service_kind = ServiceKind::Custom {
            label: "   ".into(),
        };
        assert!(custom.validate().is_err());
        custom.service_kind = ServiceKind::Custom {
            label: "Internal Wiki".into(),
        };
        assert!(custom.validate().is_ok());
    }

    #[test]
    fn serialized_account_contains_no_secret_values() {
        let account = Account {
            id: AccountId::new(),
            profile_id: ProfileId::new(),
            display_name: "Work GitHub".into(),
            service_kind: ServiceKind::GitHub,
            username: Some("octocat".into()),
            email: Some("octocat@example.com".into()),
            login_url: Some("https://github.com/login".into()),
            tags: vec!["work".into()],
            notes: String::new(),
            password_ref: Some(SecretRef::for_password(&AccountId::new())),
            factors: Vec::new(),
            recovery_codes: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&account).expect("serializable");
        // Only the structured reference appears, never a password value.
        assert!(json.contains("passwordRef"));
        assert!(!json.contains("plaintext"));
    }
}
