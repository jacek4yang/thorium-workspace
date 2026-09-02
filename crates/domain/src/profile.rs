//! Browser Profile model.
//!
//! Invariants (see CLAUDE.md):
//! - One Browser Profile owns one distinct Thorium `User Data` directory.
//! - Browser binaries (Thorium installs) and profile data are separate.
//! - The user data path is stored relative to the workspace root and is
//!   derived from the profile id, guaranteeing uniqueness.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::ids::{AccountId, ProfileId};
use crate::validation::{HttpUrl, IanaTimeZone, LocaleTag, Name};

/// Which Thorium version a profile launches with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "selection")]
pub enum ThoriumSelection {
    /// Follow the workspace-wide `current` install.
    Current,
    /// Stay on a specific installed version (e.g. `M152.0.7977.55`).
    Pinned {
        /// The pinned version tag.
        version: String,
    },
}

impl ThoriumSelection {
    /// Stable storage representation.
    pub fn storage_parts(&self) -> (&'static str, Option<&str>) {
        match self {
            Self::Current => ("current", None),
            Self::Pinned { version } => ("pinned", Some(version.as_str())),
        }
    }

    /// Reconstructs from storage parts.
    pub fn from_storage_parts(kind: &str, version: Option<&str>) -> Option<Self> {
        match (kind, version) {
            ("current", _) => Some(Self::Current),
            ("pinned", Some(version)) => Some(Self::Pinned {
                version: version.to_owned(),
            }),
            _ => None,
        }
    }
}

/// A Browser Profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserProfile {
    /// Unique identifier.
    pub id: ProfileId,
    /// Display name (unique within the workspace).
    pub name: String,
    /// Which Thorium version to launch.
    pub thorium_version: ThoriumSelection,
    /// Per-profile user data directory, relative to the workspace root
    /// (e.g. `profiles/<id>/User Data`).
    pub user_data_rel_path: String,
    /// URLs opened at launch.
    pub startup_urls: Vec<String>,
    /// Optional locale override (BCP-47).
    pub locale: Option<String>,
    /// Optional IANA timezone override.
    pub timezone: Option<String>,
    /// Accounts assigned to this profile.
    pub account_ids: Vec<AccountId>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last modification timestamp.
    pub updated_at: DateTime<Utc>,
    /// Last launch timestamp, if ever launched.
    pub last_launched_at: Option<DateTime<Utc>>,
}

/// Input for creating or updating a Browser Profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileInput {
    /// Display name.
    pub name: String,
    /// Thorium version selection.
    pub thorium_version: ThoriumSelection,
    /// Startup URLs.
    pub startup_urls: Vec<String>,
    /// Optional locale override.
    pub locale: Option<String>,
    /// Optional timezone override.
    pub timezone: Option<String>,
}

/// Maximum number of startup URLs per profile.
pub const MAX_STARTUP_URLS: usize = 10;

impl ProfileInput {
    /// Validates the input and returns normalized values.
    pub fn validate(&self) -> Result<ValidatedProfileInput, DomainError> {
        let name = Name::new(&self.name)?;
        if let ThoriumSelection::Pinned { version } = &self.thorium_version {
            if version.trim().is_empty() || version.len() > 64 {
                return Err(DomainError::OutOfRange {
                    field: "thoriumVersion",
                });
            }
        }
        if self.startup_urls.len() > MAX_STARTUP_URLS {
            return Err(DomainError::OutOfRange {
                field: "startupUrls",
            });
        }
        let mut startup_urls = Vec::with_capacity(self.startup_urls.len());
        for url in &self.startup_urls {
            let trimmed = url.trim();
            if trimmed.is_empty() {
                continue;
            }
            startup_urls.push(HttpUrl::new(trimmed)?.as_str().to_owned());
        }
        let locale = match self.locale.as_deref() {
            Some(value) => Some(LocaleTag::new(value)?.as_str().to_owned()),
            None => None,
        };
        let timezone = match self.timezone.as_deref() {
            Some(value) => Some(IanaTimeZone::new(value)?.as_str().to_owned()),
            None => None,
        };
        Ok(ValidatedProfileInput {
            name: name.as_str().to_owned(),
            thorium_version: self.thorium_version.clone(),
            startup_urls,
            locale,
            timezone,
        })
    }
}

/// Normalized, validated profile input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedProfileInput {
    /// Validated name.
    pub name: String,
    /// Version selection.
    pub thorium_version: ThoriumSelection,
    /// Validated startup URLs.
    pub startup_urls: Vec<String>,
    /// Validated locale.
    pub locale: Option<String>,
    /// Validated timezone.
    pub timezone: Option<String>,
}

impl BrowserProfile {
    /// Computes the unique user data directory for a profile id, relative
    /// to the workspace root.
    pub fn user_data_rel_path_for(id: ProfileId) -> String {
        format!("profiles/{}/User Data", id.as_uuid())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> ProfileInput {
        ProfileInput {
            name: "  Test Profile A  ".into(),
            thorium_version: ThoriumSelection::Current,
            startup_urls: vec![" https://github.com ".into(), "".into()],
            locale: Some(" en-US ".into()),
            timezone: Some(" America/Los_Angeles ".into()),
        }
    }

    #[test]
    fn profile_input_normalizes() {
        let validated = input().validate().expect("valid");
        assert_eq!(validated.name, "Test Profile A");
        assert_eq!(validated.startup_urls, vec!["https://github.com"]);
        assert_eq!(validated.locale.as_deref(), Some("en-US"));
        assert_eq!(validated.timezone.as_deref(), Some("America/Los_Angeles"));
    }

    #[test]
    fn profile_input_rejects_bad_fields() {
        let mut bad = input();
        bad.timezone = Some("C:\\Users".into());
        assert!(bad.validate().is_err());

        let mut bad = input();
        bad.locale = Some("not a locale".into());
        assert!(bad.validate().is_err());

        let mut bad = input();
        bad.startup_urls = (0..MAX_STARTUP_URLS + 1)
            .map(|i| format!("https://example.com/{i}"))
            .collect();
        assert!(bad.validate().is_err());

        let mut bad = input();
        bad.thorium_version = ThoriumSelection::Pinned {
            version: "  ".into(),
        };
        assert!(bad.validate().is_err());
    }

    #[test]
    fn user_data_paths_are_unique_per_profile() {
        let a = BrowserProfile::user_data_rel_path_for(ProfileId::new());
        let b = BrowserProfile::user_data_rel_path_for(ProfileId::new());
        assert_ne!(a, b);
        assert!(a.starts_with("profiles/"));
        assert!(a.ends_with("/User Data"));
    }

    #[test]
    fn thorium_selection_roundtrips() {
        for selection in [
            ThoriumSelection::Current,
            ThoriumSelection::Pinned {
                version: "M152.0.7977.55".into(),
            },
        ] {
            let (kind, version) = selection.storage_parts();
            let rebuilt = ThoriumSelection::from_storage_parts(kind, version).expect("roundtrip");
            assert_eq!(selection, rebuilt);
        }
    }
}
