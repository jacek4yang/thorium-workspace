//! Application-level idle tracking for vault auto-lock.
//!
//! Time is explicit: callers pass `now` (`Instant::now()` in production,
//! arbitrary values in tests), so auto-lock is testable without sleeps.

use std::time::{Duration, Instant};

/// Tracks the last activity instant and the idle threshold.
#[derive(Debug)]
pub struct IdleTracker {
    threshold: Option<Duration>,
    last_activity: Option<Instant>,
}

impl IdleTracker {
    /// Creates a tracker. `None` disables idle locking.
    pub fn new(threshold: Option<Duration>) -> Self {
        Self {
            threshold,
            last_activity: None,
        }
    }

    /// Records activity at `now`.
    pub fn record_activity(&mut self, now: Instant) {
        self.last_activity = Some(now);
    }

    /// Updates the threshold (settings change).
    pub fn set_threshold(&mut self, threshold: Option<Duration>) {
        self.threshold = threshold;
    }

    /// Configured threshold.
    pub fn threshold(&self) -> Option<Duration> {
        self.threshold
    }

    /// Whether the idle threshold has elapsed since the last activity.
    /// A tracker with no recorded activity is never idle (it arms on the
    /// first activity).
    pub fn is_idle(&self, now: Instant) -> bool {
        let Some(threshold) = self.threshold else {
            return false;
        };
        self.last_activity
            .is_some_and(|last| now.duration_since(last) >= threshold)
    }

    /// Clears the armed state (called after a lock fires).
    pub fn disarm(&mut self) {
        self.last_activity = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_requires_threshold_and_elapsed_time() {
        let mut tracker = IdleTracker::new(Some(Duration::from_secs(600)));
        let t0 = Instant::now();
        assert!(!tracker.is_idle(t0), "no activity recorded yet");

        tracker.record_activity(t0);
        assert!(!tracker.is_idle(t0 + Duration::from_secs(599)));
        assert!(tracker.is_idle(t0 + Duration::from_secs(600)));
    }

    #[test]
    fn activity_resets_the_clock() {
        let mut tracker = IdleTracker::new(Some(Duration::from_secs(10)));
        let t0 = Instant::now();
        tracker.record_activity(t0);
        assert!(!tracker.is_idle(t0 + Duration::from_secs(9)));
        tracker.record_activity(t0 + Duration::from_secs(9));
        assert!(!tracker.is_idle(t0 + Duration::from_secs(18)));
        assert!(tracker.is_idle(t0 + Duration::from_secs(19)));
    }

    #[test]
    fn disabled_threshold_never_idles() {
        let mut tracker = IdleTracker::new(None);
        let t0 = Instant::now();
        tracker.record_activity(t0);
        assert!(!tracker.is_idle(t0 + Duration::from_secs(10_000)));
        tracker.set_threshold(Some(Duration::from_secs(1)));
        assert!(tracker.is_idle(t0 + Duration::from_secs(1)));
    }
}
