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
    ///
    /// There is deliberately **no client-level total timeout**: it would
    /// kill large downloads mid-transfer (surfacing as reqwest body-decode
    /// errors). Short calls set explicit per-request timeouts; downloads
    /// are governed by the explicit size/time budgets instead.
    pub fn new() -> Result<Self, ThoriumError> {
        let http = reqwest::Client::builder()
            .user_agent(concat!(
                "thorium-workspace/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/jacek4yang/thorium-workspace)"
            ))
            .connect_timeout(Duration::from_secs(20))
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
                .timeout(Duration::from_secs(30))
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
    ///
    /// Strategy: when the server advertises byte-range support (GitHub's
    /// CDN does), the file is fetched as parallel segments with per-segment
    /// retry-and-resume — this is dramatically faster on per-connection
    /// throttled routes and survives transient proxy drops. Servers without
    /// range support fall back to a single bounded stream.
    ///
    /// The request-level timeout is disabled for the transfer itself; the
    /// `max_duration` / `max_bytes` budgets are the authoritative bounds.
    pub async fn download_bounded(
        &self,
        url: &str,
        directory: &Path,
        file_name: &str,
        max_bytes: u64,
        max_duration: Duration,
        progress: &(dyn Fn(u64, u64) + Send + Sync),
    ) -> Result<PathBuf, ThoriumError> {
        std::fs::create_dir_all(directory).map_err(|source| ThoriumError::Io {
            path: directory.to_path_buf(),
            source,
        })?;
        let target = directory.join(format!("{file_name}.part"));
        let final_path = directory.join(file_name);

        let (total, ranged) = self.probe_download(url).await?;
        match total {
            Some(size) if size > max_bytes => {
                return Err(ThoriumError::Download {
                    detail: format!("content length {size} exceeds budget {max_bytes}"),
                });
            }
            _ => {}
        }
        let segments = match total {
            Some(size) if ranged => segment_count(size),
            _ => 1,
        };

        if segments > 1 {
            let size = total.expect("segmented path requires a known size");
            download_parallel(
                &self.http,
                url,
                &target,
                size,
                segments,
                max_duration,
                progress,
            )
            .await?;
        } else {
            download_single(&self.http, url, &target, max_bytes, max_duration, progress).await?;
        }

        std::fs::rename(&target, &final_path).map_err(|source| ThoriumError::Io {
            path: final_path.clone(),
            source,
        })?;
        Ok(final_path)
    }

    /// Learns the download size and whether byte ranges are supported via a
    /// 1-byte ranged GET. `total` is `None` when the server does not report
    /// a size; `ranged` is true only when it answered `206 Partial Content`.
    async fn probe_download(&self, url: &str) -> Result<(Option<u64>, bool), ThoriumError> {
        let response = self
            .http
            .get(url)
            .timeout(Duration::from_secs(30))
            .header("Range", "bytes=0-0")
            .send()
            .await
            .map_err(|error| ThoriumError::Download {
                detail: error.to_string(),
            })?;
        match response.status() {
            reqwest::StatusCode::PARTIAL_CONTENT => {
                // Content-Range: bytes 0-0/TOTAL
                let total = response
                    .headers()
                    .get(reqwest::header::CONTENT_RANGE)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.rsplit('/').next())
                    .and_then(|total_text| total_text.trim().parse::<u64>().ok());
                Ok((total, true))
            }
            reqwest::StatusCode::OK => Ok((response.content_length(), false)),
            status => {
                let _ = response.error_for_status();
                Err(ThoriumError::Download {
                    detail: format!("download probe returned {status}"),
                })
            }
        }
    }
}

/// Segmentation policy: files under [`PARALLEL_MIN_TOTAL`] stay single
/// stream; larger ones split into up to [`TARGET_SEGMENTS`] segments of at
/// least [`MIN_SEGMENT_BYTES`] so tiny tails never spawn a segment.
const PARALLEL_MIN_TOTAL: u64 = 8 * 1024 * 1024;
const TARGET_SEGMENTS: u64 = 8;
const MIN_SEGMENT_BYTES: u64 = 4 * 1024 * 1024;
/// Per-segment retry attempts with resume; this is what turns a transient
/// proxy drop ("error decoding response body") into a hiccup instead of a
/// failed 350 MB download.
const SEGMENT_ATTEMPTS: usize = 3;

fn segment_count(total: u64) -> u64 {
    if total < PARALLEL_MIN_TOTAL {
        return 1;
    }
    TARGET_SEGMENTS.min((total / MIN_SEGMENT_BYTES).max(1))
}

/// Single bounded stream. Used for unknown-size and range-incapable
/// servers; budgets are enforced inside the read loop.
async fn download_single(
    http: &reqwest::Client,
    url: &str,
    target: &Path,
    max_bytes: u64,
    max_duration: Duration,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<(), ThoriumError> {
    let response = http
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
    let started = std::time::Instant::now();
    let mut file = tokio::fs::File::create(target)
        .await
        .map_err(|source| ThoriumError::Io {
            path: target.to_path_buf(),
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
                        path: target.to_path_buf(),
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
        path: target.to_path_buf(),
        source,
    })?;
    Ok(())
}

/// A segment failure carries how far the attempt got so the retry can
/// resume from that offset instead of restarting the segment.
struct SegmentFailure {
    written: u64,
    detail: String,
}

/// Downloads `bytes start..=end` of `url` into the preallocated `target`
/// at exactly that offset, retrying with resume up to [`SEGMENT_ATTEMPTS`]
/// times. Progress is reported through the shared atomic counter.
async fn download_segment(
    http: reqwest::Client,
    url: String,
    target: std::path::PathBuf,
    start: u64,
    end: u64,
    deadline: std::time::Instant,
    downloaded: std::sync::Arc<std::sync::atomic::AtomicU64>,
) -> Result<(), ThoriumError> {
    let expected = end - start + 1;
    let mut done: u64 = 0;
    let mut last_detail = String::from("no attempt made");
    for attempt in 1..=SEGMENT_ATTEMPTS {
        if done == expected {
            return Ok(());
        }
        match segment_attempt(
            &http,
            &url,
            &target,
            start + done,
            end,
            deadline,
            &downloaded,
        )
        .await
        {
            Ok(written) => {
                done += written;
                if done == expected {
                    return Ok(());
                }
                last_detail = format!("short segment: {done}/{expected} bytes");
            }
            Err(failure) => {
                done += failure.written;
                last_detail = failure.detail;
            }
        }
        if attempt < SEGMENT_ATTEMPTS {
            tokio::time::sleep(Duration::from_millis(300 * attempt as u64)).await;
        }
    }
    Err(ThoriumError::Download {
        detail: format!(
            "segment {start}-{end} failed after {SEGMENT_ATTEMPTS} attempts at {done}/{expected} bytes: {last_detail}"
        ),
    })
}

/// One attempt of a segment range. Returns the number of bytes appended on
/// both success and failure so retries can resume.
async fn segment_attempt(
    http: &reqwest::Client,
    url: &str,
    target: &Path,
    from: u64,
    to: u64,
    deadline: std::time::Instant,
    downloaded: &std::sync::atomic::AtomicU64,
) -> Result<u64, SegmentFailure> {
    use futures_util::StreamExt as _;
    use tokio::io::{AsyncSeekExt, AsyncWriteExt};

    let expected = to - from + 1;
    let response = http
        .get(url)
        .header("Range", format!("bytes={from}-{to}"))
        .send()
        .await
        .map_err(|error| SegmentFailure {
            written: 0,
            detail: error.to_string(),
        })?;
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        return Err(SegmentFailure {
            written: 0,
            detail: format!("range request returned {}", response.status()),
        });
    }
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .open(target)
        .await
        .map_err(|error| SegmentFailure {
            written: 0,
            detail: format!("reopen part file failed: {error}"),
        })?;
    file.seek(std::io::SeekFrom::Start(from))
        .await
        .map_err(|error| SegmentFailure {
            written: 0,
            detail: format!("seek failed: {error}"),
        })?;
    let mut written: u64 = 0;
    let mut stream = response.bytes_stream();
    loop {
        if std::time::Instant::now() > deadline {
            return Err(SegmentFailure {
                written,
                detail: "download exceeded the time budget".to_owned(),
            });
        }
        match stream.next().await {
            Some(Ok(chunk)) => {
                file.write_all(&chunk)
                    .await
                    .map_err(|error| SegmentFailure {
                        written,
                        detail: format!("write failed: {error}"),
                    })?;
                written += chunk.len() as u64;
                downloaded.fetch_add(chunk.len() as u64, std::sync::atomic::Ordering::Relaxed);
                if written > expected {
                    return Err(SegmentFailure {
                        written,
                        detail: "server sent more than the requested range".to_owned(),
                    });
                }
            }
            Some(Err(error)) => {
                return Err(SegmentFailure {
                    written,
                    detail: error.to_string(),
                });
            }
            None => {
                if written == expected {
                    return Ok(written);
                }
                // The connection dropped mid-segment: this is exactly the
                // transient failure the retry-and-resume wrapper exists for.
                return Err(SegmentFailure {
                    written,
                    detail: "connection closed before the segment completed".to_owned(),
                });
            }
        }
    }
}

/// Parallel segmented download into a preallocated file: every segment
/// opens its own handle positioned at its range start, so no concatenation
/// pass is needed. Aggregated progress is emitted from this coordinator
/// task while the segment tasks run.
async fn download_parallel(
    http: &reqwest::Client,
    url: &str,
    target: &Path,
    total: u64,
    segments: u64,
    max_duration: Duration,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<(), ThoriumError> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    // Preallocate so segments can seek-and-write concurrently.
    {
        let file = tokio::fs::File::create(target)
            .await
            .map_err(|source| ThoriumError::Io {
                path: target.to_path_buf(),
                source,
            })?;
        file.set_len(total)
            .await
            .map_err(|source| ThoriumError::Io {
                path: target.to_path_buf(),
                source,
            })?;
    }

    let deadline = std::time::Instant::now() + max_duration;
    let downloaded = Arc::new(AtomicU64::new(0));
    let seg_size = total / segments;
    let mut set = tokio::task::JoinSet::new();
    for index in 0..segments {
        let start = index * seg_size;
        let end = if index == segments - 1 {
            total - 1
        } else {
            (index + 1) * seg_size - 1
        };
        set.spawn(download_segment(
            http.clone(),
            url.to_owned(),
            target.to_path_buf(),
            start,
            end,
            deadline,
            downloaded.clone(),
        ));
    }

    // Drain the set, emitting aggregated progress between completions.
    let mut first_failure: Option<ThoriumError> = None;
    while !set.is_empty() {
        match tokio::time::timeout(Duration::from_millis(150), set.join_next()).await {
            Ok(Some(joined)) => match joined {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    first_failure.get_or_insert(error);
                }
                Err(join_error) => {
                    first_failure.get_or_insert(ThoriumError::Download {
                        detail: format!("segment task failed: {join_error}"),
                    });
                }
            },
            Ok(None) => break,
            Err(_tick) => {
                progress(downloaded.load(Ordering::Relaxed), total);
            }
        }
    }
    progress(downloaded.load(Ordering::Relaxed), total);

    if let Some(error) = first_failure {
        return Err(error);
    }
    let written = downloaded.load(Ordering::Relaxed);
    if written != total {
        return Err(ThoriumError::Download {
            detail: format!("segmented download produced {written} of {total} bytes"),
        });
    }
    Ok(())
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
                &|_, _| {},
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

    /// Live fallback test (ignored by default; requires
    /// THORIUM_TEST_PROXY): GitHub's codeload archive endpoints do NOT
    /// support byte ranges, so this exercises the single-stream fallback
    /// through the proxy end to end. The parallel segment path is covered
    /// by the local test server (segmentation, resume, budgets) and by
    /// the real release-asset install flow, whose CDN supports ranges.
    #[tokio::test]
    #[ignore]
    async fn live_fallback_download_through_proxy() {
        let Ok(proxy) = std::env::var("THORIUM_TEST_PROXY") else {
            panic!("set THORIUM_TEST_PROXY to run the live test");
        };
        let client = Client::new_with_proxy(&proxy).expect("client");
        let url = "https://github.com/git/git/archive/refs/tags/v2.47.0.zip";
        let dir = tempfile::tempdir().expect("tempdir");
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU64, Ordering};
        let last_done = Arc::new(AtomicU64::new(0));
        let last_total = Arc::new(AtomicU64::new(0));
        let progress = {
            let last_done = last_done.clone();
            let last_total = last_total.clone();
            move |done: u64, total: u64| {
                last_done.store(done, Ordering::Relaxed);
                last_total.store(total, Ordering::Relaxed);
            }
        };
        let path = client
            .download_bounded(
                url,
                dir.path(),
                "git.zip",
                64 * 1024 * 1024,
                std::time::Duration::from_secs(300),
                &progress,
            )
            .await
            .expect("parallel download through the proxy succeeds");
        let meta = std::fs::metadata(&path).expect("metadata");
        assert!(
            meta.len() > 8 * 1024 * 1024,
            "unexpected size {}",
            meta.len()
        );
        // Codeload answers without Content-Length, so the fallback reports
        // an unknown total; only the downloaded count is meaningful here.
        assert!(last_done.load(Ordering::Relaxed) > 0, "progress reported");
    }
}

#[cfg(test)]
mod download_tests {
    use super::*;

    /// A deterministic pseudo-random blob (no external rng dependency).
    fn test_blob(len: usize) -> Vec<u8> {
        let mut blob = Vec::with_capacity(len);
        let mut state: u64 = 0x9E3779B97F4A7C15;
        for _ in 0..len {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            blob.push((state >> 24) as u8);
        }
        blob
    }
    use std::io::{Read, Write as IoWrite};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    /// Minimal static-file server for exercising the downloader without
    /// touching the network. Supports `Range` requests; `ignore_ranges`
    /// forces the single-stream fallback; `truncate_bodies` closes the
    /// connection mid-body for the first N responses, simulating the
    /// proxy drops that produce reqwest body-decode errors.
    fn spawn_test_server(
        blob: Arc<Vec<u8>>,
        ignore_ranges: bool,
        truncate_bodies: usize,
    ) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr").to_string();
        let truncate = Arc::new(AtomicUsize::new(truncate_bodies));
        let ignore = Arc::new(AtomicBool::new(ignore_ranges));
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let blob = blob.clone();
                let truncate = truncate.clone();
                let ignore = ignore.clone();
                std::thread::spawn(move || {
                    let mut buffer = Vec::new();
                    let mut chunk = [0u8; 1024];
                    loop {
                        match stream.read(&mut chunk) {
                            Ok(0) => return,
                            Ok(n) => {
                                buffer.extend_from_slice(&chunk[..n]);
                                if buffer.windows(4).any(|w| w == b"\r\n\r\n") {
                                    break;
                                }
                            }
                            Err(_) => return,
                        }
                    }
                    let request = String::from_utf8_lossy(&buffer);
                    let range = request.lines().find_map(|line| {
                        line.strip_prefix("Range: bytes=")
                            .map(|value| value.trim().to_owned())
                    });

                    let (head, body) = if ignore.load(Ordering::SeqCst) {
                        (
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                blob.len()
                            ),
                            blob.as_slice().to_vec(),
                        )
                    } else if let Some(spec) = range {
                        let (start_text, end_text) = spec.split_once('-').expect("range spec");
                        let start: u64 = start_text.trim().parse().expect("start");
                        let end: u64 = if end_text.trim().is_empty() {
                            blob.len() as u64 - 1
                        } else {
                            end_text.trim().parse().expect("end")
                        };
                        let mut slice = blob[start as usize..=end as usize].to_vec();
                        // Simulate a dropped connection mid-body for the
                        // first N responses (never the 1-byte probe).
                        let should_truncate = slice.len() > 1000
                            && truncate
                                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                                    (n > 0).then(|| n - 1)
                                })
                                .is_ok();
                        if should_truncate {
                            slice.truncate(slice.len() / 2);
                        }
                        (
                            format!(
                                "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{end}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                blob.len(),
                                slice.len()
                            ),
                            slice,
                        )
                    } else {
                        (
                            format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                blob.len()
                            ),
                            blob.as_slice().to_vec(),
                        )
                    };

                    let mut response = head.into_bytes();
                    response.extend_from_slice(&body);
                    let _ = stream.write_all(&response);
                    let _ = stream.flush();
                });
            }
        });
        addr
    }

    #[tokio::test]
    async fn parallel_segments_reassemble_the_original_bytes() {
        let blob = Arc::new(test_blob(20 * 1024 * 1024)); // 5 segments of 4 MiB
        let url = format!("http://{}/blob", spawn_test_server(blob.clone(), false, 0));
        let client = Client::new().expect("client");
        let dir = tempfile::tempdir().expect("tempdir");
        let path = client
            .download_bounded(
                &url,
                dir.path(),
                "blob.bin",
                64 * 1024 * 1024,
                Duration::from_secs(120),
                &|_, _| {},
            )
            .await
            .expect("parallel download succeeds");
        let written = std::fs::read(&path).expect("read");
        assert_eq!(written.len(), blob.len());
        assert_eq!(written, *blob);
        assert!(!path.to_string_lossy().contains(".part"));
    }

    #[tokio::test]
    async fn falls_back_to_single_stream_without_range_support() {
        let blob = Arc::new(test_blob(9 * 1024 * 1024));
        let url = format!("http://{}/blob", spawn_test_server(blob.clone(), true, 0));
        let client = Client::new().expect("client");
        let dir = tempfile::tempdir().expect("tempdir");
        let path = client
            .download_bounded(
                &url,
                dir.path(),
                "blob.bin",
                64 * 1024 * 1024,
                Duration::from_secs(120),
                &|_, _| {},
            )
            .await
            .expect("single-stream download succeeds");
        assert_eq!(std::fs::read(&path).expect("read"), *blob);
    }

    #[tokio::test]
    async fn size_budget_is_enforced_before_downloading() {
        let blob = Arc::new(test_blob(20 * 1024 * 1024));
        let url = format!("http://{}/blob", spawn_test_server(blob, false, 0));
        let client = Client::new().expect("client");
        let dir = tempfile::tempdir().expect("tempdir");
        let error = client
            .download_bounded(
                &url,
                dir.path(),
                "blob.bin",
                1024 * 1024,
                Duration::from_secs(120),
                &|_, _| {},
            )
            .await
            .expect_err("over-budget download is rejected");
        assert!(error.to_string().contains("exceeds budget"));
    }

    /// The regression test for the proxy-drop failure mode: responses are
    /// cut off mid-body and the downloader must recover via resume+retry.
    #[tokio::test]
    async fn retries_recover_from_dropped_connections() {
        let blob = Arc::new(test_blob(20 * 1024 * 1024));
        let url = format!("http://{}/blob", spawn_test_server(blob.clone(), false, 12));
        let client = Client::new().expect("client");
        let dir = tempfile::tempdir().expect("tempdir");
        let path = client
            .download_bounded(
                &url,
                dir.path(),
                "blob.bin",
                64 * 1024 * 1024,
                Duration::from_secs(120),
                &|_, _| {},
            )
            .await
            .expect("download survives truncated responses");
        assert_eq!(std::fs::read(&path).expect("read"), *blob);
    }

    #[test]
    fn segmentation_policy_matches_the_documented_thresholds() {
        assert_eq!(segment_count(4 * 1024 * 1024), 1);
        assert_eq!(segment_count(8 * 1024 * 1024), 2); // two 4 MiB segments
        assert_eq!(segment_count(16 * 1024 * 1024), 4);
        assert_eq!(segment_count(64 * 1024 * 1024), 8);
        assert_eq!(segment_count(400 * 1024 * 1024), 8);
    }
}
