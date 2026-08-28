//! Timestamps.
//!
//! Persisted times are Unix epoch seconds in UTC. Keeping a single integer
//! representation avoids timezone ambiguity in the database and keeps the domain
//! free of a date-time library's opinions; formatting is a presentation concern.

use core::fmt;

use serde::{Deserialize, Serialize};

/// A point in time, stored as whole seconds since the Unix epoch (UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(i64);

impl Timestamp {
    /// The Unix epoch.
    pub const EPOCH: Self = Self(0);

    /// Wraps a Unix epoch second count.
    #[must_use]
    pub const fn from_unix_seconds(seconds: i64) -> Self {
        Self(seconds)
    }

    /// Returns the Unix epoch second count.
    #[must_use]
    pub const fn as_unix_seconds(self) -> i64 {
        self.0
    }

    /// Returns the current time.
    ///
    /// A system clock set before 1970 yields [`Timestamp::EPOCH`] rather than
    /// panicking; nothing in the product depends on pre-epoch times.
    #[must_use]
    pub fn now() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => Self(i64::try_from(d.as_secs()).unwrap_or(i64::MAX)),
            Err(_) => Self::EPOCH,
        }
    }

    /// Returns the number of seconds elapsed since `self`, saturating at zero.
    #[must_use]
    pub fn seconds_since(self, later: Timestamp) -> u64 {
        u64::try_from(later.0.saturating_sub(self.0)).unwrap_or(0)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_is_after_2024() {
        // 2024-01-01T00:00:00Z
        assert!(Timestamp::now().as_unix_seconds() > 1_704_067_200);
    }

    #[test]
    fn seconds_since_saturates_backwards() {
        let a = Timestamp::from_unix_seconds(100);
        let b = Timestamp::from_unix_seconds(160);
        assert_eq!(a.seconds_since(b), 60);
        assert_eq!(b.seconds_since(a), 0);
    }

    #[test]
    fn serializes_as_a_bare_integer() {
        let json = serde_json::to_string(&Timestamp::from_unix_seconds(42)).expect("serialize");
        assert_eq!(json, "42");
    }
}
