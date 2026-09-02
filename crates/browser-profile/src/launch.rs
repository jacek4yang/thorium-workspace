//! Launch argument construction.
//!
//! The launch line is intentionally minimal and explicit:
//!
//! - `--user-data-dir=<absolute path>` is always passed; isolation is the
//!   product invariant and is never delegated to defaults.
//! - `--no-first-run` and `--no-default-browser-check` suppress first-run
//!   wizardry that would otherwise write global state (default browser
//!   probing) outside the profile's own directory.
//! - Startup URLs are passed positionally.
//! - Locale, when configured, is passed as `--lang=<BCP-47 tag>`.
//!
//! Timezone and content-locale emulation is applied after launch through
//! the DevTools protocol (see `emulation.rs`); no deprecated command-line
//! timezone flags are used.
//!
//! This module deliberately does not support arbitrary arguments: an
//! uncontrolled argument surface would let a profile author disable
//! security features (e.g. `--disable-web-security`). Advanced options
//! must be added to the explicit allowlist below with a documented
//! rationale.

#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use thorium_workspace_domain::LocaleTag;

/// Arguments allowed in extra_flags, each with a documented rationale.
/// Anything not on this list is rejected at construction time.
pub const ALLOWED_EXTRA_ARGUMENTS: &[(&str, &str)] = &[(
    "--new-window",
    "open the first startup URL in a fresh window instead of reusing an existing browser process window",
)];

/// Everything needed to launch one Browser Profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchSpec {
    /// Absolute user data directory for this profile.
    pub user_data_dir: PathBuf,
    /// URLs to open at launch (validated upstream).
    pub startup_urls: Vec<String>,
    /// Optional UI/application language.
    pub locale: Option<LocaleTag>,
    /// Extra arguments; must be on the explicit allowlist.
    pub extra_arguments: Vec<String>,
}

impl LaunchSpec {
    /// Validates the spec and returns the canonical argument list.
    pub fn build_arguments(&self) -> Result<Vec<String>, crate::ProfileError> {
        if !self.user_data_dir.is_absolute() {
            return Err(crate::ProfileError::InvalidSpec {
                detail: "--user-data-dir must be an absolute path".to_owned(),
            });
        }
        for argument in &self.extra_arguments {
            if !ALLOWED_EXTRA_ARGUMENTS
                .iter()
                .any(|(allowed, _)| allowed == &argument.as_str())
            {
                return Err(crate::ProfileError::DisallowedArgument {
                    argument: argument.clone(),
                });
            }
        }
        let mut arguments = vec![
            format!("--user-data-dir={}", self.user_data_dir.display()),
            "--no-first-run".to_owned(),
            "--no-default-browser-check".to_owned(),
        ];
        if let Some(locale) = &self.locale {
            arguments.push(format!("--lang={}", locale.as_str()));
        }
        arguments.extend(self.extra_arguments.iter().cloned());
        arguments.extend(self.startup_urls.iter().cloned());
        Ok(arguments)
    }
}

/// Resolves (and creates) a profile's user data directory.
pub fn prepare_user_data_dir(path: &Path) -> Result<PathBuf, crate::ProfileError> {
    std::fs::create_dir_all(path).map_err(|source| crate::ProfileError::UserDataDir { source })?;
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use thorium_workspace_domain::DiagnosticCode as _;

    fn spec() -> LaunchSpec {
        LaunchSpec {
            user_data_dir: std::env::temp_dir().join("tw-launch-test\\abc\\User Data"),
            startup_urls: vec!["https://github.com".to_owned()],
            locale: Some(LocaleTag::new("en-US").expect("valid")),
            extra_arguments: Vec::new(),
        }
    }

    #[test]
    fn arguments_carry_explicit_user_data_dir() {
        let arguments = spec().build_arguments().expect("valid");
        let expected = format!(
            "--user-data-dir={}",
            std::env::temp_dir()
                .join("tw-launch-test\\abc\\User Data")
                .display()
        );
        assert!(
            arguments.iter().any(|a| a == &expected),
            "missing isolation flag: {arguments:?}"
        );
        assert!(arguments.contains(&"--no-first-run".to_owned()));
        assert!(arguments.contains(&"--no-default-browser-check".to_owned()));
        assert!(arguments.contains(&"--lang=en-US".to_owned()));
        assert!(arguments.contains(&"https://github.com".to_owned()));
    }

    #[test]
    fn locale_is_omitted_when_not_configured() {
        let mut spec = spec();
        spec.locale = None;
        let arguments = spec.build_arguments().expect("valid");
        assert!(!arguments.iter().any(|a| a.starts_with("--lang")));
    }

    #[test]
    fn relative_user_data_dirs_are_rejected() {
        let mut spec = spec();
        spec.user_data_dir = Path::new("relative/path").to_path_buf();
        let error = spec.build_arguments().expect_err("relative path");
        assert_eq!(error.diagnostic_code(), "PROFILE_INVALID_SPEC");
    }

    #[test]
    fn dangerous_extra_arguments_are_rejected() {
        let mut spec = spec();
        spec.extra_arguments = vec!["--disable-web-security".to_owned()];
        let error = spec.build_arguments().expect_err("dangerous argument");
        let rendered = format!("{error}");
        assert!(rendered.contains("disable-web-security"));
        assert_eq!(error.diagnostic_code(), "PROFILE_DISALLOWED_ARGUMENT");
    }

    #[test]
    fn allowlisted_arguments_are_accepted() {
        let mut spec = spec();
        spec.extra_arguments = vec!["--new-window".to_owned()];
        let arguments = spec.build_arguments().expect("valid");
        assert!(arguments.contains(&"--new-window".to_owned()));
    }
}
