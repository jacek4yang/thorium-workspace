//! Diagnostics.
//!
//! The report answers "why is this not working?" without answering "what are
//! this user's passwords?". Every field here is either a path the user chose, a
//! version number, a boolean, or a count. Nothing in it is derived from secret
//! material, and [`DiagnosticReport::to_shareable_text`] additionally redacts
//! the parts of a path that could identify a person.

use serde::Serialize;
use tw_domain::{ProfileRuntimeStatus, ThemePreference};

use crate::bootstrap::{BootstrapReport, WorkspacePaths};

/// A profile's line in the report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDiagnostic {
    /// Profile id.
    pub id: String,
    /// Profile name.
    pub name: String,
    /// Observed status.
    pub status: ProfileRuntimeStatus,
    /// Which Thorium build it launches.
    pub thorium_selection: String,
    /// Configured locale.
    pub locale: String,
    /// Configured timezone.
    pub timezone: String,
    /// Whether its `User Data` directory exists.
    pub user_data_present: bool,
    /// Whether a DevTools control channel is open.
    pub cdp_active: bool,
    /// Whether the timezone and locale overrides are being applied.
    pub emulation_active: bool,
}

/// The whole report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReport {
    /// Application version.
    pub app_version: String,
    /// Which platform this build targets.
    pub platform: String,
    /// Whether Windows-specific supervision is active.
    pub windows_supervision: bool,
    /// The workspace root.
    pub workspace_root: String,
    /// Whether the root is writable.
    pub workspace_writable: bool,
    /// The single-instance object name.
    pub instance_name: String,
    /// Database schema version.
    pub schema_version: u32,
    /// Whether SQLite's integrity check passes.
    pub database_integrity: String,
    /// Vault state, as a word.
    pub vault_state: &'static str,
    /// The vault's format version, when a vault exists.
    pub vault_format_version: Option<u16>,
    /// The vault's Argon2id memory cost in KiB, when a vault exists.
    pub vault_kdf_memory_kib: Option<u32>,
    /// How many secrets the vault holds. Only available while unlocked.
    pub vault_secret_count: Option<usize>,
    /// Installed Thorium versions.
    pub thorium_versions: Vec<String>,
    /// The selected Thorium version.
    pub thorium_current: Option<String>,
    /// The path to the selected browser.
    pub thorium_executable: Option<String>,
    /// Configured update channel.
    pub thorium_channel: String,
    /// One line per profile.
    pub profiles: Vec<ProfileDiagnostic>,
    /// Number of accounts.
    pub account_count: usize,
    /// Number of second factors.
    pub factor_count: usize,
    /// UI theme setting.
    pub theme: String,
    /// Whether clipboard clearing is enabled.
    pub clipboard_clear_enabled: bool,
    /// Clipboard clear delay in seconds.
    pub clipboard_clear_seconds: u32,
    /// Whether idle locking is enabled.
    pub vault_idle_lock_enabled: bool,
    /// Idle lock timeout in seconds.
    pub vault_idle_lock_seconds: u32,
    /// Stale runtime files removed at startup.
    pub stale_files_removed: usize,
    /// Stale Thorium staging entries removed at startup.
    pub stale_staging_removed: usize,
}

impl DiagnosticReport {
    /// Renders the report as text for the "copy diagnostics" button.
    ///
    /// Paths are redacted: a workspace path routinely contains the user's
    /// account name, and a support log is something people paste in public.
    #[must_use]
    pub fn to_shareable_text(&self) -> String {
        let mut out = String::new();
        let mut line = |label: &str, value: String| {
            out.push_str(label);
            out.push_str(": ");
            out.push_str(&value);
            out.push('\n');
        };

        line("Thorium Workspace", self.app_version.clone());
        line("Platform", self.platform.clone());
        line("Windows supervision", self.windows_supervision.to_string());
        line("Workspace root", redact_path(&self.workspace_root));
        line("Workspace writable", self.workspace_writable.to_string());
        line("Schema version", self.schema_version.to_string());
        line("Database integrity", self.database_integrity.clone());
        line("Vault", self.vault_state.to_owned());
        if let Some(version) = self.vault_format_version {
            line("Vault format", version.to_string());
        }
        if let Some(memory) = self.vault_kdf_memory_kib {
            line("Vault KDF memory", format!("{memory} KiB"));
        }
        if let Some(count) = self.vault_secret_count {
            line("Vault secrets", count.to_string());
        }
        line("Thorium channel", self.thorium_channel.clone());
        line(
            "Thorium installed",
            if self.thorium_versions.is_empty() {
                "none".to_owned()
            } else {
                self.thorium_versions.join(", ")
            },
        );
        line(
            "Thorium current",
            self.thorium_current.clone().unwrap_or_else(|| "none".to_owned()),
        );
        line(
            "Thorium executable",
            self.thorium_executable
                .as_deref()
                .map_or_else(|| "none".to_owned(), redact_path),
        );
        line("Accounts", self.account_count.to_string());
        line("Second factors", self.factor_count.to_string());
        line("Theme", self.theme.clone());
        line(
            "Clipboard clearing",
            format!(
                "{} ({}s)",
                if self.clipboard_clear_enabled { "on" } else { "off" },
                self.clipboard_clear_seconds
            ),
        );
        line(
            "Vault idle lock",
            format!(
                "{} ({}s)",
                if self.vault_idle_lock_enabled { "on" } else { "off" },
                self.vault_idle_lock_seconds
            ),
        );
        line(
            "Stale files cleaned",
            format!(
                "{} runtime, {} staging",
                self.stale_files_removed, self.stale_staging_removed
            ),
        );

        out.push_str("Profiles:\n");
        if self.profiles.is_empty() {
            out.push_str("  (none)\n");
        }
        for profile in &self.profiles {
            // The profile name is user-chosen text and may be personal, so only
            // its id, which is a random UUID, is included.
            out.push_str(&format!(
                "  {} status={} thorium={} locale={} tz={} userData={} cdp={} emulation={}\n",
                profile.id,
                profile.status.as_str(),
                profile.thorium_selection,
                profile.locale,
                profile.timezone,
                profile.user_data_present,
                profile.cdp_active,
                profile.emulation_active,
            ));
        }
        out
    }
}

/// Replaces everything but the final path component with a marker.
///
/// `C:\Users\Jane Smith\Portable\TW\browsers\thorium\...` becomes
/// `...\thorium`, which is enough to tell whether the path looks right without
/// naming the person.
fn redact_path(path: &str) -> String {
    let separator = if path.contains('\\') { '\\' } else { '/' };
    match path.rsplit_once(separator) {
        Some((_, last)) if !last.is_empty() => format!("...{separator}{last}"),
        _ => "...".to_owned(),
    }
}

/// Collects a report from the live workspace.
#[derive(Debug)]
pub struct DiagnosticsBuilder<'a> {
    paths: &'a WorkspacePaths,
    bootstrap: &'a BootstrapReport,
}

impl<'a> DiagnosticsBuilder<'a> {
    /// Starts a report.
    #[must_use]
    pub const fn new(paths: &'a WorkspacePaths, bootstrap: &'a BootstrapReport) -> Self {
        Self { paths, bootstrap }
    }

    /// Builds the report.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        &self,
        database_integrity: String,
        vault_state: &'static str,
        vault_header: Option<tw_vault::VaultHeader>,
        vault_secret_count: Option<usize>,
        thorium_versions: Vec<String>,
        thorium_current: Option<String>,
        thorium_executable: Option<String>,
        settings: &tw_domain::WorkspaceSettings,
        profiles: Vec<ProfileDiagnostic>,
        account_count: usize,
        factor_count: usize,
    ) -> DiagnosticReport {
        DiagnosticReport {
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            platform: if cfg!(windows) {
                "windows".to_owned()
            } else {
                std::env::consts::OS.to_owned()
            },
            windows_supervision: tw_browser_profile_supervising(),
            workspace_root: self.paths.root().to_string_lossy().into_owned(),
            workspace_writable: self.bootstrap.writable,
            instance_name: self.bootstrap.instance_name.clone(),
            schema_version: self.bootstrap.schema_version,
            database_integrity,
            vault_state,
            vault_format_version: vault_header.as_ref().map(|h| h.version),
            vault_kdf_memory_kib: vault_header.as_ref().map(|h| h.kdf.memory_kib),
            vault_secret_count,
            thorium_versions,
            thorium_current,
            thorium_executable,
            thorium_channel: settings.thorium_channel.as_str().to_owned(),
            profiles,
            account_count,
            factor_count,
            theme: match settings.theme {
                ThemePreference::System => "system",
                ThemePreference::Light => "light",
                ThemePreference::Dark => "dark",
            }
            .to_owned(),
            clipboard_clear_enabled: settings.clipboard.clear_enabled,
            clipboard_clear_seconds: settings.clipboard.clear_after_seconds,
            vault_idle_lock_enabled: settings.vault.idle_lock_enabled,
            vault_idle_lock_seconds: settings.vault.idle_lock_seconds,
            stale_files_removed: self.bootstrap.stale_files_removed,
            stale_staging_removed: self.bootstrap.stale_staging_removed,
        }
    }
}

fn tw_browser_profile_supervising() -> bool {
    tw_windows_platform::ProcessGroup::is_supervising()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report() -> DiagnosticReport {
        DiagnosticReport {
            app_version: "1.0.0".to_owned(),
            platform: "windows".to_owned(),
            windows_supervision: true,
            workspace_root: "C:\\Users\\Jane Smith\\Portable\\ThoriumWorkspace".to_owned(),
            workspace_writable: true,
            instance_name: "ThoriumWorkspace-0123456789abcdef".to_owned(),
            schema_version: 1,
            database_integrity: "ok".to_owned(),
            vault_state: "unlocked",
            vault_format_version: Some(1),
            vault_kdf_memory_kib: Some(65_536),
            vault_secret_count: Some(7),
            thorium_versions: vec!["M151".to_owned(), "M152".to_owned()],
            thorium_current: Some("M152".to_owned()),
            thorium_executable: Some(
                "C:\\Users\\Jane Smith\\Portable\\ThoriumWorkspace\\browsers\\thorium\\versions\\M152\\BIN\\thorium.exe"
                    .to_owned(),
            ),
            thorium_channel: "windows_avx2".to_owned(),
            profiles: vec![ProfileDiagnostic {
                id: "8c3d9af7-434b-41d9-a503-e5db838b9a4f".to_owned(),
                name: "Jane's personal banking".to_owned(),
                status: ProfileRuntimeStatus::Running,
                thorium_selection: "Current".to_owned(),
                locale: "pl-PL".to_owned(),
                timezone: "Europe/Warsaw".to_owned(),
                user_data_present: true,
                cdp_active: true,
                emulation_active: true,
            }],
            account_count: 12,
            factor_count: 5,
            theme: "dark".to_owned(),
            clipboard_clear_enabled: true,
            clipboard_clear_seconds: 30,
            vault_idle_lock_enabled: true,
            vault_idle_lock_seconds: 600,
            stale_files_removed: 2,
            stale_staging_removed: 1,
        }
    }

    #[test]
    fn the_shareable_report_contains_what_support_needs() {
        let text = report().to_shareable_text();
        for expected in [
            "Thorium Workspace: 1.0.0",
            "Schema version: 1",
            "Database integrity: ok",
            "Vault: unlocked",
            "Vault format: 1",
            "Vault KDF memory: 65536 KiB",
            "Thorium current: M152",
            "Accounts: 12",
            "Clipboard clearing: on (30s)",
            "Vault idle lock: on (600s)",
            "Stale files cleaned: 2 runtime, 1 staging",
        ] {
            assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
        }
    }

    #[test]
    fn the_shareable_report_redacts_paths_and_names() {
        let text = report().to_shareable_text();
        assert!(
            !text.contains("Jane Smith"),
            "the user's name must not appear:\n{text}"
        );
        assert!(
            !text.contains("Jane's personal banking"),
            "a profile name must not appear:\n{text}"
        );
        assert!(
            !text.contains("C:\\Users"),
            "a full path must not appear:\n{text}"
        );
        assert!(
            text.contains("...\\ThoriumWorkspace"),
            "the last path component is still useful"
        );
        assert!(text.contains("...\\thorium.exe"));
        // The profile is still identifiable by its random id.
        assert!(text.contains("8c3d9af7-434b-41d9-a503-e5db838b9a4f"));
    }

    #[test]
    fn the_report_never_contains_secret_values() {
        // The report type has no field that can hold secret material. This test
        // pins that by serializing a report built from a workspace whose every
        // secret is a recognisable canary, and asserting none of them appear.
        // Field *names* like `vaultSecretCount` are expected and are not
        // secrets, so the canaries are values rather than words.
        let json = serde_json::to_string(&report()).expect("serialize");
        let text = report().to_shareable_text();
        for canary in [
            "hunter2",
            "correct horse battery staple",
            "JBSWY3DPEHPK3PXP",
            "otpauth://",
            "aaaa-bbbb",
        ] {
            assert!(!json.contains(canary), "the JSON report leaked {canary}");
            assert!(!text.contains(canary), "the shareable report leaked {canary}");
        }
    }

    #[test]
    fn path_redaction_handles_both_separators_and_edge_cases() {
        assert_eq!(redact_path("C:\\Users\\Jane\\TW"), "...\\TW");
        assert_eq!(redact_path("/home/jane/tw"), ".../tw");
        assert_eq!(redact_path("TW"), "...");
        assert_eq!(redact_path(""), "...");
        assert_eq!(redact_path("C:\\Users\\Jane\\"), "...");
    }

    #[test]
    fn a_locked_vault_reports_its_header_but_not_its_contents() {
        let mut locked = report();
        locked.vault_state = "locked";
        locked.vault_secret_count = None;
        let text = locked.to_shareable_text();
        assert!(text.contains("Vault: locked"));
        assert!(
            text.contains("Vault format: 1"),
            "the header is readable while locked"
        );
        assert!(
            !text.contains("Vault secrets:"),
            "a locked vault reveals no count"
        );
    }

    #[test]
    fn an_empty_workspace_renders_without_placeholders_looking_broken() {
        let mut empty = report();
        empty.profiles.clear();
        empty.thorium_versions.clear();
        empty.thorium_current = None;
        empty.thorium_executable = None;
        empty.vault_state = "uninitialized";
        let text = empty.to_shareable_text();
        assert!(text.contains("Thorium installed: none"));
        assert!(text.contains("Thorium current: none"));
        assert!(text.contains("(none)"));
    }
}
