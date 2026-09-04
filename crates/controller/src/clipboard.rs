//! Secure clipboard scheduling.
//!
//! The scheduler owns at most one pending conditional clear. Copying
//! secret B replaces any pending clear for secret A: when A's deadline
//! fires afterwards, nothing is erased because the pending state belongs
//! to B (and the underlying `clear_if_matches` only erases content this
//! app wrote, never newer foreign clipboard content).
//!
//! Timing is driven by explicit `tick(now)` calls so tests are
//! deterministic and the Tauri layer can own a single interval task.

use std::time::{Duration, Instant};

use thorium_workspace_secrets::SecretText;

use crate::error::ControllerError;

/// Clipboard operations used by the scheduler. A tiny port so tests can
/// spy without touching the real system clipboard.
pub trait ClipboardPort {
    /// Writes the value to the clipboard.
    fn copy(&self, value: &SecretText) -> Result<(), ControllerError>;
    /// Clears the clipboard only if it still holds `expected`.
    fn clear_if_matches(&self, expected: &SecretText) -> Result<bool, ControllerError>;
}

/// Production port delegating to the Windows platform layer.
#[derive(Debug)]
pub struct SystemClipboard;

impl ClipboardPort for SystemClipboard {
    fn copy(&self, value: &SecretText) -> Result<(), ControllerError> {
        thorium_workspace_windows_platform::clipboard::copy_secret(value)?;
        Ok(())
    }

    fn clear_if_matches(&self, expected: &SecretText) -> Result<bool, ControllerError> {
        Ok(thorium_workspace_windows_platform::clipboard::clear_if_matches(expected)?)
    }
}

#[derive(Debug)]
struct PendingClear {
    value: SecretText,
    fire_at: Instant,
    #[allow(dead_code)] // diagnostics
    sequence: u64,
}

/// Owns the pending conditional clear.
#[derive(Debug, Default)]
pub struct ClipboardScheduler {
    pending: Option<PendingClear>,
    next_sequence: u64,
}

impl ClipboardScheduler {
    /// Creates an empty scheduler.
    pub fn new() -> Self {
        Self::default()
    }

    /// Copies `value` now and schedules its clear at `now + delay`.
    /// Any previously pending clear is cancelled (superseded).
    pub fn copy_scheduled(
        &mut self,
        port: &dyn ClipboardPort,
        value: SecretText,
        delay: Duration,
        now: Instant,
    ) -> Result<(), ControllerError> {
        port.copy(&value)?;
        self.next_sequence += 1;
        self.pending = Some(PendingClear {
            value,
            fire_at: now + delay,
            sequence: self.next_sequence,
        });
        Ok(())
    }

    /// Fires the pending clear if due. Returns `Ok(true)` when a clear
    /// was attempted (superseded or not, the pending slot is consumed).
    pub fn tick(
        &mut self,
        port: &dyn ClipboardPort,
        now: Instant,
    ) -> Result<bool, ControllerError> {
        let Some(pending) = &self.pending else {
            return Ok(false);
        };
        if now < pending.fire_at {
            return Ok(false);
        }
        let pending = self.pending.take().expect("checked above");
        port.clear_if_matches(&pending.value)?;
        Ok(true)
    }

    /// Cancels any pending clear without touching the clipboard.
    pub fn cancel(&mut self) {
        self.pending = None;
    }

    /// When the pending clear fires, if any (diagnostics/tests).
    pub fn pending_fire_at(&self) -> Option<Instant> {
        self.pending.as_ref().map(|pending| pending.fire_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct SpyClipboard {
        copied: Mutex<Vec<String>>,
        cleared: Mutex<Vec<String>>,
        current: Mutex<Option<String>>,
    }

    impl SpyClipboard {
        fn copied(&self) -> Vec<String> {
            self.copied.lock().expect("spy").clone()
        }

        fn cleared(&self) -> Vec<String> {
            self.cleared.lock().expect("spy").clone()
        }
    }

    impl ClipboardPort for SpyClipboard {
        fn copy(&self, value: &SecretText) -> Result<(), ControllerError> {
            self.copied
                .lock()
                .expect("spy")
                .push(value.expose().to_owned());
            *self.current.lock().expect("spy") = Some(value.expose().to_owned());
            Ok(())
        }

        fn clear_if_matches(&self, expected: &SecretText) -> Result<bool, ControllerError> {
            let expected_text = expected.expose().to_owned();
            let mut current = self.current.lock().expect("spy");
            let cleared = *current == Some(expected_text.clone());
            if cleared {
                self.cleared
                    .lock()
                    .expect("spy")
                    .push(expected_text.clone());
                *current = None;
            }
            Ok(cleared)
        }
    }

    const SECRET_A: &str = "synthetic-secret-a";
    const SECRET_B: &str = "synthetic-secret-b";

    #[test]
    fn superseded_timer_must_not_erase_newer_secret() {
        let port = SpyClipboard::default();
        let mut scheduler = ClipboardScheduler::new();
        let t0 = Instant::now();

        scheduler
            .copy_scheduled(&port, SecretText::new(SECRET_A), Duration::from_secs(2), t0)
            .expect("copy A");
        assert_eq!(port.copied(), vec![SECRET_A.to_owned()]);

        // Copy B before A's deadline: A's timer is superseded.
        scheduler
            .copy_scheduled(&port, SecretText::new(SECRET_B), Duration::from_secs(5), t0)
            .expect("copy B");
        assert_eq!(
            port.copied(),
            vec![SECRET_A.to_owned(), SECRET_B.to_owned()]
        );

        // A's deadline fires: nothing may be cleared.
        scheduler
            .tick(&port, t0 + Duration::from_secs(2))
            .expect("tick A");
        assert!(port.cleared().is_empty(), "timer A must not erase B");

        // B's deadline fires: B is cleared; A is not.
        scheduler
            .tick(&port, t0 + Duration::from_secs(5))
            .expect("tick B");
        assert_eq!(port.cleared(), vec![SECRET_B.to_owned()]);

        // Ticks after completion are no-ops.
        scheduler
            .tick(&port, t0 + Duration::from_secs(50))
            .expect("tick idle");
        assert_eq!(port.cleared().len(), 1);
    }

    #[test]
    fn early_tick_does_not_clear() {
        let port = SpyClipboard::default();
        let mut scheduler = ClipboardScheduler::new();
        let t0 = Instant::now();
        scheduler
            .copy_scheduled(
                &port,
                SecretText::new(SECRET_A),
                Duration::from_secs(10),
                t0,
            )
            .expect("copy");
        scheduler
            .tick(&port, t0 + Duration::from_secs(9))
            .expect("early");
        assert!(port.cleared().is_empty());
        assert!(scheduler.pending_fire_at().is_some());
    }

    #[test]
    fn cancel_drops_pending_clear() {
        let port = SpyClipboard::default();
        let mut scheduler = ClipboardScheduler::new();
        let t0 = Instant::now();
        scheduler
            .copy_scheduled(&port, SecretText::new(SECRET_A), Duration::from_secs(1), t0)
            .expect("copy");
        scheduler.cancel();
        assert!(scheduler.pending_fire_at().is_none());
        scheduler
            .tick(&port, t0 + Duration::from_secs(60))
            .expect("tick");
        assert!(port.cleared().is_empty());
    }
}
