//! Browser profile launching, isolation, and process supervision.

#![forbid(unsafe_code)]

mod error;
pub mod launch;
mod lock;
mod mutex_name;
mod session;

pub use error::ProfileError;
pub use launch::{ALLOWED_EXTRA_ARGUMENTS, LaunchSpec, prepare_user_data_dir};
pub use mutex_name::{profile_mutex_name, profile_mutex_name_in_workspace};
pub use session::Session;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
    use thorium_workspace_domain::{DiagnosticCode as _, ProfileId};

    /// cmd.exe is present on every Windows machine and can act as a
    /// long-running "browser stand-in" for supervision tests. Resolved
    /// to an absolute path because launch targets must be real files.
    fn fake_browser() -> std::path::PathBuf {
        std::env::var_os("ComSpec")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("C:\\Windows\\System32\\cmd.exe"))
    }

    fn long_running_arguments() -> Vec<String> {
        vec!["/c".to_owned(), "ping -n 30 127.0.0.1 > nul".to_owned()]
    }

    fn temp_user_data_dir(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tw-session-test-{tag}-{}", std::process::id()));
        launch::prepare_user_data_dir(&dir).expect("prepare");
        dir
    }

    #[test]
    fn session_launches_and_shuts_down() {
        let dir = temp_user_data_dir("lifecycle");
        let mut session = Session::launch(
            ProfileId::new(),
            &fake_browser(),
            &long_running_arguments(),
            &dir,
        )
        .expect("launch");
        assert!(session.is_running());
        assert_eq!(session.user_data_dir(), &dir);
        session.shutdown().expect("shutdown");
    }

    #[test]
    fn double_launch_is_blocked_while_running() {
        let dir = temp_user_data_dir("double");
        let profile = ProfileId::new();
        let session = Session::launch(profile, &fake_browser(), &long_running_arguments(), &dir)
            .expect("first launch");
        let error = Session::launch(profile, &fake_browser(), &long_running_arguments(), &dir)
            .expect_err("second launch must fail");
        assert_eq!(error.diagnostic_code(), "PROFILE_ALREADY_RUNNING");
        session.shutdown().expect("shutdown");
    }

    #[test]
    fn same_lock_is_released_after_shutdown() {
        // After shutdown, a new launch of the same profile succeeds.
        let dir = temp_user_data_dir("relock");
        let profile = ProfileId::new();
        let session = Session::launch(profile, &fake_browser(), &long_running_arguments(), &dir)
            .expect("first launch");
        session.shutdown().expect("shutdown");
        let mut second = Session::launch(profile, &fake_browser(), &long_running_arguments(), &dir)
            .expect("second launch");
        assert!(second.is_running());
        second.shutdown().expect("shutdown again");
    }

    #[test]
    fn drop_releases_lock_without_explicit_shutdown() {
        let dir = temp_user_data_dir("drop");
        let profile = ProfileId::new();
        {
            let session =
                Session::launch(profile, &fake_browser(), &long_running_arguments(), &dir)
                    .expect("launch");
            assert!(session.lock_name().starts_with("Local\\"));
        }
        let mut relaunched =
            Session::launch(profile, &fake_browser(), &long_running_arguments(), &dir)
                .expect("lock must be free after drop");
        assert!(relaunched.is_running());
        relaunched.shutdown().expect("shutdown");
    }

    #[test]
    fn missing_executable_is_reported() {
        let error = Session::launch(
            ProfileId::new(),
            Path::new("Z:/no/such/browser.exe"),
            &[],
            &temp_user_data_dir("missing"),
        )
        .expect_err("missing exe");
        assert_eq!(error.diagnostic_code(), "PROFILE_MISSING_EXECUTABLE");
    }

    #[test]
    fn missing_user_data_dir_is_reported() {
        let error = Session::launch(
            ProfileId::new(),
            &fake_browser(),
            &["/c".to_owned(), "exit 0".to_owned()],
            Path::new("Z:/definitely/not/here/User Data"),
        )
        .expect_err("missing dir");
        assert_eq!(error.diagnostic_code(), "PROFILE_USER_DATA_DIR_FAILED");
    }

    #[test]
    fn two_profiles_isolate_concurrently() {
        let dir_a = temp_user_data_dir("iso-a");
        let dir_b = temp_user_data_dir("iso-b");
        let mut a = Session::launch(
            ProfileId::new(),
            &fake_browser(),
            &long_running_arguments(),
            &dir_a,
        )
        .expect("a");
        let mut b = Session::launch(
            ProfileId::new(),
            &fake_browser(),
            &long_running_arguments(),
            &dir_b,
        )
        .expect("b");
        assert!(a.is_running());
        assert!(b.is_running());
        assert_ne!(a.user_data_dir(), b.user_data_dir());
        a.shutdown().expect("a shutdown");
        b.shutdown().expect("b shutdown");
    }
}
