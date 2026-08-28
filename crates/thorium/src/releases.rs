//! Upstream release discovery.
//!
//! Talks to the GitHub Releases API. Everything it returns is untrusted: sizes,
//! names and URLs are all attacker-influenced if upstream is compromised, so
//! each is bounded or validated before it is used.

use serde::Deserialize;
use tw_domain::{ThoriumChannel, ThoriumRelease, ThoriumReleaseAsset, Timestamp};

use crate::{ThoriumError, ThoriumResult};

/// How the release client behaves.
#[derive(Debug, Clone)]
pub struct ReleaseClientConfig {
    /// API host. Overridable so tests can point at a local fixture server.
    pub api_base: String,
    /// How long a single API request may take.
    pub request_timeout: std::time::Duration,
    /// How many releases to consider when looking for the newest usable one.
    pub page_size: u8,
    /// Whether pre-releases are eligible.
    pub include_prereleases: bool,
    /// Largest API response accepted, in bytes.
    pub max_response_bytes: usize,
}

impl Default for ReleaseClientConfig {
    fn default() -> Self {
        Self {
            api_base: "https://api.github.com".to_owned(),
            request_timeout: std::time::Duration::from_secs(30),
            // Upstream sometimes publishes a release whose Windows assets are
            // added later, or a channel-specific release with nothing usable.
            // Looking at a few lets discovery skip those instead of failing.
            page_size: 10,
            include_prereleases: false,
            max_response_bytes: 4 * 1024 * 1024,
        }
    }
}

/// Reads release metadata from GitHub.
#[derive(Debug, Clone)]
pub struct ReleaseClient {
    http: reqwest::Client,
    config: ReleaseClientConfig,
}

impl ReleaseClient {
    /// Builds a client.
    ///
    /// # Errors
    ///
    /// Returns [`ThoriumError::ReleaseLookup`] when the HTTP client cannot be
    /// constructed.
    pub fn new(config: ReleaseClientConfig) -> ThoriumResult<Self> {
        let http = reqwest::Client::builder()
            .timeout(config.request_timeout)
            .user_agent(user_agent())
            // TLS is rustls; no OpenSSL runtime dependency is introduced.
            .https_only(true)
            .build()
            .map_err(|e| ThoriumError::ReleaseLookup(e.to_string()))?;
        Ok(Self { http, config })
    }

    /// Builds a client over a caller-supplied HTTP client.
    ///
    /// Used by the integration tests to point discovery at a local fixture
    /// server over plain HTTP. Production code uses [`ReleaseClient::new`],
    /// which enforces HTTPS.
    #[must_use]
    pub const fn with_http(config: ReleaseClientConfig, http: reqwest::Client) -> Self {
        Self { http, config }
    }

    /// The configuration in use.
    #[must_use]
    pub const fn config(&self) -> &ReleaseClientConfig {
        &self.config
    }

    /// Lists recent releases for a channel, newest first.
    ///
    /// # Errors
    ///
    /// Returns [`ThoriumError::ReleaseLookup`] on a network, status or parse
    /// failure.
    pub async fn list_releases(&self, channel: ThoriumChannel) -> ThoriumResult<Vec<ThoriumRelease>> {
        let url = format!(
            "{}/repos/{}/{}/releases?per_page={}",
            self.config.api_base.trim_end_matches('/'),
            channel.owner(),
            channel.repository(),
            self.config.page_size
        );
        let response = self
            .http
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .map_err(|e| ThoriumError::ReleaseLookup(describe_request_error(&e)))?;

        let status = response.status();
        if !status.is_success() {
            return Err(ThoriumError::ReleaseLookup(format!(
                "{} {} returned HTTP {}",
                channel.owner(),
                channel.repository(),
                status.as_u16()
            )));
        }

        let body = response
            .bytes()
            .await
            .map_err(|e| ThoriumError::ReleaseLookup(e.to_string()))?;
        if body.len() > self.config.max_response_bytes {
            return Err(ThoriumError::ReleaseLookup(
                "the release listing was implausibly large and was rejected".to_owned(),
            ));
        }
        let raw: Vec<RawRelease> =
            serde_json::from_slice(&body).map_err(|e| ThoriumError::ReleaseLookup(e.to_string()))?;
        Ok(raw.into_iter().map(|r| r.into_domain(channel)).collect())
    }

    /// Returns the newest release that actually has a usable portable asset.
    ///
    /// # Errors
    ///
    /// Returns [`ThoriumError::AssetNotFound`] when no recent release has a
    /// matching asset, and propagates lookup failures.
    pub async fn latest_installable(&self, channel: ThoriumChannel) -> ThoriumResult<ThoriumRelease> {
        let releases = self.list_releases(channel).await?;
        select_installable(&releases, self.config.include_prereleases)
    }
}

/// Picks the newest release with a usable asset from an already-fetched list.
///
/// Separated from the HTTP call so the selection policy is testable without a
/// network.
///
/// # Errors
///
/// Returns [`ThoriumError::AssetNotFound`] when nothing in the list qualifies.
pub fn select_installable(
    releases: &[ThoriumRelease],
    include_prereleases: bool,
) -> ThoriumResult<ThoriumRelease> {
    let mut skipped = Vec::new();
    for release in releases {
        if release.prerelease && !include_prereleases {
            skipped.push(format!("{} (pre-release)", release.tag));
            continue;
        }
        if release.choose_asset().is_ok() {
            return Ok(release.clone());
        }
        skipped.push(format!("{} (no portable archive)", release.tag));
    }
    Err(ThoriumError::AssetNotFound(if skipped.is_empty() {
        "upstream published no releases for this channel".to_owned()
    } else {
        format!(
            "no recent release had an installable portable archive; skipped: {}",
            skipped.join(", ")
        )
    }))
}

fn user_agent() -> String {
    // GitHub rejects requests without a User-Agent. It identifies the app and
    // its version only; no machine, user or workspace information is sent.
    format!("ThoriumWorkspace/{}", env!("CARGO_PKG_VERSION"))
}

fn describe_request_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "the request timed out".to_owned()
    } else if error.is_connect() {
        "the connection could not be established".to_owned()
    } else {
        // reqwest's Display includes the URL, which is a public GitHub API
        // endpoint, so it is safe to surface.
        error.to_string()
    }
}

#[derive(Debug, Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

#[derive(Debug, Deserialize)]
struct RawAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

impl RawRelease {
    fn into_domain(self, channel: ThoriumChannel) -> ThoriumRelease {
        ThoriumRelease {
            name: self.name.clone().unwrap_or_else(|| self.tag_name.clone()),
            // A draft is not publicly installable, so it is treated exactly like
            // a pre-release for selection purposes.
            prerelease: self.prerelease || self.draft,
            published_at: self.published_at.as_deref().and_then(parse_rfc3339_seconds),
            html_url: self.html_url,
            assets: self
                .assets
                .into_iter()
                .map(|a| ThoriumReleaseAsset {
                    name: a.name,
                    download_url: a.browser_download_url,
                    size_bytes: a.size,
                })
                .collect(),
            tag: self.tag_name,
            channel,
        }
    }
}

/// Parses the `YYYY-MM-DDTHH:MM:SSZ` form GitHub emits.
///
/// A hand-rolled parser rather than a date-time dependency: the input format is
/// fixed by the API, the value is only ever displayed, and an unparseable date
/// degrades to "unknown" rather than failing the whole lookup.
fn parse_rfc3339_seconds(raw: &str) -> Option<Timestamp> {
    let bytes = raw.as_bytes();
    if bytes.len() < 20 || bytes[4] != b'-' || bytes[7] != b'-' || bytes[10] != b'T' {
        return None;
    }
    let year: i64 = raw.get(0..4)?.parse().ok()?;
    let month: i64 = raw.get(5..7)?.parse().ok()?;
    let day: i64 = raw.get(8..10)?.parse().ok()?;
    let hour: i64 = raw.get(11..13)?.parse().ok()?;
    let minute: i64 = raw.get(14..16)?.parse().ok()?;
    let second: i64 = raw.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    // Days from the civil epoch, Howard Hinnant's algorithm.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(Timestamp::from_unix_seconds(
        days * 86_400 + hour * 3_600 + minute * 60 + second,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, prerelease: bool, assets: Vec<(&str, u64)>) -> ThoriumRelease {
        ThoriumRelease {
            tag: tag.to_owned(),
            name: tag.to_owned(),
            channel: ThoriumChannel::WindowsAvx2,
            prerelease,
            published_at: None,
            html_url: String::new(),
            assets: assets
                .into_iter()
                .map(|(name, mb)| ThoriumReleaseAsset {
                    name: name.to_owned(),
                    download_url: format!("https://example.test/{name}"),
                    size_bytes: mb * 1024 * 1024,
                })
                .collect(),
        }
    }

    #[test]
    fn the_newest_release_with_a_usable_asset_wins() {
        let releases = vec![
            release("M153", false, vec![("thorium_avx2_mini_installer.exe", 120)]),
            release("M152", false, vec![("Thorium_AVX2_152.zip", 180)]),
            release("M151", false, vec![("Thorium_AVX2_151.zip", 180)]),
        ];
        let chosen = select_installable(&releases, false).expect("select");
        assert_eq!(chosen.tag, "M152", "M153 has only an installer, so it is skipped");
    }

    #[test]
    fn pre_releases_are_skipped_unless_requested() {
        let releases = vec![
            release("M153-beta", true, vec![("Thorium_AVX2_153.zip", 180)]),
            release("M152", false, vec![("Thorium_AVX2_152.zip", 180)]),
        ];
        assert_eq!(select_installable(&releases, false).expect("select").tag, "M152");
        assert_eq!(
            select_installable(&releases, true).expect("select").tag,
            "M153-beta"
        );
    }

    #[test]
    fn an_empty_or_unusable_listing_explains_what_was_skipped() {
        let err = select_installable(&[], false).expect_err("nothing to install");
        assert!(err.to_string().contains("no releases"), "{err}");

        let releases = vec![release("M153", false, vec![("readme.txt", 100)])];
        let err = select_installable(&releases, false).expect_err("nothing usable");
        assert!(err.to_string().contains("M153"), "{err}");
        assert!(err.to_string().contains("no portable archive"), "{err}");
    }

    #[test]
    fn github_release_json_maps_onto_the_domain_model() {
        // Shaped like a real GitHub releases response, with fields the app does
        // not use present to prove they are ignored rather than rejected.
        let json = r#"[
            {
                "tag_name": "M152.0.7977.55",
                "name": "Thorium M152",
                "prerelease": false,
                "draft": false,
                "published_at": "2026-07-14T09:31:05Z",
                "html_url": "https://github.com/Alex313031/Thorium-Win-AVX2/releases/tag/M152.0.7977.55",
                "author": { "login": "someone" },
                "assets": [
                    {
                        "name": "Thorium_AVX2_152.0.7977.55.zip",
                        "browser_download_url": "https://example.test/a.zip",
                        "size": 188743680,
                        "download_count": 1234,
                        "content_type": "application/zip"
                    },
                    {
                        "name": "thorium_avx2_mini_installer.exe",
                        "browser_download_url": "https://example.test/b.exe",
                        "size": 125829120
                    }
                ]
            }
        ]"#;
        let raw: Vec<RawRelease> = serde_json::from_str(json).expect("parse");
        let releases: Vec<ThoriumRelease> = raw
            .into_iter()
            .map(|r| r.into_domain(ThoriumChannel::WindowsAvx2))
            .collect();
        assert_eq!(releases.len(), 1);
        let release = &releases[0];
        assert_eq!(release.tag, "M152.0.7977.55");
        assert_eq!(release.assets.len(), 2);
        assert!(!release.prerelease);
        assert_eq!(
            release.choose_asset().expect("asset").name,
            "Thorium_AVX2_152.0.7977.55.zip"
        );
        assert_eq!(release.install_version(), "M152.0.7977.55");
    }

    #[test]
    fn a_draft_is_treated_as_not_installable() {
        let json = r#"[{"tag_name":"M153","draft":true,"prerelease":false,"assets":[]}]"#;
        let raw: Vec<RawRelease> = serde_json::from_str(json).expect("parse");
        assert!(
            raw.into_iter()
                .next()
                .expect("one")
                .into_domain(ThoriumChannel::WindowsAvx2)
                .prerelease
        );
    }

    #[test]
    fn a_release_with_missing_optional_fields_still_parses() {
        let json = r#"[{"tag_name":"M150"}]"#;
        let raw: Vec<RawRelease> = serde_json::from_str(json).expect("parse");
        let release = raw
            .into_iter()
            .next()
            .expect("one")
            .into_domain(ThoriumChannel::WindowsAvx);
        assert_eq!(release.name, "M150");
        assert!(release.assets.is_empty());
        assert_eq!(release.published_at, None);
    }

    #[test]
    fn publication_dates_are_parsed_and_bad_ones_degrade_to_unknown() {
        // 2026-07-14T09:31:05Z
        assert_eq!(
            parse_rfc3339_seconds("2026-07-14T09:31:05Z"),
            Some(Timestamp::from_unix_seconds(1_784_021_465))
        );
        // The Unix epoch itself, as a fixed point of the algorithm.
        assert_eq!(
            parse_rfc3339_seconds("1970-01-01T00:00:00Z"),
            Some(Timestamp::from_unix_seconds(0))
        );
        assert_eq!(
            parse_rfc3339_seconds("2000-03-01T00:00:00Z"),
            Some(Timestamp::from_unix_seconds(951_868_800))
        );
        assert_eq!(parse_rfc3339_seconds(""), None);
        assert_eq!(parse_rfc3339_seconds("not a date at all"), None);
        assert_eq!(parse_rfc3339_seconds("2026-13-01T00:00:00Z"), None);
        assert_eq!(parse_rfc3339_seconds("2026/07/14 09:31:05"), None);
    }

    #[test]
    fn the_user_agent_identifies_the_app_and_nothing_else() {
        let agent = user_agent();
        assert!(agent.starts_with("ThoriumWorkspace/"));
        assert!(!agent.contains(' '), "{agent}");
    }

    #[test]
    fn a_client_can_be_built_with_the_default_configuration() {
        let client = ReleaseClient::new(ReleaseClientConfig::default()).expect("build");
        assert!(client.config().api_base.starts_with("https://"));
        assert!(!client.config().include_prereleases);
    }
}
