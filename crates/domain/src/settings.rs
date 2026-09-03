//! Thorium installation records and workspace settings.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// A Thorium version installed under `browsers/thorium/versions/<version>/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ThoriumInstall {
    /// Version tag, e.g. `M152.0.7977.55`.
    pub version: String,
    /// Build variant, e.g. `AVX2` (see the thorium crate for the catalog).
    pub variant: String,
    /// When the install completed.
    pub installed_at: DateTime<Utc>,
    /// Path of the install directory relative to the workspace root
    /// (e.g. `browsers/thorium/versions/M152.0.7977.55`).
    pub rel_path: String,
}

/// Theme preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    /// Follow the OS theme.
    #[default]
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

/// Workspace-level settings (non-secret).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSettings {
    /// Seconds before the clipboard is cleared after copying a secret.
    /// Must be between 5 and 120.
    pub clipboard_clear_seconds: u32,
    /// Idle minutes before the vault auto-locks; `None` disables.
    /// Must be between 1 and 240 when set.
    pub vault_idle_lock_minutes: Option<u32>,
    /// Lock the vault when the main window is minimized.
    pub vault_lock_on_minimize: bool,
    /// UI theme preference.
    pub theme: ThemePreference,
    /// Preferred Thorium build variant for new installs (e.g. `AVX2`).
    pub preferred_thorium_variant: String,
    /// Optional proxy endpoint (`scheme://host:port`) used **only** for
    /// workspace downloads (Thorium release discovery and install
    /// archives). It never routes browser profile traffic. Absent/empty
    /// means direct connection. The value may contain credentials for the
    /// proxy itself, so it is treated as sensitive-adjacent: it is never
    /// logged and never echoed into error messages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_proxy: Option<String>,
}

impl Default for WorkspaceSettings {
    fn default() -> Self {
        Self {
            clipboard_clear_seconds: 20,
            vault_idle_lock_minutes: Some(10),
            vault_lock_on_minimize: false,
            theme: ThemePreference::System,
            preferred_thorium_variant: "AVX2".to_owned(),
            download_proxy: None,
        }
    }
}

/// Proxy schemes accepted for download routing.
pub const PROXY_SCHEMES: &[&str] = &["http", "https", "socks5", "socks5h"];

/// Validates a proxy endpoint string: `scheme://host[:port]`, optionally
/// with `user:pass@` before the host. Dependency-free on purpose; the
/// actual URL interpretation happens in the networking layer.
pub fn validate_proxy_url(raw: &str) -> Result<(), DomainError> {
    let url = raw.trim();
    let Some((scheme, rest)) = url.split_once("://") else {
        return Err(DomainError::InvalidProxyUrl);
    };
    if !PROXY_SCHEMES.contains(&scheme) {
        return Err(DomainError::InvalidProxyUrl);
    }
    // Strip optional userinfo, then any path/query/fragment.
    let hostport = rest.rsplit_once('@').map_or(rest, |(_, host)| host);
    let hostport = hostport.split(['/', '?', '#']).next().unwrap_or_default();
    let (host, port) = match hostport.rsplit_once(':') {
        // An IPv6 literal in brackets keeps its colons; the split above would
        // cut inside the brackets, so restore anything bracketed.
        Some((host, port)) => {
            if host.starts_with('[') && !host.contains(']') {
                (hostport, None)
            } else {
                (host, Some(port))
            }
        }
        None => (hostport, None),
    };
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.is_empty() || host.chars().any(char::is_whitespace) {
        return Err(DomainError::InvalidProxyUrl);
    }
    if let Some(port) = port {
        if port.is_empty() || port.len() > 5 || !port.chars().all(|c| c.is_ascii_digit()) {
            return Err(DomainError::InvalidProxyUrl);
        }
    }
    Ok(())
}

impl WorkspaceSettings {
    /// Validates the settings.
    pub fn validate(&self) -> Result<(), DomainError> {
        if !(5..=120).contains(&self.clipboard_clear_seconds) {
            return Err(DomainError::OutOfRange {
                field: "clipboardClearSeconds",
            });
        }
        if let Some(minutes) = self.vault_idle_lock_minutes {
            if !(1..=240).contains(&minutes) {
                return Err(DomainError::OutOfRange {
                    field: "vaultIdleLockMinutes",
                });
            }
        }
        if self.preferred_thorium_variant.trim().is_empty() {
            return Err(DomainError::OutOfRange {
                field: "preferredThoriumVariant",
            });
        }
        if let Some(proxy) = &self.download_proxy {
            validate_proxy_url(proxy)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_are_valid() {
        assert!(WorkspaceSettings::default().validate().is_ok());
    }

    #[test]
    fn settings_ranges_are_enforced() {
        let mut settings = WorkspaceSettings {
            clipboard_clear_seconds: 2,
            ..WorkspaceSettings::default()
        };
        assert!(settings.validate().is_err());
        settings.clipboard_clear_seconds = 300;
        assert!(settings.validate().is_err());

        let mut settings = WorkspaceSettings {
            vault_idle_lock_minutes: Some(0),
            ..WorkspaceSettings::default()
        };
        assert!(settings.validate().is_err());
        settings.vault_idle_lock_minutes = Some(60);
        assert!(settings.validate().is_ok());
        settings.vault_idle_lock_minutes = None;
        assert!(settings.validate().is_ok());
    }

    #[test]
    fn proxy_urls_are_validated() {
        assert!(validate_proxy_url("http://127.0.0.1:10808").is_ok());
        assert!(validate_proxy_url("https://proxy.example.com:8080").is_ok());
        assert!(validate_proxy_url("socks5://10.0.0.2:1080").is_ok());
        assert!(validate_proxy_url("socks5h://10.0.0.2:1080").is_ok());
        assert!(validate_proxy_url("http://user:pass@10.0.0.2:8080").is_ok());
        assert!(validate_proxy_url("  http://127.0.0.1:10808  ").is_ok());

        // Rejected shapes.
        assert!(validate_proxy_url("").is_err());
        assert!(validate_proxy_url("127.0.0.1:10808").is_err()); // no scheme
        assert!(validate_proxy_url("ftp://10.0.0.2:21").is_err()); // bad scheme
        assert!(validate_proxy_url("http://").is_err()); // no host
        assert!(validate_proxy_url("http://host:notaport").is_err());
        assert!(validate_proxy_url("http://ho st:1080").is_err());

        // The settings struct routes through the same validation.
        let mut settings = WorkspaceSettings {
            download_proxy: Some("ftp://bad".to_owned()),
            ..WorkspaceSettings::default()
        };
        assert!(settings.validate().is_err());
        settings.download_proxy = Some("socks5://127.0.0.1:10808".to_owned());
        assert!(settings.validate().is_ok());
        settings.download_proxy = None;
        assert!(settings.validate().is_ok());
    }

    /// Settings persisted before the proxy field existed must load with the
    /// field defaulting to `None`.
    #[test]
    fn legacy_settings_json_without_proxy_deserializes() {
        let legacy = r#"{
            "clipboardClearSeconds": 20,
            "vaultIdleLockMinutes": 10,
            "vaultLockOnMinimize": false,
            "theme": "system",
            "preferredThoriumVariant": "AVX2"
        }"#;
        let settings: WorkspaceSettings = serde_json::from_str(legacy).expect("legacy json");
        assert_eq!(settings.download_proxy, None);
    }
}
