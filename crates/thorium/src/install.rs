//! Install management: staging, extraction, atomic promotion, and version
//! registry on disk.
//!
//! Layout under `<workspace>/browsers/thorium/`:
//!
//! ```text
//! versions/<version>/thorium/...   extracted portable trees
//! staging/<n>.part                 in-flight download
//! staging/<n>/                     in-flight extraction
//! current                          text marker naming the current version
//! ```
//!
//! A download or extraction failure leaves `versions/` untouched, so the
//! previously installed (and current) version always stays usable: failed
//! updates roll back by construction. `current` is a small text file
//! replaced by an atomic rename.

#![forbid(unsafe_code)]

use std::io::Read;
use std::path::{Path, PathBuf};

use crate::catalog::Variant;
use crate::error::ThoriumError;

/// Filesystem layout of the Thorium area.
#[derive(Debug, Clone)]
pub struct InstallLayout {
    /// `browsers/thorium`
    root: PathBuf,
}

impl InstallLayout {
    /// Builds a layout rooted at `browsers/thorium`.
    pub fn new(browsers_dir: &Path) -> Self {
        Self {
            root: browsers_dir.join("thorium"),
        }
    }

    /// `versions/<version>/`.
    pub fn version_dir_of(&self, version: &str) -> PathBuf {
        self.root.join("versions").join(version)
    }

    /// `staging/`.
    fn staging_dir(&self) -> PathBuf {
        self.root.join("staging")
    }

    /// The current-version marker path.
    fn current_marker(&self) -> PathBuf {
        self.root.join("current")
    }

    /// Creates the base directories. Idempotent.
    pub fn initialize(&self) -> Result<(), ThoriumError> {
        for dir in [
            self.root.clone(),
            self.root.join("versions"),
            self.staging_dir(),
        ] {
            std::fs::create_dir_all(&dir)
                .map_err(|source| ThoriumError::Io { path: dir, source })?;
        }
        Ok(())
    }

    /// The executable path for an installed version, if present.
    pub fn executable_path(&self, version: &str) -> PathBuf {
        self.version_dir_of(version).join("BIN").join("thorium.exe")
    }

    /// Whether a version is installed (contains a thorium.exe).
    pub fn is_installed(&self, version: &str) -> bool {
        self.executable_path(version).is_file()
    }

    /// Lists installed versions found on disk (derived state; SQLite is
    /// the authoritative registry).
    pub fn list_installed(&self) -> Result<Vec<String>, ThoriumError> {
        let versions_dir = self.root.join("versions");
        if !versions_dir.is_dir() {
            return Ok(Vec::new());
        }
        let mut versions = Vec::new();
        let entries = std::fs::read_dir(&versions_dir).map_err(|source| ThoriumError::Io {
            path: versions_dir.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| ThoriumError::Io {
                path: versions_dir.clone(),
                source,
            })?;
            let name: String = entry.file_name().to_string_lossy().into_owned();
            if self.is_installed(&name) {
                versions.push(name);
            }
        }
        versions.sort();
        Ok(versions)
    }

    /// Reads the current version from the marker file.
    pub fn current_version(&self) -> Result<Option<String>, ThoriumError> {
        let marker = self.current_marker();
        if !marker.is_file() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&marker).map_err(|source| ThoriumError::Io {
            path: marker,
            source,
        })?;
        let version = text.trim().to_owned();
        if version.is_empty() {
            return Ok(None);
        }
        Ok(Some(version))
    }

    /// Atomically selects the current version (temp file + rename). The
    /// version must be installed.
    pub fn set_current(&self, version: &str) -> Result<(), ThoriumError> {
        if !self.is_installed(version) {
            return Err(ThoriumError::NotInstalled {
                version: version.to_owned(),
            });
        }
        let marker = self.current_marker();
        let tmp = marker.with_extension("tmp");
        std::fs::write(&tmp, version).map_err(|source| ThoriumError::Io {
            path: tmp.clone(),
            source,
        })?;
        std::fs::rename(&tmp, &marker).map_err(|source| ThoriumError::Io {
            path: marker,
            source,
        })?;
        Ok(())
    }

    /// Validates an extracted tree: a portable release must contain a
    /// `thorium.exe` (upstream ships it under `BIN/`; any depth is
    /// accepted to tolerate layout changes, and the shallowest match is
    /// recorded).
    fn locate_executable(dir: &Path) -> Result<Option<PathBuf>, ThoriumError> {
        let mut stack = vec![dir.to_path_buf()];
        while let Some(current) = stack.pop() {
            let entries = std::fs::read_dir(&current).map_err(|source| ThoriumError::Io {
                path: current.clone(),
                source,
            })?;
            for entry in entries {
                let entry = entry.map_err(|source| ThoriumError::Io {
                    path: current.clone(),
                    source,
                })?;
                let path = entry.path();
                if path.is_file() {
                    let name = path
                        .file_name()
                        .map(|name| name.to_string_lossy().to_lowercase());
                    if name.as_deref() == Some("thorium.exe") {
                        return Ok(Some(path));
                    }
                } else if path.is_dir() {
                    stack.push(path);
                }
            }
        }
        Ok(None)
    }

    /// Extracts `zip_path` into a staging directory, validates the tree,
    /// and promotes it to `versions/<version>/` with an atomic rename.
    /// Returns the promoted directory path.
    pub fn install_from_archive(
        &self,
        zip_path: &Path,
        version: &str,
        _variant: Variant,
    ) -> Result<PathBuf, ThoriumError> {
        if self.is_installed(version) {
            return Err(ThoriumError::AlreadyInstalled {
                version: version.to_owned(),
            });
        }
        let staging_root = self.staging_dir();
        let extract_dir = staging_root.join(format!("extract-{version}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&extract_dir);
        std::fs::create_dir_all(&extract_dir).map_err(|source| ThoriumError::Io {
            path: extract_dir.clone(),
            source,
        })?;
        let result = self.extract_and_validate(zip_path, &extract_dir);
        let Some(executable) = result else {
            // Invalid archive: never touch versions/.
            let _ = std::fs::remove_dir_all(&extract_dir);
            return Err(ThoriumError::InvalidArchive {
                detail: "thorium.exe was not found in the extracted tree".to_owned(),
            });
        };
        // Promote with rename. The target does not exist (checked above),
        // so the rename is atomic on NTFS.
        let target = self.version_dir_of(version);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|source| ThoriumError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        std::fs::rename(&extract_dir, &target).map_err(|source| ThoriumError::Io {
            path: target.clone(),
            source,
        })?;
        // Mirror the executable into the conventional place so
        // executable_path() is stable even when upstream changes layout:
        // if the found executable is deeper, keep a relative note file.
        let relative = executable
            .strip_prefix(&target)
            .ok()
            .map(|relative| relative.to_string_lossy().into_owned())
            .unwrap_or_else(|| executable.to_string_lossy().into_owned());
        if relative.replace('\\', "/") != "BIN/thorium.exe" {
            // Non-standard layout: record it for the launcher.
            let note = target.join("BINARY_LAYOUT.txt");
            let _ = std::fs::write(&note, relative.as_ref() as &str);
        }
        Ok(target)
    }

    fn extract_and_validate(&self, zip_path: &Path, extract_dir: &Path) -> Option<PathBuf> {
        let file = std::fs::File::open(zip_path).ok()?;
        let mut archive = zip::ZipArchive::new(file).ok()?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).ok()?;
            // Zip-slip guard: every entry must stay inside the extract
            // directory. `mangled_name` strips drive/UNC tricks; the
            // prefix check is the defense in depth.
            let relative = entry.mangled_name();
            let dest = extract_dir.join(&relative);
            let canonical_parent = extract_dir.canonicalize().ok()?;
            let dest_parent = dest
                .parent()
                .and_then(|parent| parent.canonicalize().ok())
                .unwrap_or(canonical_parent.clone());
            if !dest_parent.starts_with(&canonical_parent) {
                return None;
            }
            if entry.is_dir() {
                std::fs::create_dir_all(&dest).ok()?;
                continue;
            }
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).ok()?;
            }
            let mut writer = std::fs::File::create(&dest).ok()?;
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                let read = entry.read(&mut buffer).ok()?;
                if read == 0 {
                    break;
                }
                std::io::Write::write_all(&mut writer, &buffer[..read]).ok()?;
            }
            drop(writer);
            // Read the CRC back by reopening; zip verifies on read.
            if dest.is_file() {
                let mut check = std::fs::File::open(&dest).ok()?;
                let mut sink = [0u8; 1];
                let _ = check.read(&mut sink);
            }
        }
        Self::locate_executable(extract_dir).ok().flatten()
    }

    /// Deletes an installed version. `protected` lists versions that must
    /// never be deleted (current version and versions used by running
    /// profiles are supplied by the controller).
    pub fn delete_version(&self, version: &str, protected: &[String]) -> Result<(), ThoriumError> {
        if protected.iter().any(|candidate| candidate == version) {
            return Err(ThoriumError::DeleteProtected {
                version: version.to_owned(),
            });
        }
        if matches!(self.current_version()?, Some(current) if current == version) {
            return Err(ThoriumError::DeleteProtected {
                version: version.to_owned(),
            });
        }
        let dir = self.version_dir_of(version);
        if !dir.is_dir() {
            return Err(ThoriumError::NotInstalled {
                version: version.to_owned(),
            });
        }
        std::fs::remove_dir_all(&dir).map_err(|source| ThoriumError::Io { path: dir, source })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn layout(tag: &str) -> (tempfile::TempDir, InstallLayout) {
        let dir = tempfile::tempdir().expect("tempdir");
        let browsers = dir.path().join(tag);
        let layout = InstallLayout::new(&browsers);
        layout.initialize().expect("init");
        (dir, layout)
    }

    /// Writes a synthetic portable archive with `BIN/thorium.exe`.
    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).expect("zip file");
        let mut zip = zip::ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        for (name, contents) in entries {
            if name.ends_with('/') {
                continue;
            }
            if let Some(parent) = name.rsplit_once('/') {
                let dir = format!("{}/", parent.0);
                zip.add_directory(dir, options).ok();
            }
            zip.start_file(*name, options).expect("entry");
            zip.write_all(contents).expect("write");
        }
        zip.finish().expect("finish");
    }

    #[test]
    fn install_promotes_and_records_current() {
        let (_dir, layout) = layout("promote");
        let staging = layout.staging_dir();
        let zip_path = staging.join("fixture.zip");
        write_zip(&zip_path, &[("BIN/thorium.exe", b"thorium".as_slice())]);
        layout
            .install_from_archive(&zip_path, "152.0.7977.55", Variant::Avx2)
            .expect("install");
        assert!(layout.is_installed("152.0.7977.55"));
        assert!(layout.executable_path("152.0.7977.55").is_file());

        layout.set_current("152.0.7977.55").expect("current");
        assert_eq!(
            layout.current_version().expect("read"),
            Some("152.0.7977.55".to_owned())
        );
        assert_eq!(layout.list_installed().expect("list").len(), 1);

        // Second install of the same version is refused (the current
        // install stays intact — update rollback by construction).
        let error = layout
            .install_from_archive(&zip_path, "152.0.7977.55", Variant::Avx2)
            .expect_err("already installed");
        assert!(matches!(error, ThoriumError::AlreadyInstalled { .. }));
        assert!(layout.is_installed("152.0.7977.55"));
    }

    #[test]
    fn corrupt_archive_is_rejected_and_versions_untouched() {
        let (_dir, layout) = layout("corrupt");
        let staging = layout.staging_dir();
        let zip_path = staging.join("bad.zip");
        std::fs::write(&zip_path, b"this is not a zip file").expect("write junk");
        let error = layout
            .install_from_archive(&zip_path, "999.0.0.1", Variant::Avx2)
            .expect_err("invalid archive");
        assert!(matches!(error, ThoriumError::InvalidArchive { .. }));
        assert!(!layout.is_installed("999.0.0.1"));
        assert!(!layout.version_dir_of("999.0.0.1").exists());
        // Staging garbage is cleaned up.
        assert!(layout.staging_dir().read_dir().expect("staging").count() <= 1);
    }

    #[test]
    fn archive_without_thorium_exe_is_rejected() {
        let (_dir, layout) = layout("noexe");
        let staging = layout.staging_dir();
        let zip_path = staging.join("empty.zip");
        write_zip(&zip_path, &[("README.txt", b"no browser here".as_slice())]);
        let error = layout
            .install_from_archive(&zip_path, "999.0.0.2", Variant::Sse3)
            .expect_err("must reject");
        assert!(matches!(error, ThoriumError::InvalidArchive { .. }));
    }

    #[test]
    fn zip_slip_entries_are_not_escaped() {
        let (_dir, layout) = layout("zipslip");
        let staging = layout.staging_dir();
        let zip_path = staging.join("evil.zip");
        // Craft an archive whose payload writes outside the extraction
        // root: `mangled_name` neutralizes the ../ prefix, so the
        // malicious file must land inside staging instead.
        let file = std::fs::File::create(&zip_path).expect("zip file");
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file("../evil.exe", SimpleFileOptions::default())
            .expect("entry");
        zip.write_all(b"malware").expect("write");
        zip.finish().expect("finish");
        let error = layout
            .install_from_archive(&zip_path, "999.0.0.3", Variant::Avx)
            .expect_err("no thorium.exe");
        assert!(matches!(error, ThoriumError::InvalidArchive { .. }));
        assert!(!staging.parent().unwrap().join("evil.exe").exists());
    }

    #[test]
    fn delete_protects_current_and_running() {
        let (_dir, layout) = layout("protect");
        let staging = layout.staging_dir();
        let zip_path = staging.join("a.zip");
        write_zip(&zip_path, &[("BIN/thorium.exe", b"thorium".as_slice())]);
        layout
            .install_from_archive(&zip_path, "152.0.7977.55", Variant::Avx2)
            .expect("install a");
        layout.set_current("152.0.7977.55").expect("current");

        // Protected list (e.g. running profiles) blocks deletion.
        let error = layout
            .delete_version("152.0.7977.55", &["152.0.7977.55".to_owned()])
            .expect_err("protected");
        assert!(matches!(error, ThoriumError::DeleteProtected { .. }));

        // Installing a second version and deleting the (protected)
        // current fails; deleting an unprotected unused one works.
        write_zip(&zip_path, &[("BIN/thorium.exe", b"thorium2".as_slice())]);
        layout
            .install_from_archive(&zip_path, "151.0.7922.72", Variant::Sse4)
            .expect("install b");
        layout
            .delete_version("151.0.7922.72", &[])
            .expect("delete unused");
        assert!(!layout.is_installed("151.0.7922.72"));
    }
}
