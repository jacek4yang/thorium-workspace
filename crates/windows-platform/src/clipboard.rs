//! Secure clipboard behavior.
//!
//! Copies of secret material are cleared after a short interval, and the
//! clear is *conditional*: content is only erased if the clipboard still
//! contains the exact value written by this application. Content written
//! by another application in the meantime is never destroyed.
//!
//! The comparison uses [`thorium_workspace_secrets::constant_time_eq`] so
//! the check itself does not leak length- or content-dependent timing of
//! the secret.
//!
//! # Ownership model
//!
//! [`Clipboard`] is a thin handle over the OS clipboard; Windows opens
//! the clipboard per operation, so the handle carries no lock that could
//! deadlock the system clipboard.

#![forbid(unsafe_code)]

use thorium_workspace_secrets::{SecretBytes, SecretText, constant_time_eq};

use crate::error::PlatformError;

/// Writes `value` to the clipboard as text.
pub fn copy_secret(value: &SecretText) -> Result<(), PlatformError> {
    let mut clipboard = open_clipboard()?;
    clipboard
        .set_text(value.expose())
        .map_err(|error| PlatformError::Clipboard(error.to_string()))
}

/// Clears the clipboard only if its current text matches `expected`.
///
/// Returns `Ok(true)` when the clipboard was cleared (it still held the
/// expected value) and `Ok(false)` when the clipboard now holds something
/// else, which is left untouched. Images or non-text formats always block
/// clearing: they cannot contain the exact text value this app wrote.
pub fn clear_if_matches(expected: &SecretText) -> Result<bool, PlatformError> {
    let mut clipboard = open_clipboard()?;
    let current = match clipboard.get_text() {
        Ok(text) => text,
        Err(_) => {
            // Non-text content (or an empty clipboard): never erase it.
            return Ok(false);
        }
    };
    // The comparison treats the expected value as the secret operand; the
    // temporary SecretBytes copy is zeroized on drop.
    let expected_bytes = SecretBytes::new(expected.expose().as_bytes());
    if constant_time_eq(current.as_bytes(), &expected_bytes) {
        clipboard
            .clear()
            .map_err(|error| PlatformError::Clipboard(error.to_string()))?;
        return Ok(true);
    }
    Ok(false)
}

/// Unconditionally clears the clipboard (diagnostics/utility use).
pub fn clear() -> Result<(), PlatformError> {
    let mut clipboard = open_clipboard()?;
    clipboard
        .clear()
        .map_err(|error| PlatformError::Clipboard(error.to_string()))
}

fn open_clipboard() -> Result<arboard::Clipboard, PlatformError> {
    arboard::Clipboard::new().map_err(|error| PlatformError::Clipboard(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The system clipboard is process-global (and machine-global): these
    /// tests must run one at a time or they overwrite each other's data.
    static CLIPBOARD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn copied_secret_can_be_cleared_when_untouched() {
        let _clipboard = CLIPBOARD_LOCK.lock().expect("clipboard lock");
        let secret = SecretText::new("synthetic-clipboard-secret");
        copy_secret(&secret).expect("copy");
        let cleared = clear_if_matches(&secret).expect("conditional clear");
        assert!(cleared, "clipboard still held our value; it must clear");
    }

    #[test]
    fn foreign_content_is_never_destroyed() {
        let _clipboard = CLIPBOARD_LOCK.lock().expect("clipboard lock");
        let foreign = SecretText::new("content from another application");
        copy_secret(&foreign).expect("copy foreign");
        let secret = SecretText::new("synthetic-clipboard-secret");
        // `secret` is NOT on the clipboard: the clear must be refused.
        let cleared = clear_if_matches(&secret).expect("conditional clear");
        assert!(!cleared, "foreign clipboard content must be preserved");
        // The foreign content is still there.
        let current = open_clipboard().expect("open").get_text().expect("text");
        assert_eq!(current, foreign.expose());
    }

    #[test]
    fn empty_clipboard_clear_is_a_no_noop() {
        let _clipboard = CLIPBOARD_LOCK.lock().expect("clipboard lock");
        clear().expect("clear");
        let secret = SecretText::new("not-on-clipboard");
        let cleared = clear_if_matches(&secret).expect("conditional clear");
        assert!(!cleared, "empty clipboard must not report a clear");
    }
}
