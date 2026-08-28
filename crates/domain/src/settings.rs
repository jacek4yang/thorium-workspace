//! Workspace-wide settings.

use serde::{Deserialize, Serialize};

use crate::error::{DomainError, DomainResult};

/// How the UI picks its colour scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    /// Follow the Windows app colour mode.
    #[default]
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

impl ThemePreference {
    /// The stored discriminant.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }

    /// Parses a stored discriminant.
    ///
    /// # Errors
    ///
    /// Returns [`crate::DiagnosticCode::InvalidInput`] for an unknown value.
    pub fn parse(value: &str) -> DomainResult<Self> {
        match value {
            "system" => Ok(Self::System),
            "light" => Ok(Self::Light),
            "dark" => Ok(Self::Dark),
            other => Err(DomainError::invalid(format!("unknown theme '{other}'"))),
        }
    }
}

/// Shortest clipboard clear delay the UI allows, in seconds.
pub const MIN_CLIPBOARD_CLEAR_SECONDS: u32 = 5;
/// Longest clipboard clear delay the UI allows, in seconds.
pub const MAX_CLIPBOARD_CLEAR_SECONDS: u32 = 300;
/// Shortest vault idle timeout, in seconds.
pub const MIN_IDLE_LOCK_SECONDS: u32 = 30;
/// Longest vault idle timeout, in seconds.
pub const MAX_IDLE_LOCK_SECONDS: u32 = 24 * 60 * 60;

/// Clipboard protection settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClipboardSettings {
    /// Whether copied secrets are cleared automatically.
    pub clear_enabled: bool,
    /// How long a copied secret stays on the clipboard.
    pub clear_after_seconds: u32,
}

impl Default for ClipboardSettings {
    fn default() -> Self {
        Self {
            clear_enabled: true,
            clear_after_seconds: 30,
        }
    }
}

impl ClipboardSettings {
    /// Validates the configured delay.
    ///
    /// # Errors
    ///
    /// Returns [`crate::DiagnosticCode::InvalidInput`] when the delay is outside
    /// the supported range.
    pub fn validate(&self) -> DomainResult<()> {
        if !(MIN_CLIPBOARD_CLEAR_SECONDS..=MAX_CLIPBOARD_CLEAR_SECONDS).contains(&self.clear_after_seconds) {
            return Err(DomainError::invalid(format!(
                "clipboard clear delay must be between {MIN_CLIPBOARD_CLEAR_SECONDS} and \
                 {MAX_CLIPBOARD_CLEAR_SECONDS} seconds"
            )));
        }
        Ok(())
    }
}

/// Vault locking settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultSettings {
    /// Whether the vault locks itself after a period of inactivity.
    pub idle_lock_enabled: bool,
    /// Idle period before an automatic lock.
    pub idle_lock_seconds: u32,
    /// Whether minimising the window locks the vault.
    pub lock_on_minimize: bool,
}

impl Default for VaultSettings {
    fn default() -> Self {
        Self {
            idle_lock_enabled: true,
            idle_lock_seconds: 600,
            lock_on_minimize: false,
        }
    }
}

impl VaultSettings {
    /// Validates the configured idle timeout.
    ///
    /// # Errors
    ///
    /// Returns [`crate::DiagnosticCode::InvalidInput`] when the timeout is
    /// outside the supported range.
    pub fn validate(&self) -> DomainResult<()> {
        if !(MIN_IDLE_LOCK_SECONDS..=MAX_IDLE_LOCK_SECONDS).contains(&self.idle_lock_seconds) {
            return Err(DomainError::invalid(format!(
                "vault idle timeout must be between {MIN_IDLE_LOCK_SECONDS} and \
                 {MAX_IDLE_LOCK_SECONDS} seconds"
            )));
        }
        Ok(())
    }
}

/// Every workspace-level setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WorkspaceSettings {
    /// UI colour scheme.
    pub theme: ThemePreference,
    /// Clipboard protection.
    pub clipboard: ClipboardSettings,
    /// Vault locking.
    pub vault: VaultSettings,
    /// Which upstream Thorium channel new installs come from.
    pub thorium_channel: crate::thorium::ThoriumChannel,
    /// Whether the app checks for a newer Thorium version on start.
    ///
    /// Off by default: a portable tool should not make network requests the user
    /// did not ask for.
    pub check_thorium_updates_on_start: bool,
}

impl WorkspaceSettings {
    /// Validates every nested setting.
    ///
    /// # Errors
    ///
    /// Propagates the first nested validation failure.
    pub fn validate(&self) -> DomainResult<()> {
        self.clipboard.validate()?;
        self.vault.validate()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_valid_and_conservative() {
        let s = WorkspaceSettings::default();
        assert!(s.validate().is_ok());
        assert!(s.clipboard.clear_enabled, "clipboard clearing is on by default");
        assert!(s.vault.idle_lock_enabled, "idle lock is on by default");
        assert!(!s.check_thorium_updates_on_start, "no unsolicited network access");
    }

    #[test]
    fn clipboard_delay_bounds_are_enforced() {
        let too_short = ClipboardSettings {
            clear_after_seconds: MIN_CLIPBOARD_CLEAR_SECONDS - 1,
            ..Default::default()
        };
        assert!(too_short.validate().is_err());
        let too_long = ClipboardSettings {
            clear_after_seconds: MAX_CLIPBOARD_CLEAR_SECONDS + 1,
            ..Default::default()
        };
        assert!(too_long.validate().is_err());
        let shortest = ClipboardSettings {
            clear_after_seconds: MIN_CLIPBOARD_CLEAR_SECONDS,
            ..Default::default()
        };
        assert!(shortest.validate().is_ok());
    }

    #[test]
    fn idle_lock_bounds_are_enforced() {
        let too_short = VaultSettings {
            idle_lock_seconds: MIN_IDLE_LOCK_SECONDS - 1,
            ..Default::default()
        };
        assert!(too_short.validate().is_err());
        let too_long = VaultSettings {
            idle_lock_seconds: MAX_IDLE_LOCK_SECONDS + 1,
            ..Default::default()
        };
        assert!(too_long.validate().is_err());
    }

    #[test]
    fn themes_round_trip() {
        for t in [
            ThemePreference::System,
            ThemePreference::Light,
            ThemePreference::Dark,
        ] {
            assert_eq!(ThemePreference::parse(t.as_str()).expect("parse"), t);
        }
        assert!(ThemePreference::parse("solarized").is_err());
    }
}
