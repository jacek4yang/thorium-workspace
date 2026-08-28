//! Browser Profiles.
//!
//! A profile owns exactly one Thorium `User Data` directory. That is the
//! isolation boundary the whole product rests on, so the directory name is
//! derived from the profile id rather than from its (mutable, user-chosen) name.

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::{DomainError, DomainResult};
use crate::ids::{AccountId, ProfileId};
use crate::time::Timestamp;
use crate::validation::{
    LocaleTag, MAX_NOTES, MAX_STARTUP_URLS, TimeZoneId, validate_display_name, validate_startup_url,
};

/// Which Thorium build a profile launches.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(tag = "mode", content = "version", rename_all = "snake_case")]
pub enum ThoriumSelection {
    /// Follow whichever version is promoted to `current`.
    #[default]
    Current,
    /// Always launch this exact installed version.
    Pinned(String),
}

impl ThoriumSelection {
    /// The stored discriminant.
    #[must_use]
    pub const fn discriminant(&self) -> &'static str {
        match self {
            Self::Current => "current",
            Self::Pinned(_) => "pinned",
        }
    }

    /// The pinned version, if any.
    #[must_use]
    pub fn pinned_version(&self) -> Option<&str> {
        match self {
            Self::Current => None,
            Self::Pinned(v) => Some(v),
        }
    }

    /// Rebuilds a value from its stored parts.
    ///
    /// # Errors
    ///
    /// Returns [`crate::DiagnosticCode::InvalidInput`] for an unknown
    /// discriminant or a `pinned` selection with no version.
    pub fn from_parts(discriminant: &str, version: Option<&str>) -> DomainResult<Self> {
        match discriminant {
            "current" => Ok(Self::Current),
            "pinned" => version
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(|v| Self::Pinned(v.to_owned()))
                .ok_or_else(|| DomainError::invalid("a pinned Thorium selection requires a version")),
            other => Err(DomainError::invalid(format!(
                "unknown Thorium selection '{other}'"
            ))),
        }
    }
}

impl fmt::Display for ThoriumSelection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Current => f.write_str("Current"),
            Self::Pinned(v) => write!(f, "Pinned {v}"),
        }
    }
}

/// Observed state of a profile's browser session.
///
/// Runtime state is *observed*, never the source of truth: it is reconstructed
/// at startup from the on-disk lock and the live process table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProfileRuntimeStatus {
    /// No browser process is running for this profile.
    #[default]
    Stopped,
    /// The browser is starting; the control channel is not ready yet.
    Starting,
    /// The browser is running.
    Running,
    /// The browser is shutting down.
    Stopping,
    /// The last launch attempt failed.
    Failed,
}

impl ProfileRuntimeStatus {
    /// The stored discriminant.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Stopped => "stopped",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Failed => "failed",
        }
    }

    /// Returns `true` when a process is expected to exist.
    #[must_use]
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Stopping)
    }
}

impl FromStr for ProfileRuntimeStatus {
    type Err = DomainError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "stopped" => Ok(Self::Stopped),
            "starting" => Ok(Self::Starting),
            "running" => Ok(Self::Running),
            "stopping" => Ok(Self::Stopping),
            "failed" => Ok(Self::Failed),
            other => Err(DomainError::invalid(format!("unknown profile status '{other}'"))),
        }
    }
}

/// A persistent browser profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserProfile {
    /// Identity. Also determines the `User Data` directory name.
    pub id: ProfileId,
    /// Name shown in lists.
    pub name: String,
    /// Which Thorium build to launch.
    pub thorium: ThoriumSelection,
    /// Pages opened when the browser starts.
    pub startup_urls: Vec<String>,
    /// Locale applied to the browser.
    pub locale: LocaleTag,
    /// IANA timezone applied to the browser.
    pub timezone: TimeZoneId,
    /// Accounts associated with this profile.
    pub account_ids: Vec<AccountId>,
    /// Free-text notes.
    pub notes: String,
    /// Reserved for a future release that adds network routing.
    ///
    /// v1.0.0 never reads or writes anything but `None`. It exists so the
    /// persisted schema does not need a migration when routing arrives.
    pub network_route_id: Option<String>,
    /// Creation time.
    pub created_at: Timestamp,
    /// Last modification time.
    pub updated_at: Timestamp,
}

impl BrowserProfile {
    /// The directory name this profile's `User Data` lives under.
    ///
    /// Derived from the immutable id so renaming a profile can never move,
    /// merge or orphan browser state.
    #[must_use]
    pub fn user_data_dir_name(&self) -> String {
        Self::user_data_dir_name_for(self.id)
    }

    /// The `User Data` directory name for a given profile id.
    #[must_use]
    pub fn user_data_dir_name_for(id: ProfileId) -> String {
        id.as_uuid().simple().to_string()
    }
}

/// Fields supplied when creating or updating a profile.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserProfileDraft {
    /// Name shown in lists.
    pub name: String,
    /// Which Thorium build to launch.
    pub thorium: ThoriumSelection,
    /// Raw startup URLs; validated on normalization.
    pub startup_urls: Vec<String>,
    /// Raw locale tag.
    pub locale: Option<String>,
    /// Raw IANA timezone identifier.
    pub timezone: Option<String>,
    /// Accounts to associate.
    pub account_ids: Vec<AccountId>,
    /// Free-text notes.
    pub notes: String,
}

impl BrowserProfileDraft {
    /// Validates and normalizes the draft.
    ///
    /// # Errors
    ///
    /// Returns [`crate::DiagnosticCode::InvalidInput`] when the name, URLs,
    /// locale, timezone or notes are unacceptable.
    pub fn normalize(&self) -> DomainResult<NormalizedProfile> {
        let name = validate_display_name(&self.name)?;
        if self.startup_urls.len() > MAX_STARTUP_URLS {
            return Err(DomainError::invalid(format!(
                "at most {MAX_STARTUP_URLS} startup URLs are allowed"
            )));
        }
        let mut startup_urls = Vec::new();
        for raw in &self.startup_urls {
            if raw.trim().is_empty() {
                continue;
            }
            let url = validate_startup_url(raw)?;
            if !startup_urls.contains(&url) {
                startup_urls.push(url);
            }
        }
        let locale = match self.locale.as_deref().map(str::trim) {
            None | Some("") => LocaleTag::default_tag(),
            Some(raw) => LocaleTag::parse(raw)?,
        };
        let timezone = match self.timezone.as_deref().map(str::trim) {
            None | Some("") => TimeZoneId::utc(),
            Some(raw) => TimeZoneId::parse(raw)?,
        };
        if self.notes.chars().count() > MAX_NOTES {
            return Err(DomainError::invalid(format!(
                "notes must be at most {MAX_NOTES} characters"
            )));
        }
        let mut account_ids = Vec::new();
        for id in &self.account_ids {
            if !account_ids.contains(id) {
                account_ids.push(*id);
            }
        }
        Ok(NormalizedProfile {
            name,
            thorium: self.thorium.clone(),
            startup_urls,
            locale,
            timezone,
            account_ids,
            notes: self.notes.trim().to_owned(),
        })
    }
}

/// The validated result of normalizing a [`BrowserProfileDraft`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedProfile {
    /// Validated name.
    pub name: String,
    /// Thorium selection.
    pub thorium: ThoriumSelection,
    /// Validated, de-duplicated startup URLs.
    pub startup_urls: Vec<String>,
    /// Canonical locale.
    pub locale: LocaleTag,
    /// Canonical timezone.
    pub timezone: TimeZoneId,
    /// De-duplicated account ids.
    pub account_ids: Vec<AccountId>,
    /// Trimmed notes.
    pub notes: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_data_directory_name_follows_the_id_not_the_name() {
        let id = ProfileId::new();
        let profile = BrowserProfile {
            id,
            name: "First name".to_owned(),
            thorium: ThoriumSelection::Current,
            startup_urls: Vec::new(),
            locale: LocaleTag::default_tag(),
            timezone: TimeZoneId::utc(),
            account_ids: Vec::new(),
            notes: String::new(),
            network_route_id: None,
            created_at: Timestamp::now(),
            updated_at: Timestamp::now(),
        };
        let before = profile.user_data_dir_name();
        let renamed = BrowserProfile {
            name: "Second name".to_owned(),
            ..profile
        };
        assert_eq!(before, renamed.user_data_dir_name());
        assert_eq!(
            before.len(),
            32,
            "a simple UUID is a safe directory name on Windows"
        );
        assert!(before.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn distinct_profiles_get_distinct_user_data_directories() {
        assert_ne!(
            BrowserProfile::user_data_dir_name_for(ProfileId::new()),
            BrowserProfile::user_data_dir_name_for(ProfileId::new())
        );
    }

    #[test]
    fn drafts_normalize_and_deduplicate() {
        let draft = BrowserProfileDraft {
            name: " Work ".to_owned(),
            thorium: ThoriumSelection::Pinned("M152.0.7977.55".to_owned()),
            startup_urls: vec![
                "https://example.com".to_owned(),
                "https://example.com/".to_owned(),
                "  ".to_owned(),
            ],
            locale: Some("pl-pl".to_owned()),
            timezone: Some("Europe/Warsaw".to_owned()),
            account_ids: Vec::new(),
            notes: " notes ".to_owned(),
        };
        let n = draft.normalize().expect("valid");
        assert_eq!(n.name, "Work");
        assert_eq!(n.startup_urls, vec!["https://example.com/".to_owned()]);
        assert_eq!(n.locale.as_str(), "pl-PL");
        assert_eq!(n.timezone.as_str(), "Europe/Warsaw");
        assert_eq!(n.notes, "notes");
    }

    #[test]
    fn drafts_default_locale_and_timezone() {
        let draft = BrowserProfileDraft {
            name: "P".to_owned(),
            ..Default::default()
        };
        let n = draft.normalize().expect("valid");
        assert_eq!(n.locale.as_str(), "en-US");
        assert_eq!(n.timezone.as_str(), "UTC");
        assert_eq!(n.thorium, ThoriumSelection::Current);
    }

    #[test]
    fn thorium_selection_round_trips_through_storage_parts() {
        let current = ThoriumSelection::Current;
        assert_eq!(
            ThoriumSelection::from_parts(current.discriminant(), current.pinned_version()).expect("ok"),
            current
        );
        let pinned = ThoriumSelection::Pinned("M1".to_owned());
        assert_eq!(
            ThoriumSelection::from_parts(pinned.discriminant(), pinned.pinned_version()).expect("ok"),
            pinned
        );
        assert!(ThoriumSelection::from_parts("pinned", None).is_err());
        assert!(ThoriumSelection::from_parts("pinned", Some("  ")).is_err());
    }

    #[test]
    fn runtime_status_round_trips() {
        for status in [
            ProfileRuntimeStatus::Stopped,
            ProfileRuntimeStatus::Starting,
            ProfileRuntimeStatus::Running,
            ProfileRuntimeStatus::Stopping,
            ProfileRuntimeStatus::Failed,
        ] {
            assert_eq!(
                status.as_str().parse::<ProfileRuntimeStatus>().expect("parse"),
                status
            );
        }
        assert!(ProfileRuntimeStatus::Running.is_active());
        assert!(!ProfileRuntimeStatus::Stopped.is_active());
        assert!(!ProfileRuntimeStatus::Failed.is_active());
    }

    #[test]
    fn startup_url_count_is_bounded() {
        let draft = BrowserProfileDraft {
            name: "P".to_owned(),
            startup_urls: (0..=MAX_STARTUP_URLS)
                .map(|i| format!("https://e{i}.test"))
                .collect(),
            ..Default::default()
        };
        assert!(draft.normalize().is_err());
    }
}
