//! Per-profile launch lock combining an in-process registry with a
//! cross-process named mutex.
//!
//! Two layers are required:
//!
//! - **In-process registry**: Windows mutexes are re-entrant for the
//!   owning thread, so the named mutex alone cannot detect a second
//!   launch from inside the same process. A `Mutex<HashSet<ProfileId>>`
//!   closes that hole.
//! - **Named mutex**: prevents two manager processes from running the
//!   same profile; an abandoned mutex (crashed owner) is claimed, so a
//!   manager restart never wedges the profile.
//!
//! Dropping [`ProfileLock`] releases both layers.

use std::collections::HashSet;
use std::sync::Mutex;
use std::sync::OnceLock;

use thorium_workspace_domain::ProfileId;
use thorium_workspace_windows_platform::error::PlatformError;
use thorium_workspace_windows_platform::mutex::NamedMutexGuard;

fn held_profiles() -> &'static Mutex<HashSet<ProfileId>> {
    static HELD: OnceLock<Mutex<HashSet<ProfileId>>> = OnceLock::new();
    HELD.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Lock ensuring one live session per profile id.
#[derive(Debug)]
pub struct ProfileLock {
    inner: NamedMutexGuard,
    profile: ProfileId,
}

impl ProfileLock {
    /// Tries to acquire the launch lock. `Ok(None)` means the profile is
    /// already running (in this process or in another process).
    pub fn try_acquire(profile: &ProfileId) -> Result<Option<Self>, PlatformError> {
        let held = held_profiles();
        let mut set = match held.lock() {
            Ok(set) => set,
            // A panicking holder must not wedge launching forever.
            Err(poisoned) => poisoned.into_inner(),
        };
        if !set.insert(*profile) {
            return Ok(None);
        }
        drop(set);

        let name = super::mutex_name::profile_mutex_name(profile)?;
        let inner = thorium_workspace_windows_platform::mutex::try_acquire_named_mutex(&name)?;
        match inner {
            Some(inner) => Ok(Some(Self {
                inner,
                profile: *profile,
            })),
            None => {
                // Another process holds it; release the registry slot.
                if let Ok(mut set) = held.lock() {
                    set.remove(profile);
                }
                Ok(None)
            }
        }
    }

    /// The held mutex name (diagnostics).
    pub fn name(&self) -> &str {
        self.inner.name()
    }
}

impl Drop for ProfileLock {
    fn drop(&mut self) {
        // Closing the mutex handle happens in `inner`'s drop; the
        // registry slot must be released in both poisoned and healthy
        // cases.
        let held = held_profiles();
        if let Ok(mut set) = held.lock() {
            set.remove(&self.profile);
        } else {
            set_drop_poisoned(&self.profile);
        }
    }
}

fn set_drop_poisoned(profile: &ProfileId) {
    if let Err(poisoned) = held_profiles().lock() {
        poisoned.into_inner().remove(profile);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_process_double_acquire_is_rejected() {
        let profile = ProfileId::new();
        let first = ProfileLock::try_acquire(&profile)
            .expect("first")
            .expect("held");
        let second = ProfileLock::try_acquire(&profile).expect("second");
        assert!(second.is_none(), "in-process registry must reject");
        drop(first);
        let third = ProfileLock::try_acquire(&profile)
            .expect("third")
            .expect("held after release");
        drop(third);
    }

    #[test]
    fn different_profiles_do_not_contend() {
        let a = ProfileLock::try_acquire(&ProfileId::new())
            .expect("a")
            .expect("held");
        let b = ProfileLock::try_acquire(&ProfileId::new())
            .expect("b")
            .expect("held");
        assert_ne!(a.name(), b.name());
        drop(a);
        drop(b);
    }
}
