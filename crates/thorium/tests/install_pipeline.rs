//! End-to-end install pipeline against a local fixture server.
//!
//! Builds a synthetic Thorium archive, serves it and a synthetic GitHub release
//! listing from `127.0.0.1`, and drives the real manager through discovery,
//! download, verification, extraction, validation and promotion. No external
//! network access and no real Thorium binary are involved.

use std::io::Write;
use std::path::Path;

use tw_domain::ThoriumChannel;
use tw_thorium::{InstallProgress, InstallRequest, ReleaseClient, ReleaseClientConfig, ThoriumManager};

/// Builds a ZIP shaped like an upstream portable Thorium archive.
fn synthetic_archive(version: &str) -> Vec<u8> {
    let mut buffer = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut buffer);
        let options: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let root = format!("THORIUM_{version}");
        writer
            .start_file(format!("{root}/README.txt"), options)
            .expect("start");
        writer
            .write_all(b"Portable Thorium. Synthetic fixture, not a real browser.")
            .expect("write");
        writer
            .start_file(format!("{root}/BIN/thorium.exe"), options)
            .expect("start");
        // Large enough to pass the "this is implausibly small" validation.
        writer.write_all(&vec![0x4du8; 400 * 1024]).expect("write");
        writer
            .start_file(format!("{root}/BIN/{version}/resources.pak"), options)
            .expect("start");
        writer.write_all(b"resources").expect("write");
        writer.finish().expect("finish");
    }
    buffer.into_inner()
}

/// A local HTTP server that answers the release listing and the asset download.
struct Fixture {
    base: String,
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn start_fixture(version: &'static str, archive: Vec<u8>) -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let base = format!("http://{addr}");
    let listing_base = base.clone();

    let handle = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let archive = archive.clone();
            let listing_base = listing_base.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut scratch = vec![0u8; 4096];
                let Ok(read) = socket.read(&mut scratch).await else {
                    return;
                };
                let request = String::from_utf8_lossy(&scratch[..read]).to_string();

                let (content_type, body): (&str, Vec<u8>) = if request.contains("/releases?") {
                    let listing = format!(
                        r#"[{{
                            "tag_name": "{version}",
                            "name": "Thorium {version}",
                            "prerelease": false,
                            "draft": false,
                            "published_at": "2026-07-14T09:31:05Z",
                            "html_url": "{listing_base}/release",
                            "assets": [
                                {{"name": "thorium_avx2_mini_installer.exe",
                                  "browser_download_url": "{listing_base}/installer.exe",
                                  "size": 125829120}},
                                {{"name": "Thorium_AVX2_{version}.zip",
                                  "browser_download_url": "{listing_base}/archive.zip",
                                  "size": {size}}}
                            ]
                        }}]"#,
                        size = 180 * 1024 * 1024
                    );
                    ("application/json", listing.into_bytes())
                } else if request.contains("/archive.zip") {
                    ("application/zip", archive)
                } else {
                    ("text/plain", b"not found".to_vec())
                };

                let header = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(header.as_bytes()).await;
                let _ = socket.write_all(&body).await;
                let _ = socket.flush().await;
            });
        }
    });

    Fixture { base, handle }
}

fn manager_for(root: &Path, base: &str) -> ThoriumManager {
    // Any consumer that builds its own reqwest client must install the crypto
    // provider first; the manager's own constructors do this for themselves.
    tw_thorium::install_crypto_provider();
    let http = reqwest::Client::builder().build().expect("http client");
    let config = ReleaseClientConfig {
        api_base: base.to_owned(),
        ..Default::default()
    };
    let releases = ReleaseClient::with_http(config, http.clone());
    ThoriumManager::with_clients(root, releases, http).expect("manager")
}

#[tokio::test]
async fn a_release_is_discovered_downloaded_extracted_and_promoted() {
    const VERSION: &str = "M152.0.7977.55";
    let fixture = start_fixture(VERSION, synthetic_archive(VERSION)).await;
    let workspace = tempfile::tempdir().expect("tempdir");
    let manager = manager_for(workspace.path(), &fixture.base);

    let mut stages = Vec::new();
    let installation = manager
        .install(&InstallRequest::latest(ThoriumChannel::WindowsAvx2), |progress| {
            let label = match progress {
                InstallProgress::Resolving => "resolving",
                InstallProgress::Downloading { .. } => "downloading",
                InstallProgress::Verifying => "verifying",
                InstallProgress::Extracting { .. } => "extracting",
                InstallProgress::Activating => "activating",
                InstallProgress::Done { .. } => "done",
            };
            if stages.last().map(String::as_str) != Some(label) {
                stages.push(label.to_owned());
            }
        })
        .await
        .expect("install");

    assert_eq!(installation.version, VERSION);
    assert!(installation.is_current);
    assert_eq!(installation.archive_sha256.len(), 64);
    assert!(
        installation.source_url.ends_with("/archive.zip"),
        "the installer .exe must not be chosen"
    );

    // The pipeline ran in order and reported every stage.
    assert_eq!(
        stages,
        vec![
            "resolving",
            "downloading",
            "verifying",
            "extracting",
            "activating",
            "done"
        ]
    );

    // The installation is on disk, selected, and its executable resolves.
    assert_eq!(manager.installed_versions(), vec![VERSION.to_owned()]);
    assert_eq!(manager.current_version().as_deref(), Some(VERSION));
    let executable = manager.current_executable().expect("executable");
    assert!(executable.is_file());
    assert!(executable.ends_with("thorium.exe"));
    assert_eq!(executable.to_string_lossy(), installation.executable_path);

    // Staging is left clean: no archive and no partial directory survive.
    let staged: Vec<_> = std::fs::read_dir(manager.paths().staging_dir())
        .expect("read staging")
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert!(
        staged.is_empty(),
        "staging must be empty after a successful install: {staged:?}"
    );
}

#[tokio::test]
async fn a_digest_mismatch_aborts_the_install_and_leaves_nothing_behind() {
    const VERSION: &str = "M152.0.7977.55";
    let fixture = start_fixture(VERSION, synthetic_archive(VERSION)).await;
    let workspace = tempfile::tempdir().expect("tempdir");
    let manager = manager_for(workspace.path(), &fixture.base);

    let request = InstallRequest {
        expected_sha256: Some("ff".repeat(32)),
        ..InstallRequest::latest(ThoriumChannel::WindowsAvx2)
    };
    let error = manager.install(&request, |_| {}).await.expect_err("must refuse");
    assert!(
        matches!(error, tw_thorium::ThoriumError::DigestMismatch { .. }),
        "expected a digest mismatch, got {error:?}"
    );

    assert!(
        manager.installed_versions().is_empty(),
        "a rejected archive must not be installed"
    );
    assert_eq!(manager.current_version(), None);
    let staged: Vec<_> = std::fs::read_dir(manager.paths().staging_dir())
        .expect("read staging")
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert!(
        staged.is_empty(),
        "a failed install must leave no staged files: {staged:?}"
    );
}

#[tokio::test]
async fn installing_a_second_version_keeps_the_first_and_supports_rollback() {
    let workspace = tempfile::tempdir().expect("tempdir");

    {
        let fixture = start_fixture("M151.0.7922.72", synthetic_archive("M151.0.7922.72")).await;
        let manager = manager_for(workspace.path(), &fixture.base);
        manager
            .install(&InstallRequest::latest(ThoriumChannel::WindowsAvx2), |_| {})
            .await
            .expect("install first");
    }
    {
        let fixture = start_fixture("M152.0.7977.55", synthetic_archive("M152.0.7977.55")).await;
        let manager = manager_for(workspace.path(), &fixture.base);
        manager
            .install(&InstallRequest::latest(ThoriumChannel::WindowsAvx2), |_| {})
            .await
            .expect("install second");

        assert_eq!(
            manager.installed_versions(),
            vec!["M151.0.7922.72".to_owned(), "M152.0.7977.55".to_owned()],
            "an update must not delete the previous known-good version"
        );
        assert_eq!(manager.current_version().as_deref(), Some("M152.0.7977.55"));

        // Rolling back selects the older version and leaves both installed.
        assert_eq!(manager.rollback().expect("rollback"), "M151.0.7922.72");
        assert_eq!(manager.current_version().as_deref(), Some("M151.0.7922.72"));
        assert!(manager.current_executable().expect("executable").is_file());
        assert_eq!(manager.installed_versions().len(), 2);
    }
}

#[tokio::test]
async fn reinstalling_the_same_version_replaces_it_in_place() {
    const VERSION: &str = "M152.0.7977.55";
    let workspace = tempfile::tempdir().expect("tempdir");
    let fixture = start_fixture(VERSION, synthetic_archive(VERSION)).await;
    let manager = manager_for(workspace.path(), &fixture.base);

    manager
        .install(&InstallRequest::latest(ThoriumChannel::WindowsAvx2), |_| {})
        .await
        .expect("first");
    // Leave a marker that the reinstall must remove.
    let stray = manager.paths().version_dir(VERSION).join("stray.txt");
    std::fs::write(&stray, b"left over").expect("write");

    manager
        .install(&InstallRequest::latest(ThoriumChannel::WindowsAvx2), |_| {})
        .await
        .expect("second");
    assert_eq!(manager.installed_versions(), vec![VERSION.to_owned()]);
    assert!(
        !stray.exists(),
        "a reinstall must replace the directory rather than merge into it"
    );
    assert!(manager.current_executable().expect("executable").is_file());
}
