//! Bounded archive downloads.
//!
//! A download is the point where an unbounded amount of remote data enters the
//! workspace. Every limit here exists so a hostile or broken upstream cannot
//! fill the user's disk or hang the app indefinitely.

use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::{ThoriumError, ThoriumResult};

/// Limits applied to a single download.
#[derive(Debug, Clone, Copy)]
pub struct DownloadLimits {
    /// Largest number of bytes accepted before the transfer is aborted.
    pub max_bytes: u64,
    /// Total wall-clock time allowed for the whole transfer.
    pub total_timeout: std::time::Duration,
    /// Longest gap allowed between two chunks before the transfer is treated as
    /// stalled.
    pub stall_timeout: std::time::Duration,
}

impl Default for DownloadLimits {
    fn default() -> Self {
        Self {
            // Comfortably above a Thorium portable archive (roughly 150-200 MB)
            // and far below anything that would fill a disk.
            max_bytes: 2 * 1024 * 1024 * 1024,
            total_timeout: std::time::Duration::from_secs(45 * 60),
            stall_timeout: std::time::Duration::from_secs(60),
        }
    }
}

/// What a completed download produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadOutcome {
    /// Where the bytes were written.
    pub path: PathBuf,
    /// How many bytes arrived.
    pub bytes: u64,
    /// Lowercase hex SHA-256 of what arrived.
    pub sha256: String,
}

/// Downloads `url` to `destination`, hashing as it goes.
///
/// The digest is computed from the stream rather than by re-reading the file, so
/// what is verified is exactly what was written.
///
/// `on_progress` is called with `(bytes_so_far, total_if_known)`; it must be
/// cheap, since it runs on every chunk.
///
/// # Errors
///
/// Returns [`ThoriumError::Download`] on a network failure, a non-success
/// status, a stall, a timeout or a size-limit breach, and [`ThoriumError::Io`]
/// when the file cannot be written. A failed download leaves no file behind.
pub async fn download_to_file(
    http: &reqwest::Client,
    url: &str,
    destination: &Path,
    limits: DownloadLimits,
    mut on_progress: impl FnMut(u64, Option<u64>),
) -> ThoriumResult<DownloadOutcome> {
    if let Some(parent) = destination.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| ThoriumError::io("create the staging directory", e))?;
    }

    let started = std::time::Instant::now();
    let response = http
        .get(url)
        .send()
        .await
        .map_err(|e| ThoriumError::Download(format!("the request failed: {e}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(ThoriumError::Download(format!(
            "the server returned HTTP {}",
            status.as_u16()
        )));
    }

    // A declared length over the limit is refused before a single byte is
    // written; an undeclared or understated length is still caught below.
    let declared = response.content_length();
    if let Some(length) = declared
        && length > limits.max_bytes
    {
        return Err(ThoriumError::Download(format!(
            "the archive is {length} bytes, over the {} byte limit",
            limits.max_bytes
        )));
    }

    let mut file = tokio::fs::File::create(destination)
        .await
        .map_err(|e| ThoriumError::io("create the download file", e))?;
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;
    let mut stream = response.bytes_stream();

    let result = async {
        loop {
            let next = tokio::time::timeout(limits.stall_timeout, stream.next()).await;
            let chunk = match next {
                Err(_) => return Err(ThoriumError::Download("the transfer stalled".to_owned())),
                Ok(None) => break,
                Ok(Some(Err(e))) => {
                    return Err(ThoriumError::Download(format!("the transfer failed: {e}")));
                }
                Ok(Some(Ok(chunk))) => chunk,
            };

            written = written.saturating_add(chunk.len() as u64);
            if written > limits.max_bytes {
                return Err(ThoriumError::Download(format!(
                    "the archive exceeded the {} byte limit",
                    limits.max_bytes
                )));
            }
            if started.elapsed() > limits.total_timeout {
                return Err(ThoriumError::Download("the transfer took too long".to_owned()));
            }

            hasher.update(&chunk);
            file.write_all(&chunk)
                .await
                .map_err(|e| ThoriumError::io("write the download", e))?;
            on_progress(written, declared);
        }
        file.flush()
            .await
            .map_err(|e| ThoriumError::io("flush the download", e))?;
        file.sync_all()
            .await
            .map_err(|e| ThoriumError::io("flush the download to disk", e))?;
        Ok(())
    }
    .await;

    drop(file);
    if let Err(error) = result {
        // A partial file is worse than none: it would look installable to a
        // later run.
        let _ = tokio::fs::remove_file(destination).await;
        return Err(error);
    }

    Ok(DownloadOutcome {
        path: destination.to_path_buf(),
        bytes: written,
        sha256: hex::encode(hasher.finalize()),
    })
}

/// Compares a computed digest with one published upstream.
///
/// Comparison is case-insensitive and tolerates the `<digest>  <filename>`
/// layout of a `sha256sum` file.
///
/// # Errors
///
/// Returns [`ThoriumError::DigestMismatch`] when they differ.
pub fn verify_digest(actual: &str, published: &str) -> ThoriumResult<()> {
    let expected = published
        .split_whitespace()
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if expected.is_empty() {
        return Ok(());
    }
    if expected == actual.to_ascii_lowercase() {
        Ok(())
    } else {
        Err(ThoriumError::DigestMismatch {
            expected,
            actual: actual.to_ascii_lowercase(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digest_comparison_is_case_insensitive_and_tolerates_sha256sum_format() {
        let digest = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(verify_digest(digest, digest).is_ok());
        assert!(verify_digest(digest, &digest.to_uppercase()).is_ok());
        assert!(verify_digest(digest, &format!("{digest}  Thorium_AVX2.zip")).is_ok());
        assert!(
            verify_digest(digest, "  ").is_ok(),
            "an absent published digest is not a mismatch"
        );
    }

    #[test]
    fn a_differing_digest_is_reported_with_both_values() {
        let actual = "aa".repeat(32);
        let published = "bb".repeat(32);
        match verify_digest(&actual, &published) {
            Err(ThoriumError::DigestMismatch {
                expected,
                actual: got,
            }) => {
                assert_eq!(expected, published);
                assert_eq!(got, actual);
            }
            other => panic!("expected a mismatch, got {other:?}"),
        }
    }

    #[test]
    fn default_limits_are_bounded_in_every_dimension() {
        let limits = DownloadLimits::default();
        assert!(limits.max_bytes > 0 && limits.max_bytes <= 4 * 1024 * 1024 * 1024);
        assert!(limits.total_timeout.as_secs() > 0);
        assert!(limits.stall_timeout < limits.total_timeout);
    }

    /// Exercises the download path end to end against a local one-shot HTTP
    /// server, with no external network access.
    async fn serve_once(body: Vec<u8>, status_line: &'static str) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut scratch = [0u8; 2048];
                let _ = socket.read(&mut scratch).await;
                let header = format!(
                    "{status_line}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(header.as_bytes()).await;
                let _ = socket.write_all(&body).await;
                let _ = socket.flush().await;
            }
        });
        (format!("http://{addr}/archive.zip"), handle)
    }

    #[tokio::test]
    async fn a_download_writes_the_bytes_and_hashes_the_stream() {
        let body = b"abc".to_vec();
        let (url, server) = serve_once(body.clone(), "HTTP/1.1 200 OK").await;
        let dir = tempfile::tempdir().expect("tempdir");
        let destination = dir.path().join("staging").join("archive.zip");
        crate::install_crypto_provider();
        let http = reqwest::Client::new();

        let mut progress_calls = 0;
        let outcome = download_to_file(&http, &url, &destination, DownloadLimits::default(), |_, _| {
            progress_calls += 1
        })
        .await
        .expect("download");

        assert_eq!(outcome.bytes, 3);
        assert_eq!(
            outcome.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(std::fs::read(&destination).expect("read"), body);
        assert!(progress_calls > 0, "progress must be reported");
        assert_eq!(outcome.sha256, crate::sha256_file(&destination).expect("rehash"));
        server.abort();
    }

    #[tokio::test]
    async fn an_oversized_declared_length_is_refused_before_anything_is_written() {
        let (url, server) = serve_once(vec![0u8; 4096], "HTTP/1.1 200 OK").await;
        let dir = tempfile::tempdir().expect("tempdir");
        let destination = dir.path().join("archive.zip");
        crate::install_crypto_provider();
        let http = reqwest::Client::new();
        let limits = DownloadLimits {
            max_bytes: 10,
            ..Default::default()
        };

        let err = download_to_file(&http, &url, &destination, limits, |_, _| {})
            .await
            .expect_err("must be refused");
        assert!(matches!(err, ThoriumError::Download(_)), "{err:?}");
        assert!(!destination.exists(), "a refused download must leave no file");
        server.abort();
    }

    #[tokio::test]
    async fn a_non_success_status_is_reported() {
        let (url, server) = serve_once(b"nope".to_vec(), "HTTP/1.1 404 Not Found").await;
        let dir = tempfile::tempdir().expect("tempdir");
        let destination = dir.path().join("archive.zip");
        crate::install_crypto_provider();
        let http = reqwest::Client::new();

        let err = download_to_file(&http, &url, &destination, DownloadLimits::default(), |_, _| {})
            .await
            .expect_err("must fail");
        assert!(err.to_string().contains("404"), "{err}");
        assert!(!destination.exists());
        server.abort();
    }

    #[tokio::test]
    async fn a_stalled_transfer_is_abandoned_and_leaves_no_file() {
        // A server that sends headers promising more than it delivers, then
        // keeps the connection open without sending anything.
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let server = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut scratch = [0u8; 2048];
                let _ = socket.read(&mut scratch).await;
                let _ = socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1024\r\n\r\nstart")
                    .await;
                let _ = socket.flush().await;
                // Hold the connection open without sending the rest.
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            }
        });

        let dir = tempfile::tempdir().expect("tempdir");
        let destination = dir.path().join("archive.zip");
        crate::install_crypto_provider();
        let http = reqwest::Client::new();
        let limits = DownloadLimits {
            stall_timeout: std::time::Duration::from_millis(300),
            ..Default::default()
        };

        let err = download_to_file(
            &http,
            &format!("http://{addr}/a.zip"),
            &destination,
            limits,
            |_, _| {},
        )
        .await
        .expect_err("must stall");
        assert!(err.to_string().contains("stalled"), "{err}");
        assert!(
            !destination.exists(),
            "a stalled download must leave no partial file"
        );
        server.abort();
    }
}
