//! Service kinds for accounts, with well-known presets.
//!
//! The account model is generic: GitHub and Microsoft are presets, not
//! hard-coded branches. `Custom` covers any other service.

use serde::{Deserialize, Serialize};

/// Well-known service kinds shipped as presets.
pub const KNOWN_SERVICES: &[(&str, &str)] = &[
    ("github", "GitHub"),
    ("microsoft", "Microsoft"),
    ("google", "Google"),
    ("gitlab", "GitLab"),
    ("custom", "Custom"),
];

/// The kind of service an account belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum ServiceKind {
    /// GitHub (github.com).
    #[serde(rename = "github")]
    GitHub,
    /// Microsoft / Microsoft Entra ID accounts.
    #[serde(rename = "microsoft")]
    Microsoft,
    /// Google (google.com).
    #[serde(rename = "google")]
    Google,
    /// GitLab (gitlab.com).
    #[serde(rename = "gitlab")]
    GitLab,
    /// Any other service, with a free-form label.
    #[serde(rename = "custom")]
    Custom {
        /// Display label for the custom service.
        label: String,
    },
}

impl ServiceKind {
    /// Stable identifier used in storage.
    pub fn id(&self) -> &'static str {
        match self {
            Self::GitHub => "github",
            Self::Microsoft => "microsoft",
            Self::Google => "google",
            Self::GitLab => "gitlab",
            Self::Custom { .. } => "custom",
        }
    }

    /// Human-readable label.
    pub fn label(&self) -> String {
        match self {
            Self::GitHub => "GitHub".to_owned(),
            Self::Microsoft => "Microsoft".to_owned(),
            Self::Google => "Google".to_owned(),
            Self::GitLab => "GitLab".to_owned(),
            Self::Custom { label } => label.clone(),
        }
    }

    /// Suggested login URL for well-known services, if any.
    pub fn suggested_login_url(&self) -> Option<&'static str> {
        match self {
            Self::GitHub => Some("https://github.com/login"),
            Self::Microsoft => Some("https://login.live.com/"),
            Self::Google => Some("https://accounts.google.com/"),
            Self::GitLab => Some("https://gitlab.com/users/sign_in"),
            Self::Custom { .. } => None,
        }
    }

    /// Reconstructs a service kind from its storage identifier and, for
    /// custom services, the stored label.
    pub fn from_id_and_label(id: &str, custom_label: Option<&str>) -> Option<Self> {
        match id {
            "github" => Some(Self::GitHub),
            "microsoft" => Some(Self::Microsoft),
            "google" => Some(Self::Google),
            "gitlab" => Some(Self::GitLab),
            "custom" => Some(Self::Custom {
                label: custom_label.unwrap_or("Custom").to_owned(),
            }),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_kinds_roundtrip_through_ids() {
        for kind in [
            ServiceKind::GitHub,
            ServiceKind::Microsoft,
            ServiceKind::Google,
            ServiceKind::GitLab,
            ServiceKind::Custom {
                label: "Internal Wiki".into(),
            },
        ] {
            let rebuilt =
                ServiceKind::from_id_and_label(kind.id(), Some(&kind.label())).expect("roundtrip");
            assert_eq!(kind, rebuilt);
        }
    }

    #[test]
    fn unknown_service_id_is_rejected() {
        assert!(ServiceKind::from_id_and_label("nope", None).is_none());
    }

    #[test]
    fn presets_serialize_camel_cased() {
        let json = serde_json::to_value(ServiceKind::GitHub).expect("serializable");
        assert_eq!(json, serde_json::json!({ "kind": "github" }));
    }
}
