//! One running browser session.
//!
//! A session owns, for as long as it lives: the per-profile lock, the Job Object
//! holding the browser process tree, the browser process itself and the DevTools
//! control task. Stopping it releases all four in that order.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tw_domain::{BrowserProfile, ProfileRuntimeStatus};
use tw_windows_platform::{ChildProcess, ProcessGroup, SpawnOptions};

use crate::cdp::{CdpEndpoint, EmulationSettings, apply_emulation, read_devtools_endpoint};
use crate::launch::{ProfileLayout, build_launch_plan};
use crate::lock::ProfileLock;
use crate::{ProfileError, ProfileResult};

/// How long to wait for Chromium to publish its DevTools endpoint.
const DEVTOOLS_TIMEOUT: Duration = Duration::from_secs(20);

/// What the UI is told about a running session.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SessionState {
    /// Observed status.
    pub status: ProfileRuntimeStatus,
    /// The browser process id.
    pub pid: Option<u32>,
    /// The loopback DevTools port, when one was opened.
    pub cdp_port: Option<u16>,
    /// Which Thorium version is running.
    pub thorium_version: Option<String>,
    /// Whether the timezone and locale overrides are active.
    pub emulation_active: bool,
    /// Whether the process tree is supervised by a Job Object.
    pub supervised: bool,
}

/// A handle the caller keeps while a profile runs.
#[derive(Debug)]
pub struct SessionHandle {
    /// The profile's on-disk layout.
    pub layout: ProfileLayout,
    /// Current state.
    pub state: SessionState,
}

/// A live browser session.
///
/// Dropping a `BrowserSession` closes the Job Object handle, which terminates
/// the browser process tree. That is deliberate: the browser must not outlive
/// the manager that is supervising it.
pub struct BrowserSession {
    layout: ProfileLayout,
    lock: ProfileLock,
    group: ProcessGroup,
    child: ChildProcess,
    cdp: Option<CdpEndpoint>,
    emulation_shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    emulation_task: Option<tokio::task::JoinHandle<()>>,
    thorium_version: String,
    status: ProfileRuntimeStatus,
}

impl std::fmt::Debug for BrowserSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BrowserSession")
            .field("user_data_dir", &self.layout.user_data_dir)
            .field("pid", &self.child.pid())
            .field("cdp_port", &self.cdp.as_ref().map(|e| e.port))
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl BrowserSession {
    /// Launches `profile` using `executable`.
    ///
    /// Applies the timezone and locale overrides through DevTools when the
    /// profile configures anything other than the defaults.
    ///
    /// # Errors
    ///
    /// Returns [`ProfileError::AlreadyRunning`] when the profile is already in
    /// use, and the corresponding variant for a failure to prepare the data
    /// directory, take the lock or start the browser.
    pub async fn launch(
        executable: &Path,
        workspace_root: &Path,
        profile: &BrowserProfile,
        thorium_version: &str,
    ) -> ProfileResult<Self> {
        if !executable.is_file() {
            return Err(ProfileError::NoBrowser);
        }

        // Overrides are only worth a control channel when they actually differ
        // from what the browser would do anyway.
        let needs_emulation = profile.timezone != tw_domain::TimeZoneId::utc()
            || profile.locale != tw_domain::LocaleTag::default();

        let plan = build_launch_plan(executable, workspace_root, profile, needs_emulation);
        plan.layout.ensure()?;

        // Taking the lock before touching anything else is what makes a second
        // launch against the same directory impossible.
        let mut lock = ProfileLock::acquire(&plan.layout.profile_dir)?;

        // A port file left by a previous run would otherwise be adopted as this
        // run's endpoint.
        let port_file = plan.layout.devtools_port_file();
        let _ = std::fs::remove_file(&port_file);

        let group = ProcessGroup::new()
            .map_err(|e| ProfileError::Launch(format!("process supervision could not be set up: {e}")))?;

        let mut options = SpawnOptions::new(&plan.executable).args(plan.args.clone());
        for (key, value) in &plan.env {
            options = options.env(key, value);
        }
        let child = tw_windows_platform::spawn(&options).map_err(|e| ProfileError::Launch(e.to_string()))?;

        // Assign before the browser has a chance to spawn its children, so the
        // whole tree ends up inside the job.
        if let Err(error) = group.assign(child.pid()) {
            tracing::warn!(error = %error, "the browser could not be placed under process supervision");
        }
        lock.record_browser_pid(child.pid())?;

        let mut session = Self {
            layout: plan.layout,
            lock,
            group,
            child,
            cdp: None,
            emulation_shutdown: None,
            emulation_task: None,
            thorium_version: thorium_version.to_owned(),
            status: ProfileRuntimeStatus::Starting,
        };

        if needs_emulation {
            match read_devtools_endpoint(&port_file, DEVTOOLS_TIMEOUT).await {
                Ok(endpoint) => session.start_emulation(endpoint, profile),
                Err(error) => {
                    // A profile that cannot be emulated still runs. The user is
                    // told through the session state rather than being left
                    // with a browser that failed to open.
                    tracing::warn!(error = %error, "the DevTools control channel was not available");
                }
            }
        }

        session.status = ProfileRuntimeStatus::Running;
        Ok(session)
    }

    fn start_emulation(&mut self, endpoint: CdpEndpoint, profile: &BrowserProfile) {
        let settings = EmulationSettings {
            timezone: profile.timezone.as_str().to_owned(),
            locale: profile.locale.as_str().to_owned(),
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        let endpoint_for_task = endpoint.clone();
        let task = tokio::spawn(async move {
            if let Err(error) = apply_emulation(&endpoint_for_task, &settings, rx).await {
                tracing::warn!(error = %error, "the timezone and locale overrides stopped being applied");
            }
        });
        self.cdp = Some(endpoint);
        self.emulation_shutdown = Some(tx);
        self.emulation_task = Some(task);
    }

    /// The profile's on-disk layout.
    #[must_use]
    pub const fn layout(&self) -> &ProfileLayout {
        &self.layout
    }

    /// The browser process id.
    #[must_use]
    pub const fn pid(&self) -> u32 {
        self.child.pid()
    }

    /// The lock file backing this session.
    #[must_use]
    pub fn lock_path(&self) -> &Path {
        self.lock.path()
    }

    /// A snapshot for the UI and diagnostics.
    #[must_use]
    pub fn state(&self) -> SessionState {
        SessionState {
            status: self.status,
            pid: Some(self.child.pid()),
            cdp_port: self.cdp.as_ref().map(|e| e.port),
            thorium_version: Some(self.thorium_version.clone()),
            emulation_active: self.emulation_task.as_ref().is_some_and(|t| !t.is_finished()),
            supervised: ProcessGroup::is_supervising(),
        }
    }

    /// Whether the browser process is still alive.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        tw_windows_platform::process_is_running(self.child.pid())
    }

    /// Brings the browser's window to the front.
    ///
    /// Returns `false` when no window could be raised, so the caller can tell
    /// the user the profile is running rather than silently doing nothing.
    #[must_use]
    pub fn focus(&self) -> bool {
        tw_windows_platform::focus_window_of_process(self.child.pid()).unwrap_or(false)
    }

    /// Stops the browser and releases everything the session holds.
    ///
    /// # Errors
    ///
    /// Never fails: every step is best effort, because the caller's alternative
    /// to "stopped with a warning" is a profile that can never be launched
    /// again.
    pub async fn stop(mut self) -> ProfileResult<()> {
        self.status = ProfileRuntimeStatus::Stopping;

        // 1. Close the control channel first, so the browser is not being
        //    driven while it shuts down.
        if let Some(tx) = self.emulation_shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.emulation_task.take() {
            let _ = tokio::time::timeout(Duration::from_secs(2), task).await;
        }

        // 2. Ask the browser to exit.
        if let Err(error) = self.child.kill() {
            tracing::debug!(error = %error, "the browser had already exited");
        }

        // 3. Terminate anything still in the job: renderers, the GPU process,
        //    the network service.
        if let Err(error) = self.group.terminate() {
            tracing::warn!(error = %error, "the browser process tree could not be terminated");
        }

        // 4. Wait briefly for the process table to catch up, so a relaunch does
        //    not race a still-exiting browser against the same data directory.
        for _ in 0..40 {
            if !tw_windows_platform::process_is_running(self.child.pid()) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        // 5. Drop the lock last: the profile is only free once nothing is using
        //    its directory.
        self.status = ProfileRuntimeStatus::Stopped;
        Ok(())
    }

    /// The path of the `User Data` directory this session owns.
    #[must_use]
    pub fn user_data_dir(&self) -> PathBuf {
        self.layout.user_data_dir.clone()
    }
}

#[cfg(test)]
mod tests {
    use tw_domain::{LocaleTag, ProfileId, ThoriumSelection, TimeZoneId, Timestamp};

    use super::*;

    fn profile(locale: &str, timezone: &str) -> BrowserProfile {
        BrowserProfile {
            id: ProfileId::new(),
            name: "Work".to_owned(),
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
    async fn launching_without_an_installed_browser_is_reported_clearly() {
        let dir = tempfile::tempdir().expect("tempdir");
        let err = BrowserSession::launch(
            &dir.path().join("missing").join("thorium.exe"),
            dir.path(),
            &profile("en-US", "UTC"),
            "M152",
        )
        .await
        .expect_err("must fail");
        assert!(matches!(err, ProfileError::NoBrowser));
    }

    #[test]
    fn a_default_profile_needs_no_control_channel() {
        // A profile that overrides nothing must not open a DevTools port at all.
        let default_profile = profile("en-US", "UTC");
        assert_eq!(default_profile.timezone, TimeZoneId::utc());
        assert_eq!(default_profile.locale, LocaleTag::default());

        let plan =
            crate::launch::build_launch_plan(Path::new("/t.exe"), Path::new("/w"), &default_profile, false);
        assert!(!plan.args.iter().any(|a| a.starts_with("--remote-debugging")));
    }

    #[test]
    fn a_profile_with_overrides_asks_for_a_control_channel() {
        let overridden = profile("pl-PL", "Europe/Warsaw");
        assert_ne!(overridden.timezone, TimeZoneId::utc());
        let plan = crate::launch::build_launch_plan(Path::new("/t.exe"), Path::new("/w"), &overridden, true);
        assert!(plan.args.contains(&"--remote-debugging-port=0".to_owned()));
    }

    #[tokio::test]
    async fn two_sessions_cannot_share_one_profile_directory() {
        // Exercised through the lock directly: launching a real browser is not
        // possible in a unit test, and the lock is what enforces the invariant.
        let dir = tempfile::tempdir().expect("tempdir");
        let p = profile("en-US", "UTC");
        let layout = ProfileLayout::new(dir.path(), &p);
        layout.ensure().expect("create");

        let first = ProfileLock::acquire(&layout.profile_dir).expect("first");
        assert!(matches!(
            ProfileLock::acquire(&layout.profile_dir),
            Err(ProfileError::AlreadyRunning { .. })
        ));
        drop(first);
        let _second = ProfileLock::acquire(&layout.profile_dir).expect("after release");
    }

    #[test]
    fn session_state_reports_supervision_honestly() {
        let state = SessionState {
            status: ProfileRuntimeStatus::Running,
            pid: Some(1),
            cdp_port: Some(51234),
            thorium_version: Some("M152".to_owned()),
            emulation_active: true,
            supervised: ProcessGroup::is_supervising(),
        };
        assert_eq!(state.supervised, cfg!(windows));
        let json = serde_json::to_string(&state).expect("serialize");
        assert!(json.contains("\"cdp_port\":51234"));
    }
}
