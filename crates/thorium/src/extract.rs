//! Archive extraction with path validation.
//!
//! A ZIP archive from the internet can name any path it likes. Every entry is
//! therefore validated before a single byte is written: absolute paths, drive
//! letters, `..` components, symlinks and reserved Windows device names are all
//! rejected outright, and the entry count and uncompressed total are bounded so
//! a zip bomb cannot fill the disk.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use crate::{ThoriumError, ThoriumResult};

/// Limits applied while extracting.
#[derive(Debug, Clone, Copy)]
pub struct ExtractLimits {
    /// Largest number of entries accepted.
    pub max_entries: usize,
    /// Largest uncompressed total accepted, in bytes.
    pub max_total_bytes: u64,
    /// Largest single file accepted, in bytes.
    pub max_file_bytes: u64,
}

impl Default for ExtractLimits {
    fn default() -> Self {
        Self {
            // A Chromium build is large but not unbounded: roughly 3,000 files
            // and 600 MB uncompressed, with the largest single file being the
            // main binary at a few hundred MB.
            max_entries: 100_000,
            max_total_bytes: 6 * 1024 * 1024 * 1024,
            max_file_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
}

/// What an extracted archive turned out to contain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedLayout {
    /// Directory the archive was extracted into.
    pub root: PathBuf,
    /// Path to `thorium.exe` within `root`.
    pub executable: PathBuf,
    /// Number of files written.
    pub files: usize,
    /// Total uncompressed bytes written.
    pub bytes: u64,
}

/// Extracts `archive` into `destination`.
///
/// # Errors
///
/// Returns [`ThoriumError::Extraction`] when the archive is malformed, an entry
/// is rejected or a limit is exceeded, and [`ThoriumError::Io`] on a write
/// failure.
pub fn extract_zip(
    archive: &Path,
    destination: &Path,
    limits: ExtractLimits,
    mut on_progress: impl FnMut(usize, usize),
) -> ThoriumResult<(usize, u64)> {
    let file = std::fs::File::open(archive).map_err(|e| ThoriumError::io("open the archive", e))?;
    let mut zip = zip::ZipArchive::new(std::io::BufReader::new(file))
        .map_err(|e| ThoriumError::Extraction(format!("the archive could not be opened: {e}")))?;

    let entry_count = zip.len();
    if entry_count > limits.max_entries {
        return Err(ThoriumError::Extraction(format!(
            "the archive contains {entry_count} entries, over the {} entry limit",
            limits.max_entries
        )));
    }

    std::fs::create_dir_all(destination)
        .map_err(|e| ThoriumError::io("create the extraction directory", e))?;

    let mut files_written = 0usize;
    let mut bytes_written = 0u64;

    for index in 0..entry_count {
        let mut entry = zip
            .by_index(index)
            .map_err(|e| ThoriumError::Extraction(format!("entry {index} could not be read: {e}")))?;

        // `enclosed_name` already rejects absolute paths and parent traversal;
        // the explicit check below is kept because it is the invariant this
        // whole function exists to uphold, and because it also rejects the
        // Windows-specific shapes `enclosed_name` does not know about.
        let raw_name = entry.name().to_owned();
        let relative = safe_relative_path(&raw_name)?;

        if entry.is_dir() {
            std::fs::create_dir_all(destination.join(&relative))
                .map_err(|e| ThoriumError::io("create a directory from the archive", e))?;
            continue;
        }

        // A symlink in an archive can point anywhere, including outside the
        // workspace. Thorium's portable archive contains none.
        if is_symlink(&entry) {
            return Err(ThoriumError::Extraction(format!(
                "the archive contains a symbolic link ({relative:?}), which is not allowed"
            )));
        }

        let declared = entry.size();
        if declared > limits.max_file_bytes {
            return Err(ThoriumError::Extraction(format!(
                "an entry declares {declared} bytes, over the {} byte limit",
                limits.max_file_bytes
            )));
        }

        let target = destination.join(&relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ThoriumError::io("create a directory from the archive", e))?;
        }

        let mut out = std::fs::File::create(&target)
            .map_err(|e| ThoriumError::io("write a file from the archive", e))?;
        // Copy through a bounded reader so a lying `size` field cannot be used
        // to write more than the limit allows.
        let remaining_budget = limits.max_total_bytes.saturating_sub(bytes_written);
        let mut limited = entry
            .by_ref()
            .take(remaining_budget.min(limits.max_file_bytes).saturating_add(1));
        let copied = std::io::copy(&mut limited, &mut out)
            .map_err(|e| ThoriumError::io("write a file from the archive", e))?;

        if copied > limits.max_file_bytes || bytes_written.saturating_add(copied) > limits.max_total_bytes {
            drop(out);
            let _ = std::fs::remove_file(&target);
            return Err(ThoriumError::Extraction(
                "the archive expands to more data than the extraction limit allows".to_owned(),
            ));
        }

        bytes_written = bytes_written.saturating_add(copied);
        files_written += 1;
        on_progress(index + 1, entry_count);
    }

    Ok((files_written, bytes_written))
}

/// Validates one archive entry name and returns it as a relative path.
fn safe_relative_path(raw: &str) -> ThoriumResult<PathBuf> {
    if raw.is_empty() {
        return Err(ThoriumError::Extraction(
            "the archive contains an entry with no name".to_owned(),
        ));
    }
    // Both separators appear in archives produced on either platform.
    let normalized = raw.replace('\\', "/");
    if normalized.starts_with('/') {
        return Err(ThoriumError::Extraction(format!(
            "the archive contains an absolute path ({raw}), which is not allowed"
        )));
    }
    // A drive-qualified path such as `C:/Windows/System32/x.dll`.
    if normalized.as_bytes().get(1) == Some(&b':') {
        return Err(ThoriumError::Extraction(format!(
            "the archive contains a drive-qualified path ({raw}), which is not allowed"
        )));
    }

    let mut out = PathBuf::new();
    for component in Path::new(&normalized).components() {
        match component {
            Component::Normal(part) => {
                let text = part.to_string_lossy();
                if is_reserved_windows_name(&text) {
                    return Err(ThoriumError::Extraction(format!(
                        "the archive contains a reserved Windows name ({text}), which is not allowed"
                    )));
                }
                out.push(part);
            }
            Component::CurDir => {}
            Component::ParentDir => {
                return Err(ThoriumError::Extraction(format!(
                    "the archive contains a path that escapes its directory ({raw}), which is not allowed"
                )));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(ThoriumError::Extraction(format!(
                    "the archive contains an absolute path ({raw}), which is not allowed"
                )));
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err(ThoriumError::Extraction(
            "the archive contains an entry with no usable name".to_owned(),
        ));
    }
    Ok(out)
}

/// Whether a path component is a reserved Windows device name.
///
/// Creating `CON` or `LPT1` on Windows does not create a file; it opens a
/// device. Rejecting them keeps extraction predictable.
fn is_reserved_windows_name(component: &str) -> bool {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let stem = component
        .split('.')
        .next()
        .unwrap_or(component)
        .to_ascii_uppercase();
    RESERVED.contains(&stem.as_str())
}

/// Whether a ZIP entry is a symbolic link, according to its Unix mode bits.
fn is_symlink<R: std::io::Read>(entry: &zip::read::ZipFile<'_, R>) -> bool {
    // S_IFLNK is 0o120000 in the high bits of the external attributes.
    entry
        .unix_mode()
        .is_some_and(|mode| mode & 0o170_000 == 0o120_000)
}

/// Finds `thorium.exe` inside an extracted tree.
///
/// Upstream portable archives wrap everything in a versioned top-level folder
/// and place the binaries under `BIN`, but that layout has changed before, so
/// the search is a bounded walk rather than a fixed path.
///
/// # Errors
///
/// Returns [`ThoriumError::Validation`] when no executable is found.
pub fn locate_thorium_executable(root: &Path) -> ThoriumResult<PathBuf> {
    const MAX_DEPTH: usize = 6;
    const EXECUTABLE: &str = "thorium.exe";

    let mut queue = std::collections::VecDeque::from([(root.to_path_buf(), 0usize)]);
    let mut inspected = 0usize;
    while let Some((dir, depth)) = queue.pop_front() {
        if depth > MAX_DEPTH || inspected > 20_000 {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut subdirectories = Vec::new();
        for entry in entries.flatten() {
            inspected += 1;
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                subdirectories.push(path);
            } else if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.eq_ignore_ascii_case(EXECUTABLE))
            {
                return Ok(path);
            }
        }
        for sub in subdirectories {
            queue.push_back((sub, depth + 1));
        }
    }
    Err(ThoriumError::Validation(format!(
        "{EXECUTABLE} was not found in the extracted archive"
    )))
}

/// Checks an extracted tree looks like a usable Thorium installation.
///
/// # Errors
///
/// Returns [`ThoriumError::Validation`] when the executable is missing or
/// implausibly small.
pub fn validate_layout(root: &Path) -> ThoriumResult<ExtractedLayout> {
    let executable = locate_thorium_executable(root)?;
    let metadata = std::fs::metadata(&executable)
        .map_err(|e| ThoriumError::io("inspect the extracted browser executable", e))?;
    // A real chrome.exe stub is well over a megabyte. A tiny file here means the
    // archive was truncated or is not what it claims to be.
    const MIN_EXECUTABLE_BYTES: u64 = 256 * 1024;
    if metadata.len() < MIN_EXECUTABLE_BYTES {
        return Err(ThoriumError::Validation(format!(
            "the extracted thorium.exe is only {} bytes, which is too small to be a browser",
            metadata.len()
        )));
    }
    Ok(ExtractedLayout {
        root: root.to_path_buf(),
        executable,
        files: 0,
        bytes: metadata.len(),
    })
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn build_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buffer = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut buffer);
            let options: zip::write::FileOptions<'_, ()> =
                zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            for (name, content) in entries {
                writer.start_file(*name, options).expect("start");
                writer.write_all(content).expect("write");
            }
            writer.finish().expect("finish");
        }
        buffer.into_inner()
    }

    fn write_zip(dir: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
        let path = dir.join("archive.zip");
        std::fs::write(&path, build_zip(entries)).expect("write archive");
        path
    }

    #[test]
    fn a_realistic_portable_layout_extracts_and_validates() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Mirrors the upstream portable layout: a versioned top-level folder
        // with the binaries under BIN.
        let big = vec![0u8; 300 * 1024];
        let archive = write_zip(
            dir.path(),
            &[
                ("THORIUM_M152/README.txt", b"portable instructions".as_slice()),
                ("THORIUM_M152/CONTENT_SHELL.bat", b"@echo off".as_slice()),
                ("THORIUM_M152/BIN/thorium.exe", big.as_slice()),
                ("THORIUM_M152/BIN/152.0.7977.55/resources.pak", b"data".as_slice()),
            ],
        );
        let out = dir.path().join("extracted");
        let (files, bytes) =
            extract_zip(&archive, &out, ExtractLimits::default(), |_, _| {}).expect("extract");
        assert_eq!(files, 4);
        assert!(bytes > 300 * 1024);

        let layout = validate_layout(&out).expect("validate");
        assert!(
            layout.executable.ends_with("THORIUM_M152/BIN/thorium.exe"),
            "{:?}",
            layout.executable
        );
    }

    #[test]
    fn a_traversal_entry_is_rejected_and_writes_nothing_outside() {
        let dir = tempfile::tempdir().expect("tempdir");
        let archive = write_zip(dir.path(), &[("../../escaped.txt", b"pwned".as_slice())]);
        let out = dir.path().join("extracted");
        let err = extract_zip(&archive, &out, ExtractLimits::default(), |_, _| {}).expect_err("must reject");
        assert!(err.to_string().contains("escapes its directory"), "{err}");
        assert!(!dir.path().join("escaped.txt").exists());
        assert!(
            !dir.path()
                .parent()
                .unwrap_or(dir.path())
                .join("escaped.txt")
                .exists()
        );
    }

    #[test]
    fn absolute_and_drive_qualified_entries_are_rejected() {
        assert!(safe_relative_path("/etc/passwd").is_err());
        assert!(safe_relative_path("C:/Windows/System32/evil.dll").is_err());
        assert!(safe_relative_path("C:\\Windows\\System32\\evil.dll").is_err());
        assert!(safe_relative_path("\\\\server\\share\\x").is_err());
    }

    #[test]
    fn reserved_windows_names_are_rejected() {
        assert!(safe_relative_path("BIN/CON").is_err());
        assert!(safe_relative_path("BIN/nul.txt").is_err());
        assert!(safe_relative_path("BIN/LPT1.dll").is_err());
        assert!(
            safe_relative_path("BIN/console.dll").is_ok(),
            "only exact device names are reserved"
        );
    }

    #[test]
    fn ordinary_names_survive_normalization() {
        assert_eq!(
            safe_relative_path("BIN/thorium.exe").expect("ok"),
            PathBuf::from("BIN/thorium.exe")
        );
        assert_eq!(
            safe_relative_path("BIN\\thorium.exe").expect("ok"),
            PathBuf::from("BIN/thorium.exe")
        );
        assert_eq!(
            safe_relative_path("./BIN/./x.dll").expect("ok"),
            PathBuf::from("BIN/x.dll")
        );
        assert!(safe_relative_path("").is_err());
    }

    #[test]
    fn an_entry_count_over_the_limit_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        let archive = write_zip(
            dir.path(),
            &[("a.txt", b"a".as_slice()), ("b.txt", b"b".as_slice())],
        );
        let limits = ExtractLimits {
            max_entries: 1,
            ..Default::default()
        };
        let err = extract_zip(&archive, &dir.path().join("out"), limits, |_, _| {}).expect_err("must refuse");
        assert!(err.to_string().contains("entry limit"), "{err}");
    }

    #[test]
    fn an_archive_that_expands_past_the_total_limit_is_refused() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Highly compressible content: small on disk, large when expanded.
        let bomb = vec![0u8; 512 * 1024];
        let archive = write_zip(dir.path(), &[("big.bin", bomb.as_slice())]);
        let limits = ExtractLimits {
            max_total_bytes: 1024,
            max_file_bytes: 1024,
            ..Default::default()
        };
        let err = extract_zip(&archive, &dir.path().join("out"), limits, |_, _| {}).expect_err("must refuse");
        assert!(err.to_string().contains("limit"), "{err}");
    }

    #[test]
    fn a_corrupt_archive_is_reported_not_panicked() {
        let dir = tempfile::tempdir().expect("tempdir");
        let archive = dir.path().join("archive.zip");
        std::fs::write(&archive, b"this is not a zip file at all").expect("write");
        let err = extract_zip(
            &archive,
            &dir.path().join("out"),
            ExtractLimits::default(),
            |_, _| {},
        )
        .expect_err("must fail");
        assert!(matches!(err, ThoriumError::Extraction(_)), "{err:?}");
    }

    #[test]
    fn an_archive_without_a_browser_fails_validation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let archive = write_zip(dir.path(), &[("docs/readme.txt", b"nothing here".as_slice())]);
        let out = dir.path().join("extracted");
        extract_zip(&archive, &out, ExtractLimits::default(), |_, _| {}).expect("extract");
        let err = validate_layout(&out).expect_err("must fail validation");
        assert!(matches!(err, ThoriumError::Validation(_)), "{err:?}");
        assert!(err.to_string().contains("thorium.exe"), "{err}");
    }

    #[test]
    fn a_truncated_browser_executable_fails_validation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let archive = write_zip(dir.path(), &[("BIN/thorium.exe", b"MZ".as_slice())]);
        let out = dir.path().join("extracted");
        extract_zip(&archive, &out, ExtractLimits::default(), |_, _| {}).expect("extract");
        let err = validate_layout(&out).expect_err("must fail validation");
        assert!(err.to_string().contains("too small"), "{err}");
    }

    #[test]
    fn the_executable_search_is_case_insensitive_and_bounded() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("a").join("b").join("c");
        std::fs::create_dir_all(&nested).expect("mkdir");
        std::fs::write(nested.join("Thorium.EXE"), vec![0u8; 300 * 1024]).expect("write");
        let found = locate_thorium_executable(dir.path()).expect("locate");
        assert!(found.ends_with("Thorium.EXE"));

        let empty = tempfile::tempdir().expect("tempdir");
        assert!(locate_thorium_executable(empty.path()).is_err());
    }
}
