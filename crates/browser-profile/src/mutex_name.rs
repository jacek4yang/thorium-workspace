//! Per-profile lock names.
//!
//! Each profile contends on its own Local-namespace mutex so double
//! launches of one profile fail while other profiles are unaffected.

use std::path::Path;

use thorium_workspace_domain::ProfileId;
use thorium_workspace_windows_platform::error::PlatformError;
use thorium_workspace_windows_platform::mutex_name::mutex_name_for;

/// Builds the mutex name for one profile, namespaced by the workspace
/// root so two different workspaces never contend on the same lock.
pub fn profile_mutex_name_in_workspace(
    workspace_root: &Path,
    profile_id: &ProfileId,
) -> Result<String, PlatformError> {
    let workspace_name = mutex_name_for(workspace_root)?;
    Ok(format!("{workspace_name}-profile-{profile_id}"))
}

/// Builds the profile mutex name when the workspace prefix is already
/// established by the caller (tests, single-workspace mode).
pub fn profile_mutex_name(profile_id: &ProfileId) -> Result<String, PlatformError> {
    Ok(format!("Local\\ThoriumWorkspace-profile-{profile_id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_embed_profile_id() {
        let id = ProfileId::new();
        let name = profile_mutex_name(&id).expect("name");
        assert!(name.starts_with("Local\\ThoriumWorkspace-profile-"));
        assert!(name.contains(&id.to_string()));
    }

    #[test]
    fn names_differ_per_profile_and_workspace() {
        let a = profile_mutex_name(&ProfileId::new()).expect("a");
        let b = profile_mutex_name(&ProfileId::new()).expect("b");
        assert_ne!(a, b);
        let c =
            profile_mutex_name_in_workspace(Path::new("D:\\Ws1"), &ProfileId::new()).expect("c");
        let d =
            profile_mutex_name_in_workspace(Path::new("D:\\Ws2"), &ProfileId::new()).expect("d");
        assert_ne!(c, d);
    }
}
