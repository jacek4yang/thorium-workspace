//! Release discovery and bounded downloads from GitHub releases.
//!
//! Security/network posture:
//! - rustls only (no OpenSSL runtime dependency);
//! - downloads stream to disk with hard caps (time and size) and are
//!   written under a `.part` name, so interrupted downloads never look
//!   like finished archives;
//! - the catalog lists upstream repositories in priority order; assets
//!   are matched by pattern from [`crate::catalog`], never by stale
//!   hard-coded names.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::catalog::{Variant, parse_portable_zip};
use crate::error::ThoriumError;

/// Upstream repository that publishes Windows portable builds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRepository {
    /// `owner/repo` on GitHub.
    pub slug: &'static str,
}

/// Upstream catalog, in priority order. M152+ Windows portable builds are
/// published on `gz83/thorium`; `Alex313031/Thorium` remains the upstream
/// project root.
pub const SOURCES: &[SourceRepository] = &[
    SourceRepository {
        slug: "gz83/thorium",
    },
    SourceRepository {
        slug: "Alex313031/Thorium",
    },
];

/// One discovered release with its portable Windows assets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// Up repository tag, e.g. `M152.0.7977.55`.
    pub tag: String,
    /// Browser version parsed from asset names, e.g. `152.0.7977.55`.
    pub version: String,
    /// Available portable Windows variants and their download URLs.
    pub assets: Vec<(Variant, String, u64)>,
}

impl Release {
    /// Finds the asset for a variant.
    pub fn asset_for(&self, variant: Variant) -> Option<&(Variant, String, u64)> {
        self.assets
            .iter()
            .find(|(candidate, _, _)| candidate == &variant)
    }
}

/// GitHub client for release discovery and downloads.
#[derive(Debug)]
pub struct Client {
    /// Shared HTTP client; `pub(crate)` so the proxy module can build
    /// proxy-routed instances without exposing construction details.
    pub(crate) http: reqwest::Client,
}

impl Client {
    /// Builds a client with the default runtime configuration.
    pub fn new() -> Result<Self, ThoriumError> {
        let http = reqwest::Client::builder()
            .user_agent(concat!(
                "thorium-workspace/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/jacek4yang/thorium-workspace)"
            ))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| ThoriumError::Discovery(error.to_string()))?;
        Ok(Self { http })
    }

    /// Discovers recent releases with portable Windows assets across the
    /// catalog. Returns releases newest-first per repository, limited to
    /// `per_source` entries per source.
    pub async fn discover_windows_releases(
        &self,
        per_source: usize,
    ) -> Result<Vec<(String, Vec<Release>)>, ThoriumError> {
        let mut per_source_releases = Vec::new();
        for source in SOURCES {
            let url = format!(
                "https://api.github.com/repos/{}/releases?per_page={}",
                source.slug, per_source
            );
            let response = self
                .http
                .get(&url)
                .header("Accept", "application/vnd.github+json")
                .send()
                .await
                .map_err(|error| ThoriumError::Discovery(error.to_string()))?;
            let status = response.status();
            let releases: Vec<serde_json::Value> = response
                .json()
                .await
                .map_err(|error| ThoriumError::Discovery(error.to_string()))?;
            let tag_releases = match status {
                reqwest::StatusCode::OK => releases,
                _ => {
                    return Err(ThoriumError::Discovery(format!(
                        "GitHub API returned {status}"
                    )));
                }
            };
            let parsed = tag_releases
                .iter()
                .filter_map(parse_release)
                .collect::<Vec<_>>();
            per_source_releases.push((source.slug.to_owned(), parsed));
        }
        Ok(per_source_releases)
    }

    /// Downloads `url` into `directory` under a bounded budget. Returns
    /// the path of the completed archive. The download goes to
    /// `<version>-<variant>.zip.part` and is renamed once complete.
    pub async fn download_bounded(
        &self,
        url: &str,
        directory: &Path,
        file_name: &str,
        max_bytes: u64,
        max_duration: Duration,
        progress: &mut (dyn FnMut(u64, u64) + Send),
    ) -> Result<PathBuf, ThoriumError> {
        std::fs::create_dir_all(directory).map_err(|source| ThoriumError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let target = directory.join(format!("{file_name}.part"));
        let final_path = directory.join(file_name);
        let response = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|error| ThoriumError::Download {
                detail: error.to_string(),
            })?
            .error_for_status()
            .map_err(|error| ThoriumError::Download {
                detail: error.to_string(),
            })?;
        let total = response.content_length().unwrap_or(0);
        if total > max_bytes {
            return Err(ThoriumError::Download {
                detail: format!("content length {total} exceeds budget {max_bytes}"),
            });
        }
        let started = std::time::Instant::now();
        let mut file =
            tokio::fs::File::create(&target)
                .await
                .map_err(|source| ThoriumError::Io {
                    path: target.clone(),
                    source,
                })?;
        use futures_util::StreamExt as _;
        use tokio::io::AsyncWriteExt;
        let mut downloaded: u64 = 0;
        let mut response_stream = response.bytes_stream();
        loop {
            if started.elapsed() > max_duration {
                return Err(ThoriumError::Download {
                    detail: "download exceeded the time budget".to_owned(),
                });
            }
            if downloaded > max_bytes {
                return Err(ThoriumError::Download {
                    detail: format!("download exceeded the size budget {max_bytes}"),
                });
            }
            match response_stream.next().await {
                Some(Ok(chunk)) => {
                    file.write_all(&chunk)
                        .await
                        .map_err(|source| ThoriumError::Io {
                            path: target.clone(),
                            source,
                        })?;
                    downloaded += chunk.len() as u64;
                    progress(downloaded, total);
                }
                Some(Err(error)) => {
                    return Err(ThoriumError::Download {
                        detail: error.to_string(),
                    });
                }
                None => break,
            }
        }
        file.flush().await.map_err(|source| ThoriumError::Io {
            path: target.clone(),
            source,
        })?;
        drop(file);
        std::fs::rename(&target, &final_path).map_err(|source| ThoriumError::Io {
            path: final_path.clone(),
            source,
        })?;
        Ok(final_path)
    }
}

fn parse_release(release: &serde_json::Value) -> Option<Release> {
    let tag = release.get("tag_name")?.as_str()?.to_owned();
    let assets = release.get("assets")?.as_array()?;
    let mut parsed_assets = Vec::new();
    let mut version: Option<String> = None;
    for asset in assets {
        let name = asset.get("name")?.as_str()?;
        let Some(parsed) = parse_portable_zip(name) else {
            continue;
        };
        if version.is_none() {
            version = Some(parsed.version.clone());
        }
        let Some(url) = asset.get("browser_download_url")?.as_str() else {
            continue;
        };
        let Some(size) = asset.get("size")?.as_u64() else {
            continue;
        };
        parsed_assets.push((parsed.variant, url.to_owned(), size));
    }
    Some(Release {
        tag,
        version: version?,
        assets: parsed_assets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_are_prioritized() {
        assert_eq!(SOURCES[0].slug, "gz83/thorium");
        assert_eq!(SOURCES[1].slug, "Alex313031/Thorium");
    }

    /// Live upstream test (ignored by default so CI never depends on the
    /// network): parses real release assets and exercises the bounded
    /// download machinery against a small response. Requires the dev
    /// proxy to be running and THORIUM_TEST_PROXY set, e.g.
    /// `THORIUM_TEST_PROXY=http://127.0.0.1:10808`.
    #[tokio::test]
    #[ignore]
    async fn live_discovery_and_download_machinery() {
        let Ok(proxy) = std::env::var("THORIUM_TEST_PROXY") else {
            panic!("set THORIUM_TEST_PROXY to run the live test");
        };
        let client = Client::new_with_proxy(&proxy).expect("client");
        let found = client
            .discover_windows_releases(5)
            .await
            .expect("discovery succeeds");
        let total: usize = found.iter().map(|(_, releases)| releases.len()).sum();
        assert!(total > 0, "expected at least one portable Windows release");
        let first = found
            .iter()
            .find_map(|(_, releases)| releases.first())
            .expect("a release with portable assets");
        assert!(
            first
                .version
                .chars()
                .all(|c| c.is_ascii_digit() || c == '.')
        );
        assert!(!first.assets.is_empty());
        // The download machinery must handle the JSON body of the API
        // itself under the bounded budget (small; no 300 MB browser).
        let dir = tempfile::tempdir().expect("tempdir");
        let api_url = "https://api.github.com/rate_limit";
        let downloaded = client
            .download_bounded(
                api_url,
                dir.path(),
                "probe.json",
                10 * 1024 * 1024,
                std::time::Duration::from_secs(60),
                &mut |_, _| {},
            )
            .await
            .expect("bounded download");
        assert!(downloaded.is_file());
        assert!(!downloaded.to_string_lossy().contains(".part"));
    }

    /// Live probe test (ignored by default; requires THORIUM_TEST_PROXY):
    /// the exit IP observed through the proxy must be a parseable IP
    /// literal. Verified live on 2026-09-03 through http://127.0.0.1:10808.
    #[tokio::test]
    #[ignore]
    async fn live_exit_ip_probe_through_proxy() {
        let Ok(proxy) = std::env::var("THORIUM_TEST_PROXY") else {
            panic!("set THORIUM_TEST_PROXY to run the live test");
        };
        let client = Client::new_with_proxy(&proxy).expect("client");
        let ip = client.fetch_exit_ip().await.expect("probe succeeds");
        assert!(ip.parse::<std::net::IpAddr>().is_ok(), "got {ip}");
    }
}
