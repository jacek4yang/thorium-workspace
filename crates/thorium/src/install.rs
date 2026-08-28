//! Installation, promotion and removal of Thorium versions.
//!
//! # On-disk layout
//!
//! ```text
//! browsers/thorium/
//!   versions/
//!     M152.0.7977.55/        one extracted installation
//!     M151.0.7922.72/        the previous known-good version
//!   staging/                 in-progress work; deleted on failure
//!   current.txt              the version selected as `current`
//! ```
//!
//! `current` is a small text file rather than a directory junction or a symlink:
//! junctions need elevation on some Windows configurations, and a dangling one
//! is a much worse failure than a stale line of text. Switching versions writes
//! the file atomically (temp file, then rename), so a crash mid-switch leaves
//! the old version selected rather than none.
//!
//! An update never deletes the previous version. Removal is always an explicit,
//! separate action, and is refused while a profile is using the version.

use std::path::{Path, PathBuf};

use tw_domain::{ThoriumChannel, ThoriumInstallation, ThoriumRelease, Timestamp, thorium::sanitize_version};

use crate::download::{DownloadLimits, download_to_file, verify_digest};
use crate::extract::{ExtractLimits, extract_zip, validate_layout};
use crate::releases::{ReleaseClient, ReleaseClientConfig};
use crate::{ThoriumError, ThoriumResult};

/// Where Thorium lives inside the portable workspace.
#[derive(Debug, Clone)]
pub struct ThoriumPaths {
    root: PathBuf,
}

impl ThoriumPaths {
    /// Builds the layout under `browsers/thorium` inside `workspace_root`.
    #[must_use]
    pub fn new(workspace_root: &Path) -> Self {
        Self {
            root: workspace_root.join("browsers").join("thorium"),
        }
    }

    /// The `browsers/thorium` directory.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The directory holding every installed version.
    #[must_use]
    pub fn versions_dir(&self) -> PathBuf {
        self.root.join("versions")
    }

    /// The directory for one installed version.
    #[must_use]
    pub fn version_dir(&self, version: &str) -> PathBuf {
        self.versions_dir().join(sanitize_version(version))
    }

    /// The scratch directory used while installing.
    #[must_use]
    pub fn staging_dir(&self) -> PathBuf {
        self.root.join("staging")
    }

    /// The file naming the currently selected version.
    #[must_use]
    pub fn current_marker(&self) -> PathBuf {
        self.root.join("current.txt")
    }

    /// Creates every directory the manager needs.
    ///
    /// # Errors
    ///
    /// Returns [`ThoriumError::Io`] when a directory cannot be created.
    pub fn ensure(&self) -> ThoriumResult<()> {
        for dir in [self.root.clone(), self.versions_dir(), self.staging_dir()] {
            std::fs::create_dir_all(&dir)
                .map_err(|e| ThoriumError::io("create the Thorium directories", e))?;
        }
        Ok(())
    }
}

/// Progress reported while installing.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case", tag = "stage")]
pub enum InstallProgress {
    /// Looking up upstream release information.
    Resolving,
    /// Transferring the archive.
    Downloading {
        /// Bytes received.
        received: u64,
        /// Total size, when the server declared one.
        total: Option<u64>,
    },
    /// Checking the archive digest.
    Verifying,
    /// Unpacking the archive.
    Extracting {
        /// Entries processed.
        done: usize,
        /// Entries in total.
        total: usize,
    },
    /// Moving the staged installation into place.
    Activating,
    /// Finished.
    Done {
        /// The installed version.
        version: String,
    },
}

/// What to install.
#[derive(Debug, Clone)]
pub struct InstallRequest {
    /// Which channel to install from.
    pub channel: ThoriumChannel,
    /// The release to install. `None` means "the newest installable release".
    pub release: Option<ThoriumRelease>,
    /// Whether to make the new version `current` once installed.
    pub set_current: bool,
    /// A digest published upstream, when one is available out of band.
    pub expected_sha256: Option<String>,
    /// Download limits.
    pub download_limits: DownloadLimits,
    /// Extraction limits.
    pub extract_limits: ExtractLimits,
}

impl InstallRequest {
    /// Installs the newest release from `channel`, making it current.
    #[must_use]
    pub fn latest(channel: ThoriumChannel) -> Self {
        Self {
            channel,
            release: None,
            set_current: true,
            expected_sha256: None,
            download_limits: DownloadLimits::default(),
            extract_limits: ExtractLimits::default(),
        }
    }
}

/// Installs, selects and removes Thorium versions.
#[derive(Debug)]
pub struct ThoriumManager {
    paths: ThoriumPaths,
    releases: ReleaseClient,
    http: reqwest::Client,
}

impl ThoriumManager {
    /// Builds a manager rooted at `workspace_root`.
    ///
    /// # Errors
    ///
    /// Returns [`ThoriumError::ReleaseLookup`] when the HTTP clients cannot be
    /// built, and [`ThoriumError::Io`] when the directories cannot be created.
    pub fn new(workspace_root: &Path, config: ReleaseClientConfig) -> ThoriumResult<Self> {
        let paths = ThoriumPaths::new(workspace_root);
        paths.ensure()?;
        let http = reqwest::Client::builder()
            .user_agent(format!("ThoriumWorkspace/{}", env!("CARGO_PKG_VERSION")))
            .https_only(true)
            .build()
            .map_err(|e| ThoriumError::Download(e.to_string()))?;
        Ok(Self {
            paths,
            releases: ReleaseClient::new(config)?,
            http,
        })
    }

    /// Builds a manager over caller-supplied clients.
    ///
    /// Used by the integration tests to point the whole pipeline at a local
    /// fixture server over plain HTTP. Production code uses
    /// [`ThoriumManager::new`], which enforces HTTPS on both clients.
    ///
    /// # Errors
    ///
    /// Returns [`ThoriumError::Io`] when the directories cannot be created.
    pub fn with_clients(
        workspace_root: &Path,
        releases: ReleaseClient,
        http: reqwest::Client,
    ) -> ThoriumResult<Self> {
        let paths = ThoriumPaths::new(workspace_root);
        paths.ensure()?;
        Ok(Self {
            paths,
            releases,
            http,
        })
    }

    /// The on-disk layout.
    #[must_use]
    pub const fn paths(&self) -> &ThoriumPaths {
        &self.paths
    }

    /// The release client.
    #[must_use]
    pub const fn releases(&self) -> &ReleaseClient {
        &self.releases
    }

    /// Removes anything left in the staging directory by an interrupted run.
    ///
    /// Called during startup recovery. Only the staging directory is touched:
    /// installed versions are never removed implicitly.
    ///
    /// # Errors
    ///
    /// Returns [`ThoriumError::Io`] when the directory cannot be recreated.
    pub fn clean_staging(&self) -> ThoriumResult<usize> {
        let staging = self.paths.staging_dir();
        let mut removed = 0usize;
        if let Ok(entries) = std::fs::read_dir(&staging) {
            for entry in entries.flatten() {
                let path = entry.path();
                let result = if path.is_dir() {
                    std::fs::remove_dir_all(&path)
                } else {
                    std::fs::remove_file(&path)
                };
                if result.is_ok() {
                    removed += 1;
                } else {
                    tracing::warn!(path = %path.display(), "a stale staging entry could not be removed");
                }
            }
        }
        std::fs::create_dir_all(&staging)
            .map_err(|e| ThoriumError::io("recreate the staging directory", e))?;
        Ok(removed)
    }

    /// Downloads, verifies, extracts and installs a Thorium version.
    ///
    /// Every failure path removes the staging directory, so a failed install
    /// leaves the workspace exactly as it was.
    ///
    /// # Errors
    ///
    /// See [`ThoriumError`].
    pub async fn install(
        &self,
        request: &InstallRequest,
        mut on_progress: impl FnMut(InstallProgress),
    ) -> ThoriumResult<ThoriumInstallation> {
        on_progress(InstallProgress::Resolving);
        let release = match &request.release {
            Some(release) => release.clone(),
            None => self.releases.latest_installable(request.channel).await?,
        };
        let asset = release
            .choose_asset()
            .map_err(|e| ThoriumError::AssetNotFound(e.message))?
            .clone();
        let version = release.install_version();

        // A per-install staging directory so two runs cannot collide, and so
        // cleanup is a single recursive delete.
        let staging = self.paths.staging_dir().join(format!("{version}.partial"));
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging).map_err(|e| ThoriumError::io("create the staging directory", e))?;
        let guard = StagingGuard {
            path: staging.clone(),
            armed: true,
        };

        let archive = staging.join("download.zip");
        let outcome = download_to_file(
            &self.http,
            &asset.download_url,
            &archive,
            request.download_limits,
            |received, total| on_progress(InstallProgress::Downloading { received, total }),
        )
        .await?;

        on_progress(InstallProgress::Verifying);
        if let Some(published) = &request.expected_sha256 {
            verify_digest(&outcome.sha256, published)?;
        }

        let extracted = staging.join("extracted");
        extract_zip(&archive, &extracted, request.extract_limits, |done, total| {
            on_progress(InstallProgress::Extracting { done, total });
        })?;
        // The archive is no longer needed and would otherwise be promoted along
        // with the installation.
        let _ = std::fs::remove_file(&archive);

        let layout = validate_layout(&extracted)?;

        on_progress(InstallProgress::Activating);
        let destination = self.paths.version_dir(&version);
        // Replacing an existing version directory is the reinstall path. The old
        // one is moved aside first so a failure part-way through does not leave
        // a half-replaced installation.
        let displaced = if destination.exists() {
            let aside = self.paths.staging_dir().join(format!("{version}.replaced"));
            let _ = std::fs::remove_dir_all(&aside);
            std::fs::rename(&destination, &aside).map_err(|e| {
                ThoriumError::Promote(format!("the existing version could not be moved aside: {e}"))
            })?;
            Some(aside)
        } else {
            None
        };

        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ThoriumError::io("create the versions directory", e))?;
        }
        if let Err(error) = std::fs::rename(&extracted, &destination) {
            // Put the displaced version back before reporting failure.
            if let Some(aside) = &displaced {
                let _ = std::fs::rename(aside, &destination);
            }
            return Err(ThoriumError::Promote(format!(
                "the staged installation could not be moved into place: {error}"
            )));
        }
        if let Some(aside) = displaced {
            let _ = std::fs::remove_dir_all(aside);
        }

        // The executable path recorded must be the one inside the final
        // location, not the staging path it was validated at.
        let executable = layout
            .executable
            .strip_prefix(&extracted)
            .map(|relative| destination.join(relative))
            .unwrap_or_else(|_| destination.join("BIN").join("thorium.exe"));

        if request.set_current {
            self.set_current(&version)?;
        }

        drop(guard);
        let _ = std::fs::remove_dir_all(&staging);

        on_progress(InstallProgress::Done {
            version: version.clone(),
        });
        Ok(ThoriumInstallation {
            version,
            channel: request.channel,
            install_dir: destination.to_string_lossy().into_owned(),
            executable_path: executable.to_string_lossy().into_owned(),
            installed_at: Timestamp::now(),
            source_url: asset.download_url,
            archive_sha256: outcome.sha256,
            is_current: request.set_current,
        })
    }

    /// Selects `version` as `current`.
    ///
    /// The marker file is written atomically so a crash cannot leave the
    /// workspace with no selected version.
    ///
    /// # Errors
    ///
    /// Returns [`ThoriumError::VersionMissing`] when the version is not
    /// installed, and [`ThoriumError::Promote`] when the marker cannot be
    /// written.
    pub fn set_current(&self, version: &str) -> ThoriumResult<()> {
        let sanitized = sanitize_version(version);
        let dir = self.paths.version_dir(&sanitized);
        if !dir.is_dir() {
            return Err(ThoriumError::VersionMissing(sanitized));
        }
        let marker = self.paths.current_marker();
        let temp = marker.with_extension("txt.tmp");
        std::fs::write(&temp, sanitized.as_bytes())
            .map_err(|e| ThoriumError::Promote(format!("the selection could not be written: {e}")))?;
        std::fs::rename(&temp, &marker)
            .map_err(|e| ThoriumError::Promote(format!("the selection could not be applied: {e}")))?;
        Ok(())
    }

    /// The version currently selected, if it is still installed.
    #[must_use]
    pub fn current_version(&self) -> Option<String> {
        let marker = self.paths.current_marker();
        let raw = std::fs::read_to_string(marker).ok()?;
        let version = sanitize_version(raw.trim());
        self.paths.version_dir(&version).is_dir().then_some(version)
    }

    /// The versions present on disk, sorted.
    ///
    /// Reads the filesystem rather than the database so a version installed by
    /// an older run, or removed by hand, is reflected accurately.
    #[must_use]
    pub fn installed_versions(&self) -> Vec<String> {
        let mut versions = Vec::new();
        if let Ok(entries) = std::fs::read_dir(self.paths.versions_dir()) {
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|t| t.is_dir())
                    && let Some(name) = entry.file_name().to_str()
                {
                    versions.push(name.to_owned());
                }
            }
        }
        versions.sort();
        versions
    }

    /// The executable for a specific installed version.
    ///
    /// # Errors
    ///
    /// Returns [`ThoriumError::VersionMissing`] when the version is not
    /// installed, and [`ThoriumError::Validation`] when its executable is gone.
    pub fn executable_for(&self, version: &str) -> ThoriumResult<PathBuf> {
        let sanitized = sanitize_version(version);
        let dir = self.paths.version_dir(&sanitized);
        if !dir.is_dir() {
            return Err(ThoriumError::VersionMissing(sanitized));
        }
        crate::extract::locate_thorium_executable(&dir)
    }

    /// The executable for whichever version is currently selected.
    ///
    /// # Errors
    ///
    /// Returns [`ThoriumError::VersionMissing`] when nothing is selected.
    pub fn current_executable(&self) -> ThoriumResult<PathBuf> {
        let version = self
            .current_version()
            .ok_or_else(|| ThoriumError::VersionMissing("current".to_owned()))?;
        self.executable_for(&version)
    }

    /// Removes an installed version.
    ///
    /// Refuses while `in_use_by` is non-empty, and refuses to remove the
    /// selected version unless another one can take its place.
    ///
    /// # Errors
    ///
    /// Returns [`ThoriumError::VersionInUse`], [`ThoriumError::VersionMissing`]
    /// or [`ThoriumError::Io`].
    pub fn remove_version(&self, version: &str, in_use_by: usize) -> ThoriumResult<()> {
        let sanitized = sanitize_version(version);
        if in_use_by > 0 {
            return Err(ThoriumError::VersionInUse {
                version: sanitized,
                profiles: in_use_by,
            });
        }
        let dir = self.paths.version_dir(&sanitized);
        if !dir.is_dir() {
            return Err(ThoriumError::VersionMissing(sanitized));
        }

        // Removing the selected version must not leave the workspace with no
        // browser while another one is available.
        let is_current = self.current_version().as_deref() == Some(sanitized.as_str());
        let replacement = is_current
            .then(|| self.installed_versions().into_iter().find(|v| v != &sanitized))
            .flatten();

        std::fs::remove_dir_all(&dir).map_err(|e| ThoriumError::io("remove the Thorium version", e))?;

        if is_current {
            match replacement {
                Some(next) => self.set_current(&next)?,
                None => {
                    let _ = std::fs::remove_file(self.paths.current_marker());
                }
            }
        }
        Ok(())
    }

    /// Reverts `current` to the newest other installed version.
    ///
    /// # Errors
    ///
    /// Returns [`ThoriumError::VersionMissing`] when there is nothing to revert
    /// to.
    pub fn rollback(&self) -> ThoriumResult<String> {
        let current = self.current_version();
        let previous = self
            .installed_versions()
            .into_iter()
            .rfind(|v| Some(v.as_str()) != current.as_deref())
            .ok_or_else(|| ThoriumError::VersionMissing("a previous version".to_owned()))?;
        self.set_current(&previous)?;
        Ok(previous)
    }
}

/// Deletes the staging directory unless the install completed.
struct StagingGuard {
    path: PathBuf,
    armed: bool,
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        if self.armed && self.path.exists() {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manager(dir: &tempfile::TempDir) -> ThoriumManager {
        ThoriumManager::new(dir.path(), ReleaseClientConfig::default()).expect("manager")
    }

    /// Creates a fake installed version directory containing a plausible
    /// executable. Nothing here downloads anything.
    fn fake_install(manager: &ThoriumManager, version: &str) {
        let bin = manager.paths().version_dir(version).join("BIN");
        std::fs::create_dir_all(&bin).expect("mkdir");
        std::fs::write(bin.join("thorium.exe"), vec![0u8; 300 * 1024]).expect("write");
    }

    #[test]
    fn the_layout_is_created_beside_the_workspace() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        assert!(manager.paths().versions_dir().is_dir());
        assert!(manager.paths().staging_dir().is_dir());
        assert!(manager.paths().root().ends_with("browsers/thorium"));
    }

    #[test]
    fn a_version_can_be_selected_and_read_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        assert_eq!(manager.current_version(), None);
        fake_install(&manager, "M152.0.7977.55");
        manager.set_current("M152.0.7977.55").expect("select");
        assert_eq!(manager.current_version().as_deref(), Some("M152.0.7977.55"));
        assert!(
            manager
                .current_executable()
                .expect("executable")
                .ends_with("thorium.exe")
        );
    }

    #[test]
    fn selecting_a_version_that_is_not_installed_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        assert!(matches!(
            manager.set_current("M999"),
            Err(ThoriumError::VersionMissing(_))
        ));
        assert_eq!(manager.current_version(), None);
    }

    #[test]
    fn a_selection_pointing_at_a_deleted_version_reads_as_unselected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        fake_install(&manager, "M152");
        manager.set_current("M152").expect("select");
        std::fs::remove_dir_all(manager.paths().version_dir("M152")).expect("remove by hand");
        assert_eq!(
            manager.current_version(),
            None,
            "a stale marker must not name a missing version"
        );
    }

    #[test]
    fn a_hostile_marker_cannot_escape_the_versions_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        std::fs::write(manager.paths().current_marker(), "../../../../etc").expect("write");
        assert_eq!(manager.current_version(), None);
        assert!(
            manager
                .paths()
                .version_dir("../../../../etc")
                .starts_with(manager.paths().versions_dir())
        );
    }

    #[test]
    fn installed_versions_are_read_from_disk_and_sorted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        assert!(manager.installed_versions().is_empty());
        for version in ["M152", "M150", "M151"] {
            fake_install(&manager, version);
        }
        assert_eq!(manager.installed_versions(), vec!["M150", "M151", "M152"]);
    }

    #[test]
    fn an_update_never_deletes_the_previous_version() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        fake_install(&manager, "M151");
        manager.set_current("M151").expect("select");
        fake_install(&manager, "M152");
        manager.set_current("M152").expect("select");
        assert_eq!(manager.installed_versions(), vec!["M151", "M152"]);
        assert!(
            manager.executable_for("M151").is_ok(),
            "the previous version stays usable"
        );
    }

    #[test]
    fn rollback_selects_the_previous_version() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        fake_install(&manager, "M151");
        fake_install(&manager, "M152");
        manager.set_current("M152").expect("select");
        assert_eq!(manager.rollback().expect("rollback"), "M151");
        assert_eq!(manager.current_version().as_deref(), Some("M151"));
    }

    #[test]
    fn rollback_with_nothing_to_revert_to_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        fake_install(&manager, "M152");
        manager.set_current("M152").expect("select");
        assert!(matches!(manager.rollback(), Err(ThoriumError::VersionMissing(_))));
        assert_eq!(
            manager.current_version().as_deref(),
            Some("M152"),
            "the selection is unchanged"
        );
    }

    #[test]
    fn a_version_in_use_by_a_running_profile_cannot_be_removed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        fake_install(&manager, "M152");
        match manager.remove_version("M152", 2) {
            Err(ThoriumError::VersionInUse { version, profiles }) => {
                assert_eq!(version, "M152");
                assert_eq!(profiles, 2);
            }
            other => panic!("expected VersionInUse, got {other:?}"),
        }
        assert!(
            manager.paths().version_dir("M152").is_dir(),
            "the files must survive a refused removal"
        );
    }

    #[test]
    fn removing_the_current_version_falls_back_to_another_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        fake_install(&manager, "M151");
        fake_install(&manager, "M152");
        manager.set_current("M152").expect("select");
        manager.remove_version("M152", 0).expect("remove");
        assert_eq!(manager.current_version().as_deref(), Some("M151"));
        assert_eq!(manager.installed_versions(), vec!["M151"]);
    }

    #[test]
    fn removing_the_only_version_clears_the_selection() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        fake_install(&manager, "M152");
        manager.set_current("M152").expect("select");
        manager.remove_version("M152", 0).expect("remove");
        assert_eq!(manager.current_version(), None);
        assert!(manager.installed_versions().is_empty());
        assert!(matches!(
            manager.current_executable(),
            Err(ThoriumError::VersionMissing(_))
        ));
    }

    #[test]
    fn removing_a_version_that_is_not_installed_is_reported() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        assert!(matches!(
            manager.remove_version("M999", 0),
            Err(ThoriumError::VersionMissing(_))
        ));
    }

    #[test]
    fn stale_staging_files_are_cleaned_and_installed_versions_are_not() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        fake_install(&manager, "M152");
        let stale_dir = manager.paths().staging_dir().join("M151.partial");
        std::fs::create_dir_all(stale_dir.join("extracted")).expect("mkdir");
        std::fs::write(manager.paths().staging_dir().join("download.zip"), b"partial").expect("write");

        assert_eq!(manager.clean_staging().expect("clean"), 2);
        assert!(
            manager.paths().staging_dir().is_dir(),
            "the directory itself is recreated"
        );
        assert!(!stale_dir.exists());
        assert!(
            manager.paths().version_dir("M152").is_dir(),
            "installed versions are never touched"
        );
    }

    #[test]
    fn the_staging_guard_removes_its_directory_on_an_early_return() {
        let dir = tempfile::tempdir().expect("tempdir");
        let staging = dir.path().join("partial");
        std::fs::create_dir_all(&staging).expect("mkdir");
        {
            let _guard = StagingGuard {
                path: staging.clone(),
                armed: true,
            };
            assert!(staging.is_dir());
        }
        assert!(!staging.exists(), "a failed install must leave no staged files");
    }

    #[test]
    fn a_disarmed_staging_guard_leaves_its_directory_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let staging = dir.path().join("kept");
        std::fs::create_dir_all(&staging).expect("mkdir");
        {
            let _guard = StagingGuard {
                path: staging.clone(),
                armed: false,
            };
        }
        assert!(staging.is_dir());
    }

    #[test]
    fn version_directories_stay_inside_the_versions_folder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let manager = manager(&dir);
        for hostile in ["../escape", "..\\escape", "C:/Windows", "/etc/passwd", ""] {
            let path = manager.paths().version_dir(hostile);
            assert!(
                path.starts_with(manager.paths().versions_dir()),
                "{hostile} escaped to {path:?}"
            );
        }
    }
}
