//! Versioned schema migrations.
//!
//! Each migration is an idempotent-forward SQL script applied exactly once,
//! in order, inside a transaction together with its bookkeeping row. The
//! applied set is recorded in `schema_migrations`.

/// One ordered schema migration.
pub(crate) struct Migration {
    /// Monotonically increasing schema version.
    pub version: i64,
    /// Short human-readable name (recorded for diagnostics).
    pub name: &'static str,
    /// SQL applied when moving to `version`.
    pub sql: &'static str,
}

/// Highest schema version this application understands.
pub(crate) const LATEST: i64 = 1;

/// All migrations in application order.
pub(crate) const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "initial schema",
    sql: include_str!("migrations/0001_initial.sql"),
}];

/// Returns migrations that still need to be applied.
pub(crate) fn pending(current: Option<i64>) -> impl Iterator<Item = &'static Migration> {
    MIGRATIONS
        .iter()
        .filter(move |migration| current.is_none_or(|applied| migration.version > applied))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_strictly_increasing_from_one() {
        let mut expected = 1;
        for migration in MIGRATIONS {
            assert_eq!(migration.version, expected, "gap in migration sequence");
            expected += 1;
        }
        assert_eq!(LATEST, expected - 1);
    }

    #[test]
    fn pending_is_empty_when_current() {
        assert_eq!(pending(Some(LATEST)).count(), 0);
        assert_eq!(pending(None).count(), MIGRATIONS.len());
    }
}
