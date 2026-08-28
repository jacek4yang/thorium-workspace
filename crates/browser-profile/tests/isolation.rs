//! Profile isolation, driven against a stand-in browser.
//!
//! A real Thorium build cannot be downloaded inside a test, so these tests use a
//! stand-in executable that behaves like the parts of Chromium this crate
//! depends on: it accepts `--user-data-dir`, writes a `DevToolsActivePort` file
//! and stays alive until it is stopped.
//!
//! The Unix stand-in is a shell script, so these tests run on the development
//! and CI hosts used for the platform-independent crates. On Windows the same
//! guarantees are covered by `job_cleanup.rs`.

#![cfg(unix)]

use std::path::{Path, PathBuf};

use tw_browser_profile::{BrowserSession, ProfileError, ProfileLayout, ProfileLock};
use tw_domain::{BrowserProfile, LocaleTag, ProfileId, ThoriumSelection, TimeZoneId, Timestamp};

/// Writes a stand-in browser.
///
/// It behaves like the parts of Chromium this crate relies on: it honours
/// `--user-data-dir`, and it publishes a `DevToolsActivePort` file **only** when
/// `--remote-debugging-port` was passed, exactly as Chromium does.
fn fake_browser(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join("thorium.exe");
    let script = r#"#!/bin/sh
UDD=""
DEBUG=""
for arg in "$@"; do
  case "$arg" in
    --user-data-dir=*) UDD="${arg#--user-data-dir=}" ;;
    --remote-debugging-port=*) DEBUG="yes" ;;
  esac
done
[ -n "$UDD" ] || exit 64
mkdir -p "$UDD"
printf 'launched
' >> "$UDD/launches.log"
if [ -n "$DEBUG" ]; then
  # Bind an ephemeral TCP port so the number published is a real one, then
  # write the two-line file Chromium writes.
  PORT=$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1]); s.close()')
  printf '%s
/devtools/browser/00000000-0000-4000-8000-000000000000
' "$PORT" > "$UDD/DevToolsActivePort"
fi
# Stay alive like a browser would.
sleep 300
"#
    .to_owned();
    std::fs::write(&path, script).expect("write stand-in browser");
    let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).expect("chmod");
    path
}

/// Waits for the stand-in browser to record `expected` launches against a
/// directory. The browser writes its log after the launch call returns, so
/// reading immediately would race it.
async fn wait_for_launches(user_data_dir: &Path, expected: usize) -> String {
    let log = user_data_dir.join("launches.log");
    for _ in 0..200 {
        if let Ok(contents) = std::fs::read_to_string(&log)
            && contents.lines().count() >= expected
        {
            return contents;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!("the stand-in browser did not record {expected} launch(es) against {user_data_dir:?}");
}

fn profile(name: &str, locale: &str, timezone: &str) -> BrowserProfile {
    BrowserProfile {
        id: ProfileId::new(),
        name: name.to_owned(),
        thorium: ThoriumSelection::Current,
        startup_urls: Vec::new(),
        locale: LocaleTag::parse(locale).expect("locale"),
        timezone: TimeZoneId::parse(timezone).expect("timezone"),
        account_ids: Vec::new(),
        notes: String::new(),
        network_route_id: None,
        created_at: Timestamp::now(),
        updated_at: Timestamp::now(),
    }
}

#[tokio::test]
async fn two_profiles_run_concurrently_with_independent_user_data_directories() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let browser = fake_browser(workspace.path());

    let first = profile("First", "en-US", "UTC");
    let second = profile("Second", "en-US", "UTC");

    let session_a = BrowserSession::launch(&browser, workspace.path(), &first, "M152")
        .await
        .expect("launch first");
    let session_b = BrowserSession::launch(&browser, workspace.path(), &second, "M152")
        .await
        .expect("launch second");

    // Distinct directories, both actually written to by their own process.
    assert_ne!(session_a.user_data_dir(), session_b.user_data_dir());
    assert!(
        session_a
            .user_data_dir()
            .starts_with(workspace.path().join("profiles"))
    );

    for session in [&session_a, &session_b] {
        assert_eq!(wait_for_launches(&session.user_data_dir(), 1).await, "launched\n");
    }

    assert!(session_a.is_alive());
    assert!(session_b.is_alive());
    assert_ne!(session_a.pid(), session_b.pid());

    session_a.stop().await.expect("stop first");
    session_b.stop().await.expect("stop second");
}

#[tokio::test]
async fn a_second_launch_of_the_same_profile_is_refused_rather_than_conflicting() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let browser = fake_browser(workspace.path());
    let p = profile("Work", "en-US", "UTC");

    let session = BrowserSession::launch(&browser, workspace.path(), &p, "M152")
        .await
        .expect("launch");

    match BrowserSession::launch(&browser, workspace.path(), &p, "M152").await {
        Err(ProfileError::AlreadyRunning { holder }) => {
            let holder = holder.expect("the holder is recorded");
            assert_eq!(holder.manager_pid, std::process::id());
            assert_eq!(holder.browser_pid, Some(session.pid()));
        }
        Ok(_) => panic!("a second browser must never run against the same User Data directory"),
        Err(other) => panic!("expected AlreadyRunning, got {other:?}"),
    }

    // Exactly one launch happened against that directory.
    assert_eq!(wait_for_launches(&session.user_data_dir(), 1).await, "launched\n");

    session.stop().await.expect("stop");
}

#[tokio::test]
async fn stopping_a_session_releases_the_profile_for_relaunch() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let browser = fake_browser(workspace.path());
    let p = profile("Work", "en-US", "UTC");
    let layout = ProfileLayout::new(workspace.path(), &p);

    let session = BrowserSession::launch(&browser, workspace.path(), &p, "M152")
        .await
        .expect("launch");
    let first_pid = session.pid();
    assert!(ProfileLock::is_locked(&layout.profile_dir));
    // Wait until the browser has actually started before stopping it: stopping
    // within milliseconds of launch kills the forked child before it execs,
    // which is not what this test is about.
    wait_for_launches(&session.user_data_dir(), 1).await;

    session.stop().await.expect("stop");
    assert!(
        !ProfileLock::is_locked(&layout.profile_dir),
        "stopping must release the profile"
    );
    assert!(
        !tw_windows_platform::process_is_running(first_pid),
        "the browser process must be gone"
    );

    let again = BrowserSession::launch(&browser, workspace.path(), &p, "M152")
        .await
        .expect("relaunch");
    assert_ne!(again.pid(), first_pid);
    assert_eq!(
        wait_for_launches(&again.user_data_dir(), 2).await.lines().count(),
        2,
        "the same directory was reused, which is the point of a persistent profile"
    );
    again.stop().await.expect("stop");
}

#[tokio::test]
async fn a_profile_with_overrides_opens_a_loopback_control_channel() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let browser = fake_browser(workspace.path());
    let p = profile("Warsaw", "pl-PL", "Europe/Warsaw");

    let session = BrowserSession::launch(&browser, workspace.path(), &p, "M152")
        .await
        .expect("launch");
    wait_for_launches(&session.user_data_dir(), 1).await;
    let state = session.state();

    // The stand-in publishes a port but speaks no DevTools protocol, so the
    // endpoint is discovered even though the WebSocket handshake will not
    // complete. What matters here is that the port came from the profile's own
    // directory and is a real, loopback-only port.
    assert!(
        state.cdp_port.is_some(),
        "the DevTools endpoint should have been discovered"
    );
    assert!(state.cdp_port.unwrap_or(0) > 1024);
    assert!(session.user_data_dir().join("DevToolsActivePort").is_file());
    assert_eq!(state.thorium_version.as_deref(), Some("M152"));

    session.stop().await.expect("stop");
}

#[tokio::test]
async fn a_default_profile_opens_no_control_channel_at_all() {
    let workspace = tempfile::tempdir().expect("tempdir");
    let browser = fake_browser(workspace.path());
    let p = profile("Plain", "en-US", "UTC");

    let session = BrowserSession::launch(&browser, workspace.path(), &p, "M152")
        .await
        .expect("launch");
    wait_for_launches(&session.user_data_dir(), 1).await;
    assert_eq!(
        session.state().cdp_port,
        None,
        "no overrides means no debugging port is requested"
    );
    // The stand-in publishes a port file only when --remote-debugging-port was
    // passed, so its absence proves no debugging endpoint was ever opened.
    assert!(
        !session.user_data_dir().join("DevToolsActivePort").is_file(),
        "a profile with no overrides must not open a DevTools endpoint"
    );
    session.stop().await.expect("stop");
}
