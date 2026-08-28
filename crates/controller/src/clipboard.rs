//! Clipboard protection.
//!
//! Copying a password or an OTP code puts a secret somewhere every other program
//! on the machine can read. This module bounds how long it stays there.
//!
//! The rule that matters: **the clipboard is only cleared if it still contains
//! exactly what this application wrote.** If the user copied something else in
//! the meantime, that content is theirs and is left alone. Anything less careful
//! turns a security feature into a data-loss bug.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tw_domain::DiagnosticCode;
use tw_secrets::SecretString;

use crate::error::{AppError, AppResult};

/// What kind of secret was copied. Shown in the "copied" toast; never the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyKind {
    /// An account password.
    Password,
    /// A generated one-time code.
    OtpCode,
    /// A recovery code.
    RecoveryCode,
    /// A username or other non-secret field.
    PlainField,
}

impl CopyKind {
    /// A label for the UI.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Password => "Password",
            Self::OtpCode => "One-time code",
            Self::RecoveryCode => "Recovery code",
            Self::PlainField => "Value",
        }
    }

    /// Whether copies of this kind are subject to automatic clearing.
    #[must_use]
    pub const fn is_secret(self) -> bool {
        !matches!(self, Self::PlainField)
    }
}

/// The clipboard operations this crate needs.
///
/// Abstracted so the conditional-clear rule can be tested exhaustively without a
/// real desktop clipboard, which no CI runner has.
pub trait ClipboardBackend: Send + Sync {
    /// Reads the clipboard's current text.
    ///
    /// # Errors
    ///
    /// Returns an error when the clipboard cannot be read.
    fn read_text(&self) -> Result<String, String>;

    /// Writes text to the clipboard.
    ///
    /// # Errors
    ///
    /// Returns an error when the clipboard cannot be written.
    fn write_text(&self, value: &str) -> Result<(), String>;

    /// Clears the clipboard.
    ///
    /// # Errors
    ///
    /// Returns an error when the clipboard cannot be cleared.
    fn clear(&self) -> Result<(), String>;
}

/// The real Windows clipboard.
#[derive(Debug, Default)]
pub struct SystemClipboard;

impl ClipboardBackend for SystemClipboard {
    fn read_text(&self) -> Result<String, String> {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        clipboard.get_text().map_err(|e| e.to_string())
    }

    fn write_text(&self, value: &str) -> Result<(), String> {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        clipboard.set_text(value.to_owned()).map_err(|e| e.to_string())
    }

    fn clear(&self) -> Result<(), String> {
        let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
        clipboard.clear().map_err(|e| e.to_string())
    }
}

/// Reads an image from the system clipboard as RGBA8.
///
/// Used by "import a 2FA QR code from the clipboard".
///
/// # Errors
///
/// Returns [`DiagnosticCode::QrClipboardEmpty`] when the clipboard holds no
/// image.
pub fn read_clipboard_image() -> AppResult<(u32, u32, Vec<u8>)> {
    let mut clipboard = arboard::Clipboard::new()
        .map_err(|e| AppError::new(DiagnosticCode::ClipboardFailed, e.to_string()))?;
    let image = clipboard.get_image().map_err(|_| {
        AppError::new(
            DiagnosticCode::QrClipboardEmpty,
            "the clipboard does not contain an image",
        )
        .with_remedy("Take a screenshot of the QR code first, then import from the clipboard.")
    })?;
    let width = u32::try_from(image.width).map_err(|_| {
        AppError::new(
            DiagnosticCode::QrClipboardEmpty,
            "the clipboard image is too large",
        )
    })?;
    let height = u32::try_from(image.height).map_err(|_| {
        AppError::new(
            DiagnosticCode::QrClipboardEmpty,
            "the clipboard image is too large",
        )
    })?;
    Ok((width, height, image.bytes.into_owned()))
}

/// What the guard last wrote, so it can tell whether the clipboard is still its
/// own content.
#[derive(Debug)]
struct Tracked {
    value: SecretString,
    generation: u64,
}

/// Writes secrets to the clipboard and clears them again, conditionally.
pub struct ClipboardGuard {
    backend: Arc<dyn ClipboardBackend>,
    tracked: Arc<Mutex<Option<Tracked>>>,
    generation: Arc<Mutex<u64>>,
}

impl std::fmt::Debug for ClipboardGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never render what is being tracked: it is a live secret.
        f.debug_struct("ClipboardGuard")
            .field("tracking", &self.is_tracking())
            .finish_non_exhaustive()
    }
}

impl ClipboardGuard {
    /// Builds a guard over the real system clipboard.
    #[must_use]
    pub fn system() -> Self {
        Self::with_backend(Arc::new(SystemClipboard))
    }

    /// Builds a guard over a supplied backend.
    #[must_use]
    pub fn with_backend(backend: Arc<dyn ClipboardBackend>) -> Self {
        Self {
            backend,
            tracked: Arc::new(Mutex::new(None)),
            generation: Arc::new(Mutex::new(0)),
        }
    }

    /// Whether a secret written by this guard is still being tracked.
    #[must_use]
    pub fn is_tracking(&self) -> bool {
        self.tracked.lock().is_ok_and(|t| t.is_some())
    }

    /// Copies `value` and, for secret kinds with clearing enabled, schedules a
    /// conditional clear after `clear_after` seconds.
    ///
    /// # Errors
    ///
    /// Returns [`DiagnosticCode::ClipboardFailed`] when the clipboard cannot be
    /// written.
    pub fn copy(
        &self,
        value: &SecretString,
        kind: CopyKind,
        clear_enabled: bool,
        clear_after: std::time::Duration,
    ) -> AppResult<()> {
        self.backend
            .write_text(value.expose())
            .map_err(|e| AppError::new(DiagnosticCode::ClipboardFailed, e))?;

        if !kind.is_secret() || !clear_enabled {
            // A non-secret copy releases any earlier tracking: the clipboard no
            // longer holds what this guard put there.
            if let Ok(mut tracked) = self.tracked.lock() {
                *tracked = None;
            }
            return Ok(());
        }

        let generation = {
            let Ok(mut generation) = self.generation.lock() else {
                return Err(AppError::internal("the clipboard guard state is poisoned"));
            };
            *generation += 1;
            *generation
        };
        if let Ok(mut tracked) = self.tracked.lock() {
            *tracked = Some(Tracked {
                value: SecretString::new(value.expose()),
                generation,
            });
        }

        let backend = Arc::clone(&self.backend);
        let tracked = Arc::clone(&self.tracked);
        tokio::spawn(async move {
            tokio::time::sleep(clear_after).await;
            clear_if_unchanged(backend.as_ref(), &tracked, generation);
        });
        Ok(())
    }

    /// Clears the clipboard now, but only if it still holds what this guard
    /// wrote.
    ///
    /// Returns `true` when the clipboard was actually cleared.
    pub fn clear_now(&self) -> bool {
        let generation = self
            .tracked
            .lock()
            .ok()
            .and_then(|t| t.as_ref().map(|t| t.generation));
        match generation {
            Some(generation) => clear_if_unchanged(self.backend.as_ref(), &self.tracked, generation),
            None => false,
        }
    }

    /// Forgets what is being tracked without touching the clipboard.
    ///
    /// Used at shutdown: the app stops being responsible for content it can no
    /// longer supervise.
    pub fn forget(&self) {
        if let Ok(mut tracked) = self.tracked.lock() {
            *tracked = None;
        }
    }
}

/// The conditional-clear rule, in one place.
///
/// Clears only when all of the following hold:
/// * this generation is still the current one (no newer copy has happened);
/// * the clipboard is readable;
/// * its content is byte-for-byte what was written.
fn clear_if_unchanged(
    backend: &dyn ClipboardBackend,
    tracked: &Mutex<Option<Tracked>>,
    generation: u64,
) -> bool {
    let Ok(mut slot) = tracked.lock() else {
        return false;
    };
    let Some(entry) = slot.as_ref() else {
        return false;
    };
    if entry.generation != generation {
        // A newer copy superseded this one; that copy owns its own timer.
        return false;
    }

    let Ok(current) = backend.read_text() else {
        // The clipboard could not be read: another application may hold it, or
        // it may contain something that is not text. Clearing blind would
        // destroy content this app did not put there.
        return false;
    };
    if !tw_secrets::constant_time_eq(current.as_bytes(), entry.value.expose_bytes()) {
        // Someone else copied something. It is theirs; leave it.
        *slot = None;
        return false;
    }

    let cleared = backend.clear().is_ok();
    *slot = None;
    cleared
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An in-memory clipboard that records what happened to it.
    #[derive(Debug, Default)]
    struct FakeClipboard {
        content: Mutex<Option<String>>,
        readable: Mutex<bool>,
        clears: Mutex<usize>,
    }

    impl FakeClipboard {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                content: Mutex::new(None),
                readable: Mutex::new(true),
                clears: Mutex::new(0),
            })
        }

        fn set_external(&self, value: &str) {
            *self.content.lock().expect("lock") = Some(value.to_owned());
        }

        fn content(&self) -> Option<String> {
            self.content.lock().expect("lock").clone()
        }

        fn clears(&self) -> usize {
            *self.clears.lock().expect("lock")
        }

        fn make_unreadable(&self) {
            *self.readable.lock().expect("lock") = false;
        }
    }

    impl ClipboardBackend for FakeClipboard {
        fn read_text(&self) -> Result<String, String> {
            if !*self.readable.lock().expect("lock") {
                return Err("the clipboard holds no text".to_owned());
            }
            self.content
                .lock()
                .expect("lock")
                .clone()
                .ok_or_else(|| "empty".to_owned())
        }

        fn write_text(&self, value: &str) -> Result<(), String> {
            *self.content.lock().expect("lock") = Some(value.to_owned());
            Ok(())
        }

        fn clear(&self) -> Result<(), String> {
            *self.content.lock().expect("lock") = None;
            *self.clears.lock().expect("lock") += 1;
            Ok(())
        }
    }

    fn guard(backend: &Arc<FakeClipboard>) -> ClipboardGuard {
        ClipboardGuard::with_backend(Arc::clone(backend) as Arc<dyn ClipboardBackend>)
    }

    const NEVER: std::time::Duration = std::time::Duration::from_secs(3600);

    #[tokio::test]
    async fn copying_a_secret_puts_it_on_the_clipboard_and_tracks_it() {
        let backend = FakeClipboard::new();
        let guard = guard(&backend);
        guard
            .copy(&SecretString::new("hunter2"), CopyKind::Password, true, NEVER)
            .expect("copy");
        assert_eq!(backend.content().as_deref(), Some("hunter2"));
        assert!(guard.is_tracking());
    }

    #[tokio::test]
    async fn clearing_removes_content_this_app_wrote() {
        let backend = FakeClipboard::new();
        let guard = guard(&backend);
        guard
            .copy(&SecretString::new("hunter2"), CopyKind::Password, true, NEVER)
            .expect("copy");
        assert!(guard.clear_now());
        assert_eq!(backend.content(), None);
        assert_eq!(backend.clears(), 1);
        assert!(!guard.is_tracking());
    }

    #[tokio::test]
    async fn newer_content_from_another_application_is_never_erased() {
        let backend = FakeClipboard::new();
        let guard = guard(&backend);
        guard
            .copy(&SecretString::new("hunter2"), CopyKind::Password, true, NEVER)
            .expect("copy");

        // The user copies something else.
        backend.set_external("a paragraph the user is working on");

        assert!(!guard.clear_now(), "the clipboard no longer holds our secret");
        assert_eq!(
            backend.content().as_deref(),
            Some("a paragraph the user is working on"),
            "another application's content must survive"
        );
        assert_eq!(backend.clears(), 0, "clear() must not even be called");
    }

    #[tokio::test]
    async fn an_unreadable_clipboard_is_left_alone() {
        let backend = FakeClipboard::new();
        let guard = guard(&backend);
        guard
            .copy(&SecretString::new("hunter2"), CopyKind::Password, true, NEVER)
            .expect("copy");
        backend.make_unreadable();
        assert!(
            !guard.clear_now(),
            "clearing blind could destroy someone else's content"
        );
        assert_eq!(backend.clears(), 0);
    }

    #[tokio::test]
    async fn a_superseded_copy_does_not_clear_the_newer_one() {
        let backend = FakeClipboard::new();
        let guard = guard(&backend);
        guard
            .copy(&SecretString::new("first"), CopyKind::Password, true, NEVER)
            .expect("copy");
        guard
            .copy(&SecretString::new("second"), CopyKind::OtpCode, true, NEVER)
            .expect("copy");

        // The first copy's timer fires late. It must not clear the second value.
        assert!(!clear_if_unchanged(backend.as_ref(), &guard.tracked, 1));
        assert_eq!(backend.content().as_deref(), Some("second"));

        // The second copy's own timer does clear it.
        assert!(clear_if_unchanged(backend.as_ref(), &guard.tracked, 2));
        assert_eq!(backend.content(), None);
    }

    #[tokio::test]
    async fn an_automatic_clear_happens_after_the_configured_delay() {
        let backend = FakeClipboard::new();
        let guard = guard(&backend);
        guard
            .copy(
                &SecretString::new("hunter2"),
                CopyKind::Password,
                true,
                std::time::Duration::from_millis(60),
            )
            .expect("copy");
        assert_eq!(backend.content().as_deref(), Some("hunter2"));

        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        assert_eq!(
            backend.content(),
            None,
            "the copy should have been cleared automatically"
        );
        assert_eq!(backend.clears(), 1);
    }

    #[tokio::test]
    async fn an_automatic_clear_still_respects_newer_content() {
        let backend = FakeClipboard::new();
        let guard = guard(&backend);
        guard
            .copy(
                &SecretString::new("hunter2"),
                CopyKind::Password,
                true,
                std::time::Duration::from_millis(60),
            )
            .expect("copy");
        backend.set_external("something the user copied");

        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        assert_eq!(backend.content().as_deref(), Some("something the user copied"));
        assert_eq!(backend.clears(), 0);
    }

    #[tokio::test]
    async fn disabling_automatic_clearing_leaves_the_value_in_place() {
        let backend = FakeClipboard::new();
        let guard = guard(&backend);
        guard
            .copy(
                &SecretString::new("hunter2"),
                CopyKind::Password,
                false,
                std::time::Duration::from_millis(20),
            )
            .expect("copy");
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert_eq!(backend.content().as_deref(), Some("hunter2"));
        assert!(
            !guard.is_tracking(),
            "nothing is scheduled when clearing is disabled"
        );
    }

    #[tokio::test]
    async fn non_secret_copies_are_never_cleared() {
        let backend = FakeClipboard::new();
        let guard = guard(&backend);
        guard
            .copy(
                &SecretString::new("alice@example.test"),
                CopyKind::PlainField,
                true,
                std::time::Duration::from_millis(20),
            )
            .expect("copy");
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        assert_eq!(backend.content().as_deref(), Some("alice@example.test"));
        assert_eq!(backend.clears(), 0);
    }

    #[tokio::test]
    async fn forgetting_leaves_the_clipboard_untouched() {
        let backend = FakeClipboard::new();
        let guard = guard(&backend);
        guard
            .copy(&SecretString::new("hunter2"), CopyKind::Password, true, NEVER)
            .expect("copy");
        guard.forget();
        assert!(!guard.is_tracking());
        assert_eq!(backend.content().as_deref(), Some("hunter2"));
        assert!(!guard.clear_now());
    }

    #[test]
    fn the_guard_debug_output_reveals_nothing() {
        let backend = FakeClipboard::new();
        let guard = guard(&backend);
        assert!(!format!("{guard:?}").contains("hunter2"));
    }

    #[test]
    fn copy_kinds_classify_secrets_correctly() {
        assert!(CopyKind::Password.is_secret());
        assert!(CopyKind::OtpCode.is_secret());
        assert!(CopyKind::RecoveryCode.is_secret());
        assert!(!CopyKind::PlainField.is_secret());
    }
}
