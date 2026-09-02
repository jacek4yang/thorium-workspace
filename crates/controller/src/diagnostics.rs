//! Typed diagnostics snapshot.
//!
//! Every field is safe by construction: paths, versions, and states only.
//! No field can carry a secret value, and the redaction test pins that.

use crate::error::ControllerError;
use crate::workspace::Workspace;

/// Safe diagnostics for the GUI diagnostics page and copied reports.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsSnapshot {
    /// Workspace root path.
    pub workspace_path: String,
    /// Whether the workspace root passed the write probe.
    pub workspace_writable: bool,
    /// Applied storage schema version.
    pub schema_version: i64,
    /// Whether a vault file exists.
    pub vault_exists: bool,
    /// Vault lock state ("missing" | "locked" | "unlocked").
    pub vault_lock_state: &'static str,
    /// Installed Thorium versions.
    pub installed_thorium_versions: Vec<String>,
    /// Current Thorium version, if selected.
    pub current_thorium_version: Option<String>,
    /// Profile ids with live sessions.
    pub running_profiles: Vec<String>,
    /// Configured idle auto-lock minutes, if enabled.
    pub idle_lock_minutes: Option<u32>,
    /// Seconds before the clipboard clear fires.
    pub clipboard_clear_seconds: u32,
}

impl Workspace {
    /// Builds the current diagnostics snapshot.
    pub fn diagnostics(&self) -> Result<DiagnosticsSnapshot, ControllerError> {
        let writable =
            thorium_workspace_windows_platform::paths::verify_writable(self.root()).is_ok();
        let schema_version = self.store().schema_version()?;
        let (vault_exists, vault_lock_state) = {
            let vault = self.vault();
            (
                vault.exists(),
                match vault.lock_state() {
                    thorium_workspace_vault::VaultLockState::Missing => "missing",
                    thorium_workspace_vault::VaultLockState::Locked => "locked",
                    thorium_workspace_vault::VaultLockState::Unlocked => "unlocked",
                },
            )
        };
        let layout = self.thorium_layout();
        let installed = layout.list_installed()?;
        let current = layout.current_version()?;
        let running = self
            .running_profiles()?
            .into_iter()
            .map(|id| id.to_string())
            .collect();
        let settings = self.settings()?;
        Ok(DiagnosticsSnapshot {
            workspace_path: self.root().to_string_lossy().into_owned(),
            workspace_writable: writable,
            schema_version,
            vault_exists,
            vault_lock_state,
            installed_thorium_versions: installed,
            current_thorium_version: current,
            running_profiles: running,
            idle_lock_minutes: settings.vault_idle_lock_minutes,
            clipboard_clear_seconds: settings.clipboard_clear_seconds,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_has_no_secret_shaped_fields() {
        // Structural redaction proof: the snapshot type has no field that
        // could hold a password, seed, or recovery code. Pin the expected
        // serialized keys so an accidental secret field is caught.
        let json = serde_json::to_string(&DiagnosticsSnapshot {
            workspace_path: "C:\\ws".to_owned(),
            workspace_writable: true,
            schema_version: 1,
            vault_exists: true,
            vault_lock_state: "locked",
            installed_thorium_versions: vec![],
            current_thorium_version: None,
            running_profiles: vec![],
            idle_lock_minutes: Some(10),
            clipboard_clear_seconds: 20,
        })
        .expect("serializable");
        for forbidden in [
            "password",
            "seed",
            "secretRef",
            "recoveryCodeValue",
            "otpauth",
        ] {
            assert!(
                !json.to_lowercase().contains(&forbidden.to_lowercase()),
                "diagnostics must not expose {forbidden}"
            );
        }
        assert!(json.contains("vaultLockState"));
    }
}
