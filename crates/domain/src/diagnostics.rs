//! Stable diagnostic codes.
//!
//! Codes are part of the support contract: they appear in error messages, the
//! Diagnostics page and copied diagnostic reports, so they must stay stable
//! across releases. Add new codes; never renumber existing ones.

use core::fmt;

use serde::{Deserialize, Serialize};

/// A stable, user-quotable diagnostic identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[non_exhaustive]
pub enum DiagnosticCode {
    // --- Bootstrap / workspace layout: TW-01xx -------------------------------
    /// The executable directory could not be resolved.
    ExecutableDirectoryUnresolved,
    /// The executable directory exists but is not writable.
    WorkspaceNotWritable,
    /// The portable directory layout could not be created.
    WorkspaceLayoutFailed,
    /// Another Thorium Workspace instance already owns this workspace.
    WorkspaceAlreadyRunning,
    /// A stale runtime or temporary file could not be cleaned up.
    StaleRuntimeCleanupFailed,

    // --- Storage: TW-02xx ----------------------------------------------------
    /// The metadata database could not be opened.
    StorageOpenFailed,
    /// A schema migration failed.
    StorageMigrationFailed,
    /// The database schema is newer than this build understands.
    StorageSchemaTooNew,
    /// A storage query or statement failed.
    StorageQueryFailed,
    /// The requested record does not exist.
    RecordNotFound,
    /// A uniqueness rule was violated.
    RecordConflict,

    // --- Vault: TW-03xx ------------------------------------------------------
    /// No vault file exists yet.
    VaultMissing,
    /// A vault file already exists and would be overwritten.
    VaultAlreadyExists,
    /// The vault is locked and the requested operation needs it unlocked.
    VaultLocked,
    /// The supplied master password did not unlock the vault.
    VaultBadPassword,
    /// The vault file is damaged, truncated or not a Thorium Workspace vault.
    VaultCorrupt,
    /// The vault file was written by a newer format version.
    VaultFormatTooNew,
    /// The vault could not be written.
    VaultWriteFailed,
    /// The referenced secret is not present in the vault.
    SecretNotFound,

    // --- OTP / QR: TW-04xx ---------------------------------------------------
    /// An `otpauth://` URI could not be parsed.
    OtpUriInvalid,
    /// The OTP secret was not valid Base32.
    OtpSecretInvalid,
    /// The OTP parameters are outside what the standards allow.
    OtpParametersInvalid,
    /// No QR code could be found in the supplied image.
    QrNotFound,
    /// The clipboard did not contain an image.
    QrClipboardEmpty,
    /// A QR code was decoded but did not contain an `otpauth://` URI.
    QrPayloadNotOtpauth,
    /// Screen-region capture failed or is unavailable.
    ScreenCaptureFailed,

    // --- Thorium management: TW-05xx -----------------------------------------
    /// Upstream release information could not be retrieved.
    ThoriumReleaseLookupFailed,
    /// No release asset matched the configured selection rules.
    ThoriumAssetNotFound,
    /// The download failed, timed out or exceeded the configured size limit.
    ThoriumDownloadFailed,
    /// A downloaded archive did not match its expected digest.
    ThoriumDigestMismatch,
    /// The archive could not be extracted or failed structural validation.
    ThoriumExtractionFailed,
    /// The staged installation could not be promoted.
    ThoriumPromoteFailed,
    /// The requested Thorium version is not installed.
    ThoriumVersionMissing,
    /// The version is in use by a running profile and cannot be removed.
    ThoriumVersionInUse,
    /// No Thorium installation is selected.
    ThoriumNotInstalled,

    // --- Browser profiles: TW-06xx -------------------------------------------
    /// The profile is already running.
    ProfileAlreadyRunning,
    /// The profile is not running.
    ProfileNotRunning,
    /// The profile's `User Data` directory could not be prepared.
    ProfileUserDataFailed,
    /// The per-profile lock could not be acquired.
    ProfileLockFailed,
    /// Launching the Thorium process failed.
    ProfileLaunchFailed,
    /// The DevTools control channel could not be established.
    CdpUnavailable,
    /// Timezone or locale could not be applied to the running browser.
    EmulationFailed,

    // --- Platform: TW-07xx ---------------------------------------------------
    /// A Windows API call failed.
    WindowsApiFailed,
    /// The operation is only implemented on Windows.
    UnsupportedPlatform,
    /// Clipboard access failed.
    ClipboardFailed,

    // --- Backup: TW-08xx -----------------------------------------------------
    /// A backup could not be created.
    BackupFailed,
    /// A backup archive could not be restored.
    RestoreFailed,

    // --- Generic: TW-09xx ----------------------------------------------------
    /// Caller-supplied input failed validation.
    InvalidInput,
    /// An underlying I/O operation failed.
    IoFailed,
    /// An internal invariant was violated.
    Internal,
}

impl DiagnosticCode {
    /// Returns the stable printable code, for example `TW-0301`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExecutableDirectoryUnresolved => "TW-0101",
            Self::WorkspaceNotWritable => "TW-0102",
            Self::WorkspaceLayoutFailed => "TW-0103",
            Self::WorkspaceAlreadyRunning => "TW-0104",
            Self::StaleRuntimeCleanupFailed => "TW-0105",

            Self::StorageOpenFailed => "TW-0201",
            Self::StorageMigrationFailed => "TW-0202",
            Self::StorageSchemaTooNew => "TW-0203",
            Self::StorageQueryFailed => "TW-0204",
            Self::RecordNotFound => "TW-0205",
            Self::RecordConflict => "TW-0206",

            Self::VaultMissing => "TW-0301",
            Self::VaultAlreadyExists => "TW-0302",
            Self::VaultLocked => "TW-0303",
            Self::VaultBadPassword => "TW-0304",
            Self::VaultCorrupt => "TW-0305",
            Self::VaultFormatTooNew => "TW-0306",
            Self::VaultWriteFailed => "TW-0307",
            Self::SecretNotFound => "TW-0308",

            Self::OtpUriInvalid => "TW-0401",
            Self::OtpSecretInvalid => "TW-0402",
            Self::OtpParametersInvalid => "TW-0403",
            Self::QrNotFound => "TW-0404",
            Self::QrClipboardEmpty => "TW-0405",
            Self::QrPayloadNotOtpauth => "TW-0406",
            Self::ScreenCaptureFailed => "TW-0407",

            Self::ThoriumReleaseLookupFailed => "TW-0501",
            Self::ThoriumAssetNotFound => "TW-0502",
            Self::ThoriumDownloadFailed => "TW-0503",
            Self::ThoriumDigestMismatch => "TW-0504",
            Self::ThoriumExtractionFailed => "TW-0505",
            Self::ThoriumPromoteFailed => "TW-0506",
            Self::ThoriumVersionMissing => "TW-0507",
            Self::ThoriumVersionInUse => "TW-0508",
            Self::ThoriumNotInstalled => "TW-0509",

            Self::ProfileAlreadyRunning => "TW-0601",
            Self::ProfileNotRunning => "TW-0602",
            Self::ProfileUserDataFailed => "TW-0603",
            Self::ProfileLockFailed => "TW-0604",
            Self::ProfileLaunchFailed => "TW-0605",
            Self::CdpUnavailable => "TW-0606",
            Self::EmulationFailed => "TW-0607",

            Self::WindowsApiFailed => "TW-0701",
            Self::UnsupportedPlatform => "TW-0702",
            Self::ClipboardFailed => "TW-0703",

            Self::BackupFailed => "TW-0801",
            Self::RestoreFailed => "TW-0802",

            Self::InvalidInput => "TW-0901",
            Self::IoFailed => "TW-0902",
            Self::Internal => "TW-0903",
        }
    }

    /// Every code, for the documentation table and the uniqueness test.
    #[must_use]
    pub const fn all() -> &'static [DiagnosticCode] {
        use DiagnosticCode as C;
        &[
            C::ExecutableDirectoryUnresolved,
            C::WorkspaceNotWritable,
            C::WorkspaceLayoutFailed,
            C::WorkspaceAlreadyRunning,
            C::StaleRuntimeCleanupFailed,
            C::StorageOpenFailed,
            C::StorageMigrationFailed,
            C::StorageSchemaTooNew,
            C::StorageQueryFailed,
            C::RecordNotFound,
            C::RecordConflict,
            C::VaultMissing,
            C::VaultAlreadyExists,
            C::VaultLocked,
            C::VaultBadPassword,
            C::VaultCorrupt,
            C::VaultFormatTooNew,
            C::VaultWriteFailed,
            C::SecretNotFound,
            C::OtpUriInvalid,
            C::OtpSecretInvalid,
            C::OtpParametersInvalid,
            C::QrNotFound,
            C::QrClipboardEmpty,
            C::QrPayloadNotOtpauth,
            C::ScreenCaptureFailed,
            C::ThoriumReleaseLookupFailed,
            C::ThoriumAssetNotFound,
            C::ThoriumDownloadFailed,
            C::ThoriumDigestMismatch,
            C::ThoriumExtractionFailed,
            C::ThoriumPromoteFailed,
            C::ThoriumVersionMissing,
            C::ThoriumVersionInUse,
            C::ThoriumNotInstalled,
            C::ProfileAlreadyRunning,
            C::ProfileNotRunning,
            C::ProfileUserDataFailed,
            C::ProfileLockFailed,
            C::ProfileLaunchFailed,
            C::CdpUnavailable,
            C::EmulationFailed,
            C::WindowsApiFailed,
            C::UnsupportedPlatform,
            C::ClipboardFailed,
            C::BackupFailed,
            C::RestoreFailed,
            C::InvalidInput,
            C::IoFailed,
            C::Internal,
        ]
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = BTreeSet::new();
        for code in DiagnosticCode::all() {
            let text = code.as_str();
            assert!(text.starts_with("TW-"), "{text}");
            assert_eq!(text.len(), 7, "{text}");
            assert!(text[3..].chars().all(|c| c.is_ascii_digit()), "{text}");
            assert!(seen.insert(text), "duplicate diagnostic code {text}");
        }
    }

    #[test]
    fn all_lists_every_variant() {
        // A missing entry in `all()` would silently exclude a code from the
        // uniqueness check above, so pin the count explicitly.
        assert_eq!(DiagnosticCode::all().len(), 50);
    }
}
