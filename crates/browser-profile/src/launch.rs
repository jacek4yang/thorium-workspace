//! Building the Thorium command line.
//!
//! Every flag here is deliberate and documented. Nothing in this module
//! randomizes, spoofs or masks anything about the browser: the profile controls
//! its own data directory, language and startup pages, and that is all.

use std::path::{Path, PathBuf};

use tw_domain::BrowserProfile;

/// Where one profile's data lives inside the portable workspace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileLayout {
    /// The profile's own directory, named after its immutable id.
    pub profile_dir: PathBuf,
    /// The Chromium `User Data` directory inside it.
    pub user_data_dir: PathBuf,
}

impl ProfileLayout {
    /// Builds the layout for `profile` under `workspace_root`.
    ///
    /// The directory name comes from the profile id, never its name: renaming a
    /// profile must not move, merge or orphan browser state.
    #[must_use]
    pub fn new(workspace_root: &Path, profile: &BrowserProfile) -> Self {
        Self::for_dir_name(workspace_root, &profile.user_data_dir_name())
    }

    /// Builds the layout for a directory name already stored in the database.
    #[must_use]
    pub fn for_dir_name(workspace_root: &Path, dir_name: &str) -> Self {
        let profile_dir = workspace_root.join("profiles").join(dir_name);
        let user_data_dir = profile_dir.join("User Data");
        Self {
            profile_dir,
            user_data_dir,
        }
    }

    /// Creates the directories.
    ///
    /// # Errors
    ///
    /// Returns [`crate::ProfileError::UserData`] when a directory cannot be
    /// created.
    pub fn ensure(&self) -> crate::ProfileResult<()> {
        std::fs::create_dir_all(&self.user_data_dir).map_err(|e| {
            crate::ProfileError::UserData(format!(
                "{} could not be created: {e}",
                self.user_data_dir.display()
            ))
        })
    }

    /// The file Chromium writes its chosen DevTools port into.
    #[must_use]
    pub fn devtools_port_file(&self) -> PathBuf {
        self.user_data_dir.join("DevToolsActivePort")
    }
}

/// A fully resolved launch: the executable, its arguments and its environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    /// Absolute path to `thorium.exe`.
    pub executable: PathBuf,
    /// The command line, in order.
    pub args: Vec<String>,
    /// Environment variables set for the child.
    pub env: Vec<(String, String)>,
    /// The layout the profile will use.
    pub layout: ProfileLayout,
}

/// Builds the command line for one profile.
///
/// # Flags
///
/// * `--user-data-dir` is absolute and unique per profile. This is the
///   isolation boundary.
/// * `--lang` sets the browser's UI language. It does not by itself change what
///   `navigator.language` reports; the DevTools locale override does that.
/// * `--no-first-run` and `--no-default-browser-check` suppress dialogs a
///   managed profile should never show.
/// * `--remote-debugging-port=0` asks for an ephemeral port. Chromium binds
///   DevTools to loopback and writes the chosen port into `DevToolsActivePort`
///   in the profile's own directory. It is requested only when an override
///   actually needs applying.
/// * `--remote-debugging-address` is deliberately **not** passed: leaving it at
///   the default keeps the endpoint on 127.0.0.1, and passing it is the usual
///   way people accidentally expose DevTools to the LAN.
#[must_use]
pub fn build_launch_plan(
    executable: &Path,
    workspace_root: &Path,
    profile: &BrowserProfile,
    enable_devtools: bool,
) -> LaunchPlan {
    let layout = ProfileLayout::new(workspace_root, profile);
    let mut args = vec![
        format!("--user-data-dir={}", layout.user_data_dir.display()),
        format!("--lang={}", profile.locale.as_str()),
        "--no-first-run".to_owned(),
        "--no-default-browser-check".to_owned(),
    ];
    if enable_devtools {
        args.push("--remote-debugging-port=0".to_owned());
    }
    for url in &profile.startup_urls {
        args.push(url.clone());
    }

    // TZ is honoured by ICU on some platforms and ignored on others. It is set
    // as a best-effort hint; the DevTools override is what actually guarantees
    // the timezone inside pages, and the difference is documented for the user.
    let env = vec![
        ("TZ".to_owned(), profile.timezone.as_str().to_owned()),
        // Chromium reads this for its accept-language header and
        // navigator.languages default.
        ("LANGUAGE".to_owned(), profile.locale.as_str().to_owned()),
    ];

    LaunchPlan {
        executable: executable.to_path_buf(),
        args,
        env,
        layout,
    }
}

#[cfg(test)]
mod tests {
    use tw_domain::{BrowserProfile, LocaleTag, ProfileId, ThoriumSelection, TimeZoneId, Timestamp};

    use super::*;

    fn profile(urls: Vec<&str>) -> BrowserProfile {
        BrowserProfile {
            id: ProfileId::new(),
            name: "Work".to_owned(),
            thorium: ThoriumSelection::Current,
            startup_urls: urls.into_iter().map(str::to_owned).collect(),
            locale: LocaleTag::parse("pl-PL").expect("locale"),
            timezone: TimeZoneId::parse("Europe/Warsaw").expect("timezone"),
            account_ids: Vec::new(),
            notes: String::new(),
            network_route_id: None,
            created_at: Timestamp::now(),
            updated_at: Timestamp::now(),
        }
    }

    #[test]
    fn the_user_data_directory_is_absolute_unique_and_named_after_the_id() {
        let root = Path::new("/w");
        let a = profile(Vec::new());
        let b = profile(Vec::new());
        let plan_a = build_launch_plan(Path::new("/t/thorium.exe"), root, &a, false);
        let plan_b = build_launch_plan(Path::new("/t/thorium.exe"), root, &b, false);

        assert_ne!(plan_a.layout.user_data_dir, plan_b.layout.user_data_dir);
        assert!(plan_a.layout.user_data_dir.ends_with("User Data"));
        assert!(plan_a.layout.user_data_dir.starts_with(root.join("profiles")));
        assert!(
            plan_a
                .args
                .iter()
                .any(|arg| arg == &format!("--user-data-dir={}", plan_a.layout.user_data_dir.display()))
        );
    }

    #[test]
    fn renaming_a_profile_does_not_change_its_directory() {
        let root = Path::new("/w");
        let before = profile(Vec::new());
        let after = BrowserProfile {
            name: "Renamed".to_owned(),
            ..before.clone()
        };
        assert_eq!(
            build_launch_plan(Path::new("/t.exe"), root, &before, false).layout,
            build_launch_plan(Path::new("/t.exe"), root, &after, false).layout
        );
    }

    #[test]
    fn the_locale_and_startup_urls_reach_the_command_line() {
        let plan = build_launch_plan(
            Path::new("/t.exe"),
            Path::new("/w"),
            &profile(vec!["https://example.test/", "https://second.test/"]),
            false,
        );
        assert!(plan.args.contains(&"--lang=pl-PL".to_owned()));
        assert!(plan.args.contains(&"https://example.test/".to_owned()));
        assert!(plan.args.contains(&"https://second.test/".to_owned()));
        assert!(plan.args.contains(&"--no-first-run".to_owned()));
        assert!(plan.args.contains(&"--no-default-browser-check".to_owned()));
    }

    #[test]
    fn devtools_is_only_requested_when_asked_for_and_never_binds_beyond_loopback() {
        let without = build_launch_plan(Path::new("/t.exe"), Path::new("/w"), &profile(Vec::new()), false);
        assert!(!without.args.iter().any(|a| a.starts_with("--remote-debugging")));

        let with = build_launch_plan(Path::new("/t.exe"), Path::new("/w"), &profile(Vec::new()), true);
        assert!(
            with.args.contains(&"--remote-debugging-port=0".to_owned()),
            "the port must be ephemeral"
        );
        assert!(
            !with
                .args
                .iter()
                .any(|a| a.starts_with("--remote-debugging-address")),
            "overriding the bind address is how DevTools gets exposed to the LAN"
        );
    }

    #[test]
    fn the_timezone_is_passed_to_the_child_environment() {
        let plan = build_launch_plan(Path::new("/t.exe"), Path::new("/w"), &profile(Vec::new()), true);
        assert!(plan.env.contains(&("TZ".to_owned(), "Europe/Warsaw".to_owned())));
        assert!(plan.env.contains(&("LANGUAGE".to_owned(), "pl-PL".to_owned())));
    }

    #[test]
    fn no_fingerprint_or_automation_flags_are_ever_added() {
        // v1.0.0 must not spoof or automate anything. This test is the guard
        // that keeps that true as the flag list grows.
        let plan = build_launch_plan(
            Path::new("/t.exe"),
            Path::new("/w"),
            &profile(vec!["https://example.test/"]),
            true,
        );
        let forbidden = [
            "--user-agent",
            "--proxy-server",
            "--proxy-pac-url",
            "--host-resolver-rules",
            "--disable-web-security",
            "--ignore-certificate-errors",
            "--disable-blink-features",
            "--enable-automation",
            "--headless",
            "--disable-gpu",
            "--no-sandbox",
        ];
        for arg in &plan.args {
            for bad in forbidden {
                assert!(!arg.starts_with(bad), "{arg} must never be passed in v1.0.0");
            }
        }
    }

    #[test]
    fn the_devtools_port_file_lives_inside_the_profile() {
        let layout = ProfileLayout::for_dir_name(Path::new("/w"), "abc");
        assert_eq!(
            layout.devtools_port_file(),
            Path::new("/w/profiles/abc/User Data/DevToolsActivePort")
        );
    }

    #[test]
    fn the_layout_can_be_created_on_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let layout = ProfileLayout::for_dir_name(dir.path(), "abc");
        layout.ensure().expect("create");
        assert!(layout.user_data_dir.is_dir());
        assert!(layout.profile_dir.is_dir());
    }
}
