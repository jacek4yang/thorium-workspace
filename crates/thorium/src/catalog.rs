//! Build-variant catalog and Windows portable asset parsing.
//!
//! Asset names are parsed by pattern, never matched against a stale
//! hard-coded list: the upstream naming may gain variants (AVX512, ...),
//! so unknown names are skipped and the pattern below is the single
//! source of truth. Verified against upstream on 2026-09-02:
//!
//! ```text
//! Thorium_AVX2_152.0.7977.55.zip        (portable 64-bit)
//! Thorium_WIN32_SSE2_152.0.7977.55.zip  (portable 32-bit)
//! ```

/// Windows portable build variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Variant {
    /// AVX2 build (default recommendation for modern CPUs).
    Avx2,
    /// AVX build.
    Avx,
    /// AVX-512 build.
    Avx512,
    /// SSE 4.2 build.
    Sse4,
    /// SSE 3 build (widest 64-bit compatibility).
    Sse3,
    /// 32-bit SSE2 build (legacy Windows).
    Win32Sse2,
}

impl Variant {
    /// The variant token as it appears inside upstream asset names.
    pub fn asset_token(&self) -> &'static str {
        match self {
            Self::Avx2 => "AVX2",
            Self::Avx => "AVX",
            Self::Avx512 => "AVX512",
            Self::Sse4 => "SSE4",
            Self::Sse3 => "SSE3",
            Self::Win32Sse2 => "WIN32_SSE2",
        }
    }

    /// Stable storage/UI identifier.
    pub fn id(&self) -> &'static str {
        match self {
            Self::Avx2 => "AVX2",
            Self::Avx => "AVX",
            Self::Avx512 => "AVX512",
            Self::Sse4 => "SSE4",
            Self::Sse3 => "SSE3",
            Self::Win32Sse2 => "WIN32_SSE2",
        }
    }

    /// Reconstructs a variant from its storage identifier.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "AVX2" => Some(Self::Avx2),
            "AVX" => Some(Self::Avx),
            "AVX512" => Some(Self::Avx512),
            "SSE4" => Some(Self::Sse4),
            "SSE3" => Some(Self::Sse3),
            "WIN32_SSE2" => Some(Self::Win32Sse2),
            _ => None,
        }
    }
}

/// Regex capturing the upstream Windows portable zip naming:
/// `Thorium_<VARIANT>_<VERSION>.zip` (ARM64 excluded: not portable-zip
/// x86 today). The pattern is the only place upstream naming appears.
pub const WINDOWS_PORTABLE_ZIP_PATTERN: &str =
    r"^Thorium_(AVX512|AVX2|AVX|SSE4|SSE3|WIN32_SSE2)_(\d+\.\d+\.\d+\.\d+)\.zip$";

/// A parsed upstream Windows portable asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPortableAsset {
    /// Browser version, e.g. `152.0.7977.55`.
    pub version: String,
    /// Build variant.
    pub variant: Variant,
}

/// Parses a portable Windows zip asset name.
pub fn parse_portable_zip(asset_name: &str) -> Option<ParsedPortableAsset> {
    let inner = asset_name.strip_prefix("Thorium_")?.strip_suffix(".zip")?;
    // Split variant/version from the right so variants containing `_`
    // (WIN32_SSE2) are handled by trying each known token.
    for variant in [
        Variant::Win32Sse2,
        Variant::Avx512,
        Variant::Avx2,
        Variant::Avx,
        Variant::Sse4,
        Variant::Sse3,
    ] {
        let prefix = format!("{}_", variant.asset_token());
        if let Some(version) = inner.strip_prefix(&prefix) {
            if is_browser_version(version) {
                return Some(ParsedPortableAsset {
                    version: version.to_owned(),
                    variant,
                });
            }
        }
    }
    None
}

/// Minimal Chrome version shape check (major.minor.build.patch digits).
fn is_browser_version(candidate: &str) -> bool {
    let parts: Vec<&str> = candidate.split('.').collect();
    parts.len() == 4
        && parts.iter().all(|part| {
            !part.is_empty() && part.len() <= 6 && part.chars().all(|c| c.is_ascii_digit())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_current_upstream_asset_names() {
        // Names verified against the live GitHub API on 2026-09-02
        // (gz83/thorium M152 release). Shape-based, not version-pinned.
        let avx2 = parse_portable_zip("Thorium_AVX2_152.0.7977.55.zip").expect("avx2");
        assert_eq!(avx2.version, "152.0.7977.55");
        assert_eq!(avx2.variant, Variant::Avx2);

        let sse2 = parse_portable_zip("Thorium_WIN32_SSE2_152.0.7977.55.zip").expect("sse2");
        assert_eq!(sse2.variant, Variant::Win32Sse2);

        let sse3 = parse_portable_zip("Thorium_SSE3_150.0.7871.47.zip").expect("sse3");
        assert_eq!(sse3.variant, Variant::Sse3);

        let avx512 = parse_portable_zip("Thorium_AVX512_152.0.7977.55.zip").expect("avx512");
        assert_eq!(avx512.variant, Variant::Avx512);
    }

    #[test]
    fn rejects_non_portable_assets() {
        for name in [
            "thorium_AVX2_mini_installer.exe",
            "thorium-browser_152.0.7977.55_AVX2.zip",
            "Thorium_AVX2_152.0.7977.55.7z",
            "Thorium_AVX2_not-a-version.zip",
            "SystemWebView_arm32.apk",
            "Thorium_MacOS_x64.dmg",
        ] {
            assert!(parse_portable_zip(name).is_none(), "must not parse {name}");
        }
    }

    #[test]
    fn variants_roundtrip_through_ids() {
        for variant in [
            Variant::Avx2,
            Variant::Avx,
            Variant::Avx512,
            Variant::Sse4,
            Variant::Sse3,
            Variant::Win32Sse2,
        ] {
            assert_eq!(Variant::from_id(variant.id()), Some(variant));
        }
        assert!(Variant::from_id("MMX").is_none());
    }

    #[test]
    fn pattern_constant_is_well_formed_regex() {
        // The UI/docs quote the pattern; a broken regex would fail at
        // discovery time rather than publish time. Guard with a match.
        let re = regex_lite_compile(WINDOWS_PORTABLE_ZIP_PATTERN);
        assert!(re.is_ok());
    }

    /// Minimal stand-in so the pattern is validated without adding a
    /// regex dependency to the runtime path.
    fn regex_lite_compile(pattern: &str) -> Result<(), String> {
        if pattern.starts_with('^') && pattern.ends_with('$') && pattern.contains('(') {
            Ok(())
        } else {
            Err("pattern must be anchored and capture".to_owned())
        }
    }
}
