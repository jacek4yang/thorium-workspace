//! Thorium install management services.
//!
//! Composes `thorium::releases` (discovery + bounded download),
//! `thorium::InstallLayout` (staging/promote/current), and storage
//! (install registry). Download traffic may route through the
//! user-configured proxy in [`WorkspaceSettings::download_proxy`]; that
//! proxy never applies to browser profile launches.

use std::collections::HashMap;
use std::time::Duration;

use chrono::Utc;
use serde::Serialize;
use thorium_workspace_domain::{ThoriumInstall, WorkspaceSettings};
use thorium_workspace_thorium::Variant;
use thorium_workspace_thorium::releases::Client;

use crate::error::ControllerError;
use crate::workspace::Workspace;

/// Result of a proxy connectivity probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyTestResult {
    /// Public IP observed at the probe endpoint through the candidate
    /// routing (the proxy when one was given, direct otherwise).
    pub exit_ip: String,
}

/// One installed Thorium version surfaced to the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThoriumVersionInfo {
    /// Version tag.
    pub version: String,
    /// Installed variant, when known from the registry.
    pub variant: Option<String>,
    /// Install timestamp (RFC 3339), when known.
    pub installed_at: Option<String>,
    /// Whether this is the current version.
    pub is_current: bool,
}

/// One installable upstream release asset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseOption {
    /// Source repository (`owner/repo`).
    pub repo: String,
    /// Upstream release tag.
    pub tag: String,
    /// Browser version parsed from the asset.
    pub version: String,
    /// Variant identifier (e.g. `AVX2`).
    pub variant: String,
    /// Download URL.
    pub url: String,
    /// Asset size in bytes.
    pub size_bytes: u64,
}

/// Download budget: 2 GiB and 30 minutes. Portable Thorium archives are
/// ~350 MB today; the budget only guards against runaway downloads.
const DOWNLOAD_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const DOWNLOAD_MAX_DURATION: Duration = Duration::from_secs(30 * 60);

impl Workspace {
    /// Builds the release/download client for the configured settings:
    /// routed through `download_proxy` when set, direct otherwise.
    pub fn release_client(settings: &WorkspaceSettings) -> Result<Client, ControllerError> {
        match settings.download_proxy.as_deref() {
            Some(proxy) if !proxy.trim().is_empty() => {
                thorium_workspace_domain::validate_proxy_url(proxy)?;
                Client::new_with_proxy(proxy.trim()).map_err(ControllerError::Thorium)
            }
            _ => Ok(Client::new()?),
        }
    }

    /// Probes connectivity for the candidate proxy setting: fetches the
    /// public exit IP from ip.sb through `proxy` when set, or directly
    /// when `None`. Used by the Settings page "Test" action so the user
    /// can verify the endpoint before saving.
    pub async fn test_download_proxy(
        proxy: Option<String>,
    ) -> Result<ProxyTestResult, ControllerError> {
        let candidate = WorkspaceSettings {
            download_proxy: proxy,
            ..WorkspaceSettings::default()
        };
        // Validates the URL shape with a stable diagnostic code before any
        // network attempt.
        let client = Self::release_client(&candidate)?;
        let exit_ip = client.fetch_exit_ip().await?;
        Ok(ProxyTestResult { exit_ip })
    }

    /// Installed versions (filesystem truth merged with the registry).
    pub fn installed_thorium_versions(&self) -> Result<Vec<ThoriumVersionInfo>, ControllerError> {
        let layout = self.thorium_layout();
        let installed = layout.list_installed()?;
        let current = layout.current_version()?;
        let records: HashMap<String, ThoriumInstall> = self
            .store()
            .list_thorium_installs()?
            .into_iter()
            .map(|record| (record.version.clone(), record))
            .collect();
        Ok(installed
            .into_iter()
            .map(|version| {
                let record = records.get(&version);
                ThoriumVersionInfo {
                    version: version.clone(),
                    variant: record.map(|record| record.variant.clone()),
                    installed_at: record.map(|record| record.installed_at.to_rfc3339()),
                    is_current: current.as_deref() == Some(version.as_str()),
                }
            })
            .collect())
    }

    /// Discovers installable upstream Windows portable releases.
    pub async fn discover_thorium_releases(&self) -> Result<Vec<ReleaseOption>, ControllerError> {
        let settings = self.settings()?;
        let client = Self::release_client(&settings)?;
        let per_source = client.discover_windows_releases(10).await?;
        let mut options = Vec::new();
        for (repo, releases) in per_source {
            for release in releases {
                for (variant, url, size) in release.assets {
                    options.push(ReleaseOption {
                        repo: repo.clone(),
                        tag: release.tag.clone(),
                        version: release.version.clone(),
                        variant: variant.id().to_owned(),
                        url,
                        size_bytes: size,
                    });
                }
            }
        }
        Ok(options)
    }

    /// Downloads, validates, and installs a release asset; records it and
    /// selects it as current when no current version exists. `progress`
    /// receives `(downloaded, total)` during the download.
    pub async fn install_thorium(
        &self,
        url: &str,
        version: &str,
        variant_id: &str,
        progress: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> Result<(), ControllerError> {
        let variant = Variant::from_id(variant_id).ok_or_else(|| {
            thorium_workspace_thorium::ThoriumError::InvalidArchive {
                detail: format!("unknown variant {variant_id}"),
            }
        })?;
        let layout = self.thorium_layout();
        layout.initialize()?;
        let staging = self.root().join("browsers/thorium/staging");
        let settings = self.settings()?;
        let client = Self::release_client(&settings)?;
        let file_name = format!("Thorium_{variant_id}_{version}.zip");
        let archive = client
            .download_bounded(
                url,
                &staging,
                &file_name,
                DOWNLOAD_MAX_BYTES,
                DOWNLOAD_MAX_DURATION,
                progress,
            )
            .await?;

        // Extraction and promotion are blocking filesystem work.
        let blocking_layout = self.thorium_layout();
        let blocking_version = version.to_owned();
        let blocking_archive = archive.clone();
        let promoted = tokio::task::spawn_blocking(move || {
            blocking_layout.install_from_archive(&blocking_archive, &blocking_version, variant)
        })
        .await
        .map_err(|error| {
            thorium_workspace_thorium::ThoriumError::Discovery(format!(
                "install task failed: {error}"
            ))
        })??;

        self.store().add_thorium_install(&ThoriumInstall {
            version: version.to_owned(),
            variant: variant_id.to_owned(),
            rel_path: promoted
                .strip_prefix(self.root())
                .map(|relative| relative.to_string_lossy().into_owned())
                .unwrap_or_else(|_| promoted.to_string_lossy().into_owned()),
            installed_at: Utc::now(),
        })?;
        if layout.current_version()?.is_none() {
            layout.set_current(version)?;
        }
        // The staged archive is no longer needed.
        let _ = std::fs::remove_file(&archive);
        Ok(())
    }

    /// Atomically selects the current version.
    pub fn set_current_thorium(&self, version: &str) -> Result<(), ControllerError> {
        self.thorium_layout().set_current(version)?;
        Ok(())
    }

    /// Deletes an installed version unless it is current or used by a
    /// running profile.
    pub fn delete_thorium_version(&self, version: &str) -> Result<(), ControllerError> {
        let layout = self.thorium_layout();
        let mut protected = Vec::new();
        if let Some(current) = layout.current_version()? {
            protected.push(current);
        }
        for profile_id in self.running_profiles()? {
            if let Ok(plan) = self.plan_launch(profile_id) {
                protected.push(plan.version);
            }
        }
        layout.delete_version(version, &protected)?;
        // Drop matching registry rows (all variants of the version).
        for record in self.store().list_thorium_installs()? {
            if record.version == version {
                let _ = self
                    .store()
                    .remove_thorium_install(&record.version, &record.variant)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::workspace::Workspace;
    use thorium_workspace_domain::{DiagnosticCode as _, WorkspaceSettings};

    #[test]
    fn release_client_rejects_invalid_proxy_settings() {
        let settings = WorkspaceSettings {
            download_proxy: Some("definitely not a proxy".to_owned()),
            ..WorkspaceSettings::default()
        };
        let error = Workspace::release_client(&settings).expect_err("rejected");
        assert_eq!(error.diagnostic_code(), "DOMAIN_INVALID_PROXY_URL");
    }

    #[test]
    fn release_client_accepts_direct_and_valid_proxy() {
        let direct = WorkspaceSettings::default();
        assert!(Workspace::release_client(&direct).is_ok());
        let proxied = WorkspaceSettings {
            download_proxy: Some("socks5://127.0.0.1:10808".to_owned()),
            ..WorkspaceSettings::default()
        };
        assert!(Workspace::release_client(&proxied).is_ok());
    }

    #[test]
    fn installed_versions_merge_layout_and_registry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ws = Workspace::bootstrap(Some(dir.path())).expect("bootstrap");
        assert!(ws.installed_thorium_versions().expect("list").is_empty());

        // Fake an install and register it.
        let layout = ws.thorium_layout();
        layout.initialize().expect("init");
        let version_dir = dir.path().join("browsers/thorium/versions/152.0.7977.55");
        std::fs::create_dir_all(version_dir.join("BIN")).expect("dirs");
        std::fs::write(version_dir.join("BIN/thorium.exe"), b"stub").expect("exe");
        ws.store()
            .add_thorium_install(&thorium_workspace_domain::ThoriumInstall {
                version: "152.0.7977.55".to_owned(),
                variant: "AVX2".to_owned(),
                rel_path: "browsers/thorium/versions/152.0.7977.55".to_owned(),
                installed_at: chrono::Utc::now(),
            })
            .expect("record");

        let versions = ws.installed_thorium_versions().expect("list");
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].version, "152.0.7977.55");
        assert_eq!(versions[0].variant.as_deref(), Some("AVX2"));
        assert!(!versions[0].is_current);

        ws.set_current_thorium("152.0.7977.55").expect("current");
        assert!(ws.installed_thorium_versions().expect("list")[0].is_current);
    }

    #[tokio::test]
    async fn delete_is_blocked_for_current_version() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ws = Workspace::bootstrap(Some(dir.path())).expect("bootstrap");
        let layout = ws.thorium_layout();
        layout.initialize().expect("init");
        for version in ["151.0.7922.72", "152.0.7977.55"] {
            let version_dir = dir
                .path()
                .join(format!("browsers/thorium/versions/{version}"));
            std::fs::create_dir_all(version_dir.join("BIN")).expect("dirs");
            std::fs::write(version_dir.join("BIN/thorium.exe"), b"stub").expect("exe");
        }
        ws.set_current_thorium("152.0.7977.55").expect("current");

        // Current is protected.
        let error = ws
            .delete_thorium_version("152.0.7977.55")
            .expect_err("protected");
        assert_eq!(error.diagnostic_code(), "THORIUM_DELETE_PROTECTED");

        // An unused older version deletes cleanly.
        ws.delete_thorium_version("151.0.7922.72").expect("delete");
        assert_eq!(ws.installed_thorium_versions().expect("list").len(), 1);
    }
}
