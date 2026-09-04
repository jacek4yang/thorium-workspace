//! Typed Thorium manager errors with stable diagnostic codes.

use thorium_workspace_domain::DiagnosticCode;

/// Error type for Thorium release discovery, download, and install.
#[derive(Debug, thiserror::Error)]
pub enum ThoriumError {
    /// A filesystem operation failed.
    #[error("filesystem operation failed for {path}: {source}")]
    Io {
        /// Target path.
        path: std::path::PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },

    /// Release discovery (GitHub API) failed.
    #[error("release discovery failed: {0}")]
    Discovery(String),

    /// The configured download proxy endpoint was rejected by the HTTP
    /// client. The proxy URL is deliberately not embedded: it may contain
    /// proxy credentials.
    #[error("download proxy configuration is not usable")]
    ProxyConfig,

    /// The proxy connectivity probe (ip.sb exit-IP check) failed or the
    /// answer was not an IP literal.
    #[error("proxy probe failed: {0}")]
    Probe(String),

    /// The download failed, timed out, or exceeded the size budget.
    #[error("download failed: {detail}")]
    Download {
        /// What went wrong.
        detail: String,
    },

    /// The downloaded archive is not a usable portable release.
    #[error("downloaded archive is not a valid Thorium portable release: {detail}")]
    InvalidArchive {
        /// Why the archive was rejected.
        detail: String,
    },

    /// The requested version/variant is not installed.
    #[error("Thorium version {version} is not installed")]
    NotInstalled {
        /// Requested version.
        version: String,
    },

    /// The install directory is already present.
    #[error("Thorium version {version} is already installed; delete it first to reinstall")]
    AlreadyInstalled {
        /// Existing version.
        version: String,
    },

    /// Deletion was blocked because the version is protected (current or
    /// in use by a running profile).
    #[error("Thorium version {version} is protected and cannot be deleted")]
    DeleteProtected {
        /// Protected version.
        version: String,
    },
}

impl DiagnosticCode for ThoriumError {
    fn diagnostic_code(&self) -> &'static str {
        match self {
            Self::Io { .. } => "THORIUM_IO_FAILED",
            Self::Discovery(_) => "THORIUM_DISCOVERY_FAILED",
            Self::ProxyConfig => "THORIUM_PROXY_CONFIG",
            Self::Probe(_) => "THORIUM_PROBE_FAILED",
            Self::Download { .. } => "THORIUM_DOWNLOAD_FAILED",
            Self::InvalidArchive { .. } => "THORIUM_INVALID_ARCHIVE",
            Self::NotInstalled { .. } => "THORIUM_NOT_INSTALLED",
            Self::AlreadyInstalled { .. } => "THORIUM_ALREADY_INSTALLED",
            Self::DeleteProtected { .. } => "THORIUM_DELETE_PROTECTED",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_codes_are_stable() {
        assert_eq!(
            ThoriumError::DeleteProtected {
                version: "152.0".to_owned()
            }
            .diagnostic_code(),
            "THORIUM_DELETE_PROTECTED"
        );
        let error = ThoriumError::Download {
            detail: "timed out".to_owned(),
        };
        assert_eq!(error.diagnostic_code(), "THORIUM_DOWNLOAD_FAILED");
    }
}
