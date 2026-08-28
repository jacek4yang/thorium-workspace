//! Shared application state.

use std::sync::Arc;

use tokio::sync::Mutex;
use tw_controller::{AppError, AppResult, Workspace};
use tw_domain::DiagnosticCode;

/// The workspace, or the reason there isn't one.
///
/// Held behind an async mutex because several commands (launching a profile,
/// installing Thorium) are genuinely asynchronous and must not block the
/// runtime while they hold the lock.
pub struct AppState {
    workspace: Mutex<Option<Workspace>>,
    startup_error: Option<AppError>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("startup_error", &self.startup_error.as_ref().map(|e| e.code))
            .finish_non_exhaustive()
    }
}

impl AppState {
    /// A state with a usable workspace.
    #[must_use]
    pub fn ready(workspace: Workspace) -> Self {
        Self {
            workspace: Mutex::new(Some(workspace)),
            startup_error: None,
        }
    }

    /// A state that failed to open its workspace.
    #[must_use]
    pub fn failed(error: AppError) -> Self {
        Self {
            workspace: Mutex::new(None),
            startup_error: Some(error),
        }
    }

    /// The startup failure, if there was one.
    #[must_use]
    pub const fn startup_error(&self) -> Option<&AppError> {
        self.startup_error.as_ref()
    }

    /// Runs `f` against the workspace.
    ///
    /// # Errors
    ///
    /// Returns the startup error when there is no workspace, and otherwise
    /// whatever `f` returns.
    pub async fn with<R>(&self, f: impl FnOnce(&mut Workspace) -> AppResult<R>) -> AppResult<R> {
        let mut guard = self.workspace.lock().await;
        match guard.as_mut() {
            Some(workspace) => f(workspace),
            None => Err(self.unavailable()),
        }
    }

    /// Runs an async `f` against the workspace.
    ///
    /// Takes an async closure so the borrow of the workspace can live across
    /// the await without the caller having to box a future.
    ///
    /// # Errors
    ///
    /// Returns the startup error when there is no workspace, and otherwise
    /// whatever `f` returns.
    pub async fn with_async<R>(&self, f: impl AsyncFnOnce(&mut Workspace) -> AppResult<R>) -> AppResult<R> {
        let mut guard = self.workspace.lock().await;
        match guard.as_mut() {
            Some(workspace) => f(workspace).await,
            None => Err(self.unavailable()),
        }
    }

    /// Attempts to run `f` without waiting for the lock.
    ///
    /// Used by background tasks so a periodic tick can never queue up behind a
    /// long-running install.
    pub fn try_with<R>(&self, f: impl FnOnce(&mut Workspace) -> R) -> Option<R> {
        let mut guard = self.workspace.try_lock().ok()?;
        guard.as_mut().map(f)
    }

    fn unavailable(&self) -> AppError {
        self.startup_error
            .clone()
            .unwrap_or_else(|| AppError::new(DiagnosticCode::Internal, "the workspace is not available"))
    }
}

/// The managed state type commands receive.
pub type SharedState = Arc<AppState>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_failed_startup_reports_its_error_from_every_command() {
        let state = AppState::failed(
            AppError::new(DiagnosticCode::WorkspaceNotWritable, "cannot write").with_remedy("Move the app."),
        );
        assert_eq!(
            state.startup_error().map(|e| e.code),
            Some(DiagnosticCode::WorkspaceNotWritable)
        );

        let error = state.with(|_| Ok(())).await.expect_err("no workspace");
        assert_eq!(error.code, DiagnosticCode::WorkspaceNotWritable);
        assert_eq!(error.remedy.as_deref(), Some("Move the app."));
    }

    #[tokio::test]
    async fn a_ready_state_runs_the_closure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = Workspace::open_in(dir.path()).expect("workspace");
        let state = AppState::ready(workspace);
        assert!(state.startup_error().is_none());
        let count = state
            .with(|w| w.list_accounts().map(|a| a.len()))
            .await
            .expect("ran");
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn try_with_gives_up_rather_than_queueing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let workspace = Workspace::open_in(dir.path()).expect("workspace");
        let state = Arc::new(AppState::ready(workspace));

        let held = state.workspace.lock().await;
        assert!(
            state.try_with(|_| ()).is_none(),
            "a busy workspace must not block a background tick"
        );
        drop(held);
        assert!(state.try_with(|_| ()).is_some());
    }
}
