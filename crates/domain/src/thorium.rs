//! Thorium releases, installations and the rules used to pick a download.
//!
//! Upstream publishes several Windows variants per release and has renamed its
//! assets more than once. Nothing here hard-codes a file name: a channel is a
//! GitHub repository plus a set of *rules*, and asset selection is a scored
//! match evaluated against whatever the release actually contains. When upstream
//! renames an asset the rules keep working; when they stop matching the user
//! gets [`crate::DiagnosticCode::ThoriumAssetNotFound`] instead of a wrong file.

use core::fmt;

use serde::{Deserialize, Serialize};

use crate::error::{DomainError, DomainResult};
use crate::time::Timestamp;

/// A source of portable Thorium builds.
///
/// Each variant names an upstream repository; the CPU token distinguishes the
/// builds inside a repository that serves more than one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThoriumChannel {
    /// 64-bit AVX2 builds. The fastest build most modern CPUs can run.
    #[default]
    WindowsAvx2,
    /// 64-bit AVX builds. The upstream baseline for Windows.
    WindowsAvx,
    /// 64-bit SSE3 builds, for CPUs without AVX.
    WindowsSse3,
    /// Windows on ARM (arm64) builds.
    WindowsArm64,
}

impl ThoriumChannel {
    /// Every selectable channel.
    #[must_use]
    pub const fn all() -> &'static [ThoriumChannel] {
        &[
            Self::WindowsAvx2,
            Self::WindowsAvx,
            Self::WindowsSse3,
            Self::WindowsArm64,
        ]
    }

    /// The stored discriminant.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WindowsAvx2 => "windows_avx2",
            Self::WindowsAvx => "windows_avx",
            Self::WindowsSse3 => "windows_sse3",
            Self::WindowsArm64 => "windows_arm64",
        }
    }

    /// Human-readable channel name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::WindowsAvx2 => "Windows x64 (AVX2)",
            Self::WindowsAvx => "Windows x64 (AVX)",
            Self::WindowsSse3 => "Windows x64 (SSE3)",
            Self::WindowsArm64 => "Windows on ARM (arm64)",
        }
    }

    /// The GitHub owner that publishes this channel.
    #[must_use]
    pub const fn owner(self) -> &'static str {
        "Alex313031"
    }

    /// The GitHub repository that publishes this channel.
    ///
    /// Verified against upstream documentation for the M150-M152 era: AVX2
    /// Windows builds moved to their own repository, Windows-on-ARM has always
    /// had one, and SSE3/SSE4/32-bit builds are served from `Thorium-Special`.
    #[must_use]
    pub const fn repository(self) -> &'static str {
        match self {
            Self::WindowsAvx2 => "Thorium-Win-AVX2",
            Self::WindowsAvx => "Thorium-Win",
            Self::WindowsSse3 => "Thorium-Special",
            Self::WindowsArm64 => "Thorium-WOA",
        }
    }

    /// The upstream releases page, shown in the UI so a user can always check
    /// what the app is about to download.
    #[must_use]
    pub fn releases_url(self) -> String {
        format!(
            "https://github.com/{}/{}/releases",
            self.owner(),
            self.repository()
        )
    }

    /// The rules used to choose a portable archive from a release.
    #[must_use]
    pub fn asset_rules(self) -> AssetSelectionRules {
        let mut rules = AssetSelectionRules::portable_windows_defaults();
        match self {
            Self::WindowsAvx2 => {
                rules.preferred_tokens = vec!["avx2".into()];
                rules
                    .rejected_tokens
                    .extend(["win32".into(), "arm64".into(), "woa".into()]);
            }
            Self::WindowsAvx => {
                rules.preferred_tokens = vec!["avx".into()];
                rules
                    .rejected_tokens
                    .extend(["avx2".into(), "win32".into(), "arm64".into(), "woa".into()]);
            }
            Self::WindowsSse3 => {
                rules.preferred_tokens = vec!["sse3".into()];
                rules
                    .rejected_tokens
                    .extend(["win32".into(), "sse2".into(), "arm64".into(), "woa".into()]);
            }
            Self::WindowsArm64 => {
                rules.preferred_tokens = vec!["arm64".into(), "woa".into()];
                rules.rejected_tokens.extend(["win32".into(), "avx2".into()]);
            }
        }
        rules
    }

    /// Parses a stored discriminant.
    ///
    /// # Errors
    ///
    /// Returns [`crate::DiagnosticCode::InvalidInput`] for an unknown value.
    pub fn parse(value: &str) -> DomainResult<Self> {
        Self::all()
            .iter()
            .copied()
            .find(|c| c.as_str() == value)
            .ok_or_else(|| DomainError::invalid(format!("unknown Thorium channel '{value}'")))
    }
}

impl fmt::Display for ThoriumChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Rules for picking one asset out of a release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetSelectionRules {
    /// The asset file name must end with one of these (case-insensitive).
    pub required_extensions: Vec<String>,
    /// The asset is rejected outright if its name contains any of these.
    pub rejected_tokens: Vec<String>,
    /// Presence of any of these raises the asset's score.
    pub preferred_tokens: Vec<String>,
    /// Assets larger than this are rejected as implausible.
    pub max_size_bytes: u64,
    /// Assets smaller than this are rejected as implausible.
    pub min_size_bytes: u64,
}

impl AssetSelectionRules {
    /// Defaults shared by every Windows channel.
    ///
    /// Portable builds are `.zip`; the installer, driver, debug symbols and the
    /// headless `thorium_shell` archives all live in the same release and must
    /// never be selected.
    #[must_use]
    pub fn portable_windows_defaults() -> Self {
        Self {
            required_extensions: vec![".zip".into()],
            rejected_tokens: vec![
                "installer".into(),
                "setup".into(),
                "symbols".into(),
                "debug".into(),
                "pdb".into(),
                "chromedriver".into(),
                "driver".into(),
                "shell".into(),
                "source".into(),
                "src".into(),
                "sha256".into(),
                "checksum".into(),
                "linux".into(),
                "mac".into(),
                "raspi".into(),
                "android".into(),
            ],
            preferred_tokens: Vec::new(),
            // A Thorium portable archive is roughly 150-200 MB. The bounds are
            // wide enough to survive upstream growth and tight enough to reject
            // a stray text file or an unexpectedly huge artefact.
            min_size_bytes: 40 * 1024 * 1024,
            max_size_bytes: 2 * 1024 * 1024 * 1024,
        }
    }

    /// Scores an asset. `None` means the asset is not a candidate at all.
    ///
    /// A higher score is a better match. Scoring rather than exact-matching is
    /// what keeps the app working across upstream renames.
    #[must_use]
    pub fn score(&self, asset: &ThoriumReleaseAsset) -> Option<i32> {
        let name = asset.name.to_ascii_lowercase();
        if !self
            .required_extensions
            .iter()
            .any(|ext| name.ends_with(&ext.to_ascii_lowercase()))
        {
            return None;
        }
        if self
            .rejected_tokens
            .iter()
            .any(|t| name.contains(&t.to_ascii_lowercase()))
        {
            return None;
        }
        if asset.size_bytes < self.min_size_bytes || asset.size_bytes > self.max_size_bytes {
            return None;
        }
        let mut score = 0;
        if name.contains("thorium") {
            score += 10;
        }
        if name.contains("win") {
            score += 5;
        }
        for (index, token) in self.preferred_tokens.iter().enumerate() {
            if name.contains(&token.to_ascii_lowercase()) {
                // Earlier tokens are stronger preferences.
                score += 40 - i32::try_from(index).unwrap_or(0);
            }
        }
        // Prefer the shortest matching name: upstream suffixes extra qualifiers
        // ("_no_avx", "_old") onto secondary artefacts.
        score -= i32::try_from(name.len()).unwrap_or(0) / 8;
        Some(score)
    }

    /// Picks the best-scoring asset from `assets`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::DiagnosticCode::ThoriumAssetNotFound`] when nothing
    /// matches, with the candidate names in the message so a user can report
    /// what upstream actually published.
    pub fn choose<'a>(&self, assets: &'a [ThoriumReleaseAsset]) -> DomainResult<&'a ThoriumReleaseAsset> {
        let mut best: Option<(i32, &ThoriumReleaseAsset)> = None;
        for asset in assets {
            if let Some(score) = self.score(asset)
                && best.is_none_or(|(best_score, _)| score > best_score)
            {
                best = Some((score, asset));
            }
        }
        best.map(|(_, asset)| asset).ok_or_else(|| {
            let names: Vec<&str> = assets.iter().map(|a| a.name.as_str()).collect();
            DomainError::new(
                crate::DiagnosticCode::ThoriumAssetNotFound,
                format!(
                    "no portable Thorium archive matched this release; it published: {}",
                    if names.is_empty() {
                        "no assets".to_owned()
                    } else {
                        names.join(", ")
                    }
                ),
            )
            .with_remedy(
                "Open the upstream releases page and install a version manually, or report the asset names.",
            )
        })
    }
}

/// One downloadable file attached to an upstream release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThoriumReleaseAsset {
    /// File name exactly as upstream published it.
    pub name: String,
    /// Direct download URL.
    pub download_url: String,
    /// Size in bytes as reported by the release metadata.
    pub size_bytes: u64,
}

/// An upstream release.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThoriumRelease {
    /// Upstream tag, for example `M152.0.7977.55`.
    pub tag: String,
    /// Release title.
    pub name: String,
    /// Which channel it came from.
    pub channel: ThoriumChannel,
    /// Whether upstream marked it a pre-release.
    pub prerelease: bool,
    /// Publication time, when upstream provided one.
    pub published_at: Option<Timestamp>,
    /// Page a user can open to check the release themselves.
    pub html_url: String,
    /// Every asset upstream attached.
    pub assets: Vec<ThoriumReleaseAsset>,
}

impl ThoriumRelease {
    /// The version string used for the on-disk install directory.
    ///
    /// Upstream tags are already filesystem-safe, but a tag is upstream-provided
    /// input: anything outside `[A-Za-z0-9._-]` is replaced so a crafted tag can
    /// never escape the versions directory.
    #[must_use]
    pub fn install_version(&self) -> String {
        sanitize_version(&self.tag)
    }

    /// Picks the asset to download for this release's channel.
    ///
    /// # Errors
    ///
    /// Returns [`crate::DiagnosticCode::ThoriumAssetNotFound`] when no asset
    /// matches the channel's rules.
    pub fn choose_asset(&self) -> DomainResult<&ThoriumReleaseAsset> {
        self.channel.asset_rules().choose(&self.assets)
    }
}

/// Replaces every character outside `[A-Za-z0-9._-]` with `_`, collapses leading
/// dots and bounds the length.
///
/// Used for any upstream-provided string that becomes a path component.
#[must_use]
pub fn sanitize_version(raw: &str) -> String {
    let mut out: String = raw
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    while out.starts_with('.') {
        out.remove(0);
    }
    if out.is_empty() {
        out.push_str("unknown");
    }
    out
}

/// A Thorium build installed into the workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThoriumInstallation {
    /// Sanitized upstream version, and the install directory name.
    pub version: String,
    /// Which channel it was installed from.
    pub channel: ThoriumChannel,
    /// Absolute path to the version directory.
    pub install_dir: String,
    /// Absolute path to `thorium.exe` inside the version directory.
    pub executable_path: String,
    /// When the install completed.
    pub installed_at: Timestamp,
    /// The URL the archive came from, for auditing.
    pub source_url: String,
    /// SHA-256 of the downloaded archive, lowercase hex.
    pub archive_sha256: String,
    /// Whether this version is currently promoted to `current`.
    pub is_current: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str, size_mb: u64) -> ThoriumReleaseAsset {
        ThoriumReleaseAsset {
            name: name.to_owned(),
            download_url: format!("https://example.test/{name}"),
            size_bytes: size_mb * 1024 * 1024,
        }
    }

    /// Mirrors the shape of a real upstream Windows release: a portable archive
    /// alongside the installer, the driver and the headless shell.
    fn realistic_release(channel: ThoriumChannel, assets: Vec<ThoriumReleaseAsset>) -> ThoriumRelease {
        ThoriumRelease {
            tag: "M152.0.7977.55".to_owned(),
            name: "Thorium M152".to_owned(),
            channel,
            prerelease: false,
            published_at: Some(Timestamp::from_unix_seconds(1_760_000_000)),
            html_url: "https://example.test/release".to_owned(),
            assets,
        }
    }

    #[test]
    fn installers_drivers_and_symbols_are_never_selected() {
        let release = realistic_release(
            ThoriumChannel::WindowsAvx2,
            vec![
                asset("thorium_avx2_mini_installer.exe", 120),
                asset("thorium_AVX2_152.0.7977.55.zip", 180),
                asset("chromedriver_win.zip", 60),
                asset("thorium_shell_win.zip", 90),
                asset("debug_symbols.zip", 900),
            ],
        );
        let chosen = release.choose_asset().expect("an asset matches");
        assert_eq!(chosen.name, "thorium_AVX2_152.0.7977.55.zip");
    }

    #[test]
    fn the_avx_channel_does_not_pick_an_avx2_archive() {
        let release = realistic_release(
            ThoriumChannel::WindowsAvx,
            vec![
                asset("Thorium_AVX2_152.zip", 180),
                asset("Thorium_AVX_152.zip", 180),
            ],
        );
        assert_eq!(release.choose_asset().expect("match").name, "Thorium_AVX_152.zip");
    }

    #[test]
    fn the_avx2_channel_prefers_the_avx2_archive() {
        let release = realistic_release(
            ThoriumChannel::WindowsAvx2,
            vec![
                asset("Thorium_AVX_152.zip", 180),
                asset("Thorium_AVX2_152.zip", 180),
            ],
        );
        assert_eq!(
            release.choose_asset().expect("match").name,
            "Thorium_AVX2_152.zip"
        );
    }

    #[test]
    fn thirty_two_bit_and_arm_archives_are_rejected_on_x64_channels() {
        let release = realistic_release(
            ThoriumChannel::WindowsSse3,
            vec![
                asset("Thorium_WIN32_SSE3_152.zip", 150),
                asset("Thorium_SSE3_152.zip", 170),
            ],
        );
        assert_eq!(
            release.choose_asset().expect("match").name,
            "Thorium_SSE3_152.zip"
        );
    }

    #[test]
    fn an_empty_or_unmatched_release_reports_what_it_published() {
        let release = realistic_release(ThoriumChannel::WindowsAvx2, vec![asset("readme.txt", 100)]);
        let err = release.choose_asset().expect_err("nothing matches");
        assert_eq!(err.code, crate::DiagnosticCode::ThoriumAssetNotFound);
        assert!(err.message.contains("readme.txt"), "{}", err.message);

        let empty = realistic_release(ThoriumChannel::WindowsAvx2, Vec::new());
        assert!(empty.choose_asset().unwrap_err().message.contains("no assets"));
    }

    #[test]
    fn implausible_sizes_are_rejected() {
        let rules = ThoriumChannel::WindowsAvx2.asset_rules();
        assert!(rules.score(&asset("Thorium_AVX2.zip", 1)).is_none(), "too small");
        assert!(
            rules.score(&asset("Thorium_AVX2.zip", 4096)).is_none(),
            "too large"
        );
        assert!(rules.score(&asset("Thorium_AVX2.zip", 180)).is_some());
    }

    #[test]
    fn selection_survives_an_upstream_rename() {
        // Same channel, a completely different naming scheme.
        let release = realistic_release(
            ThoriumChannel::WindowsAvx2,
            vec![
                asset("thorium-win-x64-avx2-M153.zip", 190),
                asset("thorium-win-x64-avx2-M153-installer.exe", 130),
            ],
        );
        assert_eq!(
            release.choose_asset().expect("match").name,
            "thorium-win-x64-avx2-M153.zip"
        );
    }

    #[test]
    fn versions_are_sanitized_into_safe_path_components() {
        assert_eq!(sanitize_version("M152.0.7977.55"), "M152.0.7977.55");
        assert_eq!(
            sanitize_version("../../etc/passwd"),
            "_.._etc_passwd",
            "leading dots are stripped so the name cannot traverse upwards"
        );
        assert_eq!(sanitize_version("  v1 beta  "), "v1_beta");
        assert_eq!(sanitize_version(""), "unknown");
        assert_eq!(sanitize_version("..."), "unknown");
        assert!(sanitize_version(&"x".repeat(200)).len() <= 64);
        assert!(!sanitize_version("C:\\evil").contains('\\'));
    }

    #[test]
    fn channels_round_trip_and_expose_upstream_repositories() {
        for channel in ThoriumChannel::all() {
            assert_eq!(ThoriumChannel::parse(channel.as_str()).expect("parse"), *channel);
            assert!(
                channel
                    .releases_url()
                    .starts_with("https://github.com/Alex313031/")
            );
        }
        assert!(ThoriumChannel::parse("windows_mips").is_err());
        assert_eq!(ThoriumChannel::WindowsAvx2.repository(), "Thorium-Win-AVX2");
    }
}
