//! Portable Thorium version management.
//!
//! Thorium is downloaded at runtime rather than bundled: it is a full Chromium
//! build, and shipping it inside the workspace executable would make the
//! portable EXE enormous and stale.
//!
//! # Safety of the install pipeline
//!
//! Everything upstream publishes is untrusted input. The pipeline is therefore:
//!
//! 1. **Discover** the release through the GitHub API and pick an asset by
//!    *rule*, never by a hard-coded file name (see
//!    [`tw_domain::thorium::AssetSelectionRules`]).
//! 2. **Download** with a total size cap, a per-read timeout and a wall-clock
//!    limit, into a staging directory that is deleted on any failure.
//! 3. **Verify** the SHA-256 of what actually arrived, against an upstream
//!    digest when one is published, and record it either way.
//! 4. **Extract** with every entry path validated: no absolute paths, no `..`,
//!    no symlinks, a bounded entry count and a bounded uncompressed total.
//! 5. **Validate** that the result actually looks like Thorium.
//! 6. **Promote** by renaming the staged directory into place, then switching
//!    `current` atomically. The previous version is never deleted as part of an
//!    update.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod download;
mod extract;
mod install;
mod releases;

pub use download::{DownloadLimits, DownloadOutcome, download_to_file};
pub use extract::{ExtractLimits, ExtractedLayout, extract_zip, locate_thorium_executable};
pub use install::{InstallProgress, InstallRequest, ThoriumManager, ThoriumPaths};
pub use releases::{ReleaseClient, ReleaseClientConfig};

use tw_domain::DiagnosticCode;

/// Failures raised by the Thorium manager.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ThoriumError {
    /// Upstream release information could not be retrieved.
    #[error("Thorium release information could not be retrieved: {0}")]
    ReleaseLookup(String),
    /// No asset in the release matched the channel's rules.
    #[error("{0}")]
    AssetNotFound(String),
    /// The download failed, timed out or exceeded its size limit.
    #[error("the Thorium download failed: {0}")]
    Download(String),
    /// The downloaded archive did not match its published digest.
    #[error("the downloaded archive does not match the digest published upstream")]
    DigestMismatch {
        /// The digest upstream published.
        expected: String,
        /// The digest of what actually arrived.
        actual: String,
    },
    /// The archive could not be extracted, or an entry was rejected.
    #[error("the Thorium archive could not be extracted: {0}")]
    Extraction(String),
    /// The extracted tree does not look like a Thorium installation.
    #[error("the downloaded archive does not contain a Thorium browser: {0}")]
    Validation(String),
    /// The staged installation could not be promoted.
    #[error("the new Thorium version could not be activated: {0}")]
    Promote(String),
    /// The requested version is not installed.
    #[error("Thorium version {0} is not installed")]
    VersionMissing(String),
    /// The version is in use by a running profile.
    #[error("Thorium {version} cannot be removed while {profiles} profile(s) are using it")]
    VersionInUse {
        /// Which version.
        version: String,
        /// How many profiles.
        profiles: usize,
    },
    /// An I/O operation failed.
    #[error("{operation} failed: {source}")]
    Io {
        /// What was being attempted.
        operation: &'static str,
        /// The underlying error.
        #[source]
        source: std::io::Error,
    },
}

impl ThoriumError {
    /// The stable diagnostic code for this error.
    #[must_use]
    pub const fn code(&self) -> DiagnosticCode {
        match self {
            Self::ReleaseLookup(_) => DiagnosticCode::ThoriumReleaseLookupFailed,
            Self::AssetNotFound(_) => DiagnosticCode::ThoriumAssetNotFound,
            Self::Download(_) => DiagnosticCode::ThoriumDownloadFailed,
            Self::DigestMismatch { .. } => DiagnosticCode::ThoriumDigestMismatch,
            Self::Extraction(_) | Self::Validation(_) => DiagnosticCode::ThoriumExtractionFailed,
            Self::Promote(_) => DiagnosticCode::ThoriumPromoteFailed,
            Self::VersionMissing(_) => DiagnosticCode::ThoriumVersionMissing,
            Self::VersionInUse { .. } => DiagnosticCode::ThoriumVersionInUse,
            Self::Io { .. } => DiagnosticCode::IoFailed,
        }
    }

    pub(crate) fn io(operation: &'static str, source: std::io::Error) -> Self {
        Self::Io { operation, source }
    }
}

/// Thorium result alias.
pub type ThoriumResult<T> = Result<T, ThoriumError>;

/// Installs the process-wide rustls crypto provider, once.
///
/// reqwest is built with `rustls-no-provider` so the provider is an explicit
/// choice rather than a transitive default. Installing it more than once is not
/// an error here: another component may legitimately have installed the same
/// provider first.
pub fn install_crypto_provider() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

/// Computes the lowercase hex SHA-256 of a file.
///
/// # Errors
///
/// Returns [`ThoriumError::Io`] when the file cannot be read.
pub fn sha256_file(path: &std::path::Path) -> ThoriumResult<String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).map_err(|e| ThoriumError::io("open the archive", e))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| ThoriumError::io("read the archive", e))?;
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_map_to_stable_diagnostic_codes() {
        assert_eq!(
            ThoriumError::AssetNotFound("x".into()).code(),
            DiagnosticCode::ThoriumAssetNotFound
        );
        assert_eq!(
            ThoriumError::DigestMismatch {
                expected: "a".into(),
                actual: "b".into()
            }
            .code(),
            DiagnosticCode::ThoriumDigestMismatch
        );
        assert_eq!(
            ThoriumError::VersionInUse {
                version: "v".into(),
                profiles: 2
            }
            .code(),
            DiagnosticCode::ThoriumVersionInUse
        );
    }

    #[test]
    fn the_digest_of_a_known_file_is_correct() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("f");
        std::fs::write(&path, b"abc").expect("write");
        // The published SHA-256 of "abc".
        assert_eq!(
            sha256_file(&path).expect("digest"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_missing_file_is_an_io_error_not_a_panic() {
        assert!(matches!(
            sha256_file(std::path::Path::new("/definitely/missing")),
            Err(ThoriumError::Io { .. })
        ));
    }
}
