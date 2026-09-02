//! Workspace lifecycle: portable bootstrap, single-instance ownership,
//! storage, vault, and runtime recovery.
//!
//! Composition (everything reuses existing crates; no duplication):
//! - `windows-platform::paths` for exe-relative root + layout;
//! - `windows-platform::mutex` for cross-process single instance, plus an
//!   in-process registry (Win32 mutexes are per-thread re-entrant);
//! - `storage::Store` for `workspace.db`;
//! - `vault::Vault` for `vault/vault.bin`.
//!
//! Startup recovery removes only project-owned stale temporary files
//! (`workspace.db.tmp`, `vault.bin.tmp`, `current.tmp`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use thorium_workspace_domain::ProfileId;
use thorium_workspace_storage::Store;
use thorium_workspace_vault::Vault;
use thorium_workspace_windows_platform::mutex::NamedMutexGuard;
use thorium_workspace_windows_platform::paths;

use crate::clipboard::{ClipboardPort, SystemClipboard};
use crate::error::ControllerError;

/// In-process registry of open workspace roots (Win32 named mutexes are
/// re-entrant per thread, so the named mutex alone cannot detect a
/// second open inside this process).
fn open_workspaces() -> &'static Mutex<HashMap<u64, usize>> {
    static OPEN: OnceLock<Mutex<HashMap<u64, usize>>> = OnceLock::new();
    OPEN.get_or_init(|| Mutex::new(HashMap::new()))
}

fn root_key(root: &Path) -> u64 {
    // FNV-1a over the lowercased absolute path text.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in root.to_string_lossy().to_lowercase().as_bytes() {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The opened workspace: storage, vault, sessions, and scheduling state.
pub struct Workspace {
    root: PathBuf,
    _instance: NamedMutexGuard,
    _registry_slot: RegistrySlot,
    store: Mutex<Store>,
    vault: Mutex<Vault>,
    sessions: Mutex<HashMap<ProfileId, thorium_workspace_browser_profile::Session>>,
    clipboard: Arc<dyn ClipboardPort + Send + Sync>,
    clipboard_state: Mutex<crate::clipboard::ClipboardScheduler>,
    idle: Mutex<crate::idle::IdleTracker>,
}

#[derive(Debug)]
struct RegistrySlot {
    key: u64,
}

impl Drop for RegistrySlot {
    fn drop(&mut self) {
        if let Ok(mut open) = open_workspaces().lock() {
            if let Some(count) = open.get_mut(&self.key) {
                *count -= 1;
                if *count == 0 {
                    open.remove(&self.key);
                }
            }
        }
    }
}

/// File names under the workspace root.
pub const DB_FILE: &str = "workspace.db";
pub const VAULT_REL: &str = "vault/vault.bin";
pub const BROWSERS_REL: &str = "browsers";

impl Workspace {
    /// Boots (or re-opens) the workspace rooted at `root_override`
    /// (production passes `None` to use the executable directory).
    ///
    /// Fails with [`ControllerError::WorkspaceInUse`] when the workspace
    /// is already open — in this process or another.
    pub fn bootstrap(root_override: Option<&Path>) -> Result<Self, ControllerError> {
        let root = match root_override {
            Some(path) => {
                std::fs::create_dir_all(path).map_err(|source| {
                    thorium_workspace_windows_platform::PlatformError::Io {
                        path: path.to_path_buf(),
                        source,
                    }
                })?;
                paths::verify_writable(path)?;
                path.to_path_buf()
            }
            None => paths::workspace_root()?,
        };
        paths::initialize_layout(&root)?;

        // Cross-process single instance.
        let instance =
            thorium_workspace_windows_platform::mutex::try_acquire_workspace_mutex(&root)?
                .ok_or(ControllerError::WorkspaceInUse)?;

        // In-process single open (same-thread re-entrancy guard).
        let key = root_key(&root);
        {
            let mut open = open_workspaces()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if open.contains_key(&key) {
                return Err(ControllerError::WorkspaceInUse);
            }
            open.insert(key, 1);
        }
        let registry_slot = RegistrySlot { key };

        // Startup recovery: remove only project-owned stale temp files.
        for stale in [
            root.join(DB_FILE).with_extension("tmp"),
            root.join(VAULT_REL).with_extension("tmp"),
            root.join("browsers/thorium/current").with_extension("tmp"),
        ] {
            if stale.is_file() {
                let _ = std::fs::remove_file(&stale);
            }
        }

        let store = Store::open(&root.join(DB_FILE))?;
        let vault = Vault::open(&root.join(VAULT_REL))?;

        // Idle-lock threshold comes from persisted settings.
        let settings = store
            .load_settings()?
            .unwrap_or_else(thorium_workspace_domain::WorkspaceSettings::default);
        let idle = crate::idle::IdleTracker::new(
            settings
                .vault_idle_lock_minutes
                .map(|minutes| std::time::Duration::from_secs(60 * u64::from(minutes))),
        );
        let now = std::time::Instant::now();
        let mut idle = idle;
        idle.record_activity(now);

        Ok(Self {
            root,
            _instance: instance,
            _registry_slot: registry_slot,
            store: Mutex::new(store),
            vault: Mutex::new(vault),
            sessions: Mutex::new(HashMap::new()),
            clipboard: Arc::new(SystemClipboard),
            clipboard_state: Mutex::new(crate::clipboard::ClipboardScheduler::new()),
            idle: Mutex::new(idle),
        })
    }

    /// Workspace root path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Storage handle (locked; call-scoped).
    pub(crate) fn store(&self) -> std::sync::MutexGuard<'_, Store> {
        self.store
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Vault handle (locked; call-scoped).
    pub(crate) fn vault(&self) -> std::sync::MutexGuard<'_, Vault> {
        self.vault
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Running profile sessions (locked; call-scoped).
    pub(crate) fn sessions(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<ProfileId, thorium_workspace_browser_profile::Session>>
    {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Clipboard port (injected).
    pub(crate) fn clipboard(&self) -> Arc<dyn ClipboardPort + Send + Sync> {
        self.clipboard.clone()
    }

    /// Clipboard scheduler state (locked; call-scoped).
    pub(crate) fn clipboard_state(
        &self,
    ) -> std::sync::MutexGuard<'_, crate::clipboard::ClipboardScheduler> {
        self.clipboard_state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Idle tracker (locked; call-scoped).
    pub(crate) fn idle(&self) -> std::sync::MutexGuard<'_, crate::idle::IdleTracker> {
        self.idle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Thorium install layout for this workspace.
    pub(crate) fn thorium_layout(&self) -> thorium_workspace_thorium::InstallLayout {
        thorium_workspace_thorium::InstallLayout::new(&self.root.join(BROWSERS_REL))
    }
}

impl std::fmt::Debug for Workspace {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Deliberately minimal: the workspace holds stores, vaults, and a
        // clipboard port; none of their internals belong in logs.
        formatter
            .debug_struct("Workspace")
            .field("root", &self.root.display().to_string())
            .finish_non_exhaustive()
    }
}
