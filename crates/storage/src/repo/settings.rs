//! Workspace settings, stored as a small key/value table.
//!
//! A key/value table rather than a one-row settings table: adding a setting then
//! needs no migration, and a setting this build does not recognise survives a
//! downgrade instead of being dropped.

use rusqlite::{Connection, OptionalExtension, params};
use tw_domain::{ClipboardSettings, ThemePreference, ThoriumChannel, VaultSettings, WorkspaceSettings};

use crate::error::{StorageError, StorageResult};

const KEY_THEME: &str = "theme";
const KEY_CLIPBOARD_CLEAR_ENABLED: &str = "clipboard.clear_enabled";
const KEY_CLIPBOARD_CLEAR_SECONDS: &str = "clipboard.clear_after_seconds";
const KEY_VAULT_IDLE_ENABLED: &str = "vault.idle_lock_enabled";
const KEY_VAULT_IDLE_SECONDS: &str = "vault.idle_lock_seconds";
const KEY_VAULT_LOCK_ON_MINIMIZE: &str = "vault.lock_on_minimize";
const KEY_THORIUM_CHANNEL: &str = "thorium.channel";
const KEY_THORIUM_CHECK_ON_START: &str = "thorium.check_updates_on_start";

/// Reads and writes workspace settings.
pub struct SettingsRepo;

impl SettingsRepo {
    /// Reads a single raw setting.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] when the read fails.
    pub fn get_raw(conn: &Connection, key: &str) -> StorageResult<Option<String>> {
        let value = conn
            .query_row(
                "SELECT value FROM workspace_settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value)
    }

    /// Writes a single raw setting.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] when the write fails.
    pub fn set_raw(conn: &Connection, key: &str, value: &str) -> StorageResult<()> {
        conn.execute(
            "INSERT INTO workspace_settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Loads every setting, falling back to defaults for anything absent or
    /// unreadable.
    ///
    /// A malformed stored value must never stop the app from starting, so it is
    /// logged and replaced by the default rather than returned as an error.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] when the table cannot be read at all.
    pub fn load(conn: &Connection) -> StorageResult<WorkspaceSettings> {
        let defaults = WorkspaceSettings::default();
        let theme = match Self::get_raw(conn, KEY_THEME)? {
            Some(raw) => ThemePreference::parse(&raw).unwrap_or_else(|_| {
                tracing::warn!(key = KEY_THEME, "ignoring an unrecognised stored setting");
                defaults.theme
            }),
            None => defaults.theme,
        };
        let thorium_channel = match Self::get_raw(conn, KEY_THORIUM_CHANNEL)? {
            Some(raw) => ThoriumChannel::parse(&raw).unwrap_or_else(|_| {
                tracing::warn!(
                    key = KEY_THORIUM_CHANNEL,
                    "ignoring an unrecognised stored setting"
                );
                defaults.thorium_channel
            }),
            None => defaults.thorium_channel,
        };
        let clipboard = ClipboardSettings {
            clear_enabled: Self::bool_or(
                conn,
                KEY_CLIPBOARD_CLEAR_ENABLED,
                defaults.clipboard.clear_enabled,
            )?,
            clear_after_seconds: Self::u32_or(
                conn,
                KEY_CLIPBOARD_CLEAR_SECONDS,
                defaults.clipboard.clear_after_seconds,
            )?,
        };
        let vault = VaultSettings {
            idle_lock_enabled: Self::bool_or(conn, KEY_VAULT_IDLE_ENABLED, defaults.vault.idle_lock_enabled)?,
            idle_lock_seconds: Self::u32_or(conn, KEY_VAULT_IDLE_SECONDS, defaults.vault.idle_lock_seconds)?,
            lock_on_minimize: Self::bool_or(
                conn,
                KEY_VAULT_LOCK_ON_MINIMIZE,
                defaults.vault.lock_on_minimize,
            )?,
        };
        let mut settings = WorkspaceSettings {
            theme,
            clipboard,
            vault,
            thorium_channel,
            check_thorium_updates_on_start: Self::bool_or(
                conn,
                KEY_THORIUM_CHECK_ON_START,
                defaults.check_thorium_updates_on_start,
            )?,
        };
        // A stored value outside the accepted range would otherwise make every
        // later save fail validation.
        if settings.validate().is_err() {
            tracing::warn!(
                "stored settings failed validation; falling back to defaults for the invalid parts"
            );
            if settings.clipboard.validate().is_err() {
                settings.clipboard = defaults.clipboard;
            }
            if settings.vault.validate().is_err() {
                settings.vault = defaults.vault;
            }
        }
        Ok(settings)
    }

    /// Writes every setting.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Conflict`] when the settings fail validation, and
    /// [`StorageError::Query`] when a write fails.
    pub fn save(conn: &Connection, settings: &WorkspaceSettings) -> StorageResult<()> {
        settings
            .validate()
            .map_err(|e| StorageError::Conflict(e.to_string()))?;
        Self::set_raw(conn, KEY_THEME, settings.theme.as_str())?;
        Self::set_raw(
            conn,
            KEY_CLIPBOARD_CLEAR_ENABLED,
            bool_text(settings.clipboard.clear_enabled),
        )?;
        Self::set_raw(
            conn,
            KEY_CLIPBOARD_CLEAR_SECONDS,
            &settings.clipboard.clear_after_seconds.to_string(),
        )?;
        Self::set_raw(
            conn,
            KEY_VAULT_IDLE_ENABLED,
            bool_text(settings.vault.idle_lock_enabled),
        )?;
        Self::set_raw(
            conn,
            KEY_VAULT_IDLE_SECONDS,
            &settings.vault.idle_lock_seconds.to_string(),
        )?;
        Self::set_raw(
            conn,
            KEY_VAULT_LOCK_ON_MINIMIZE,
            bool_text(settings.vault.lock_on_minimize),
        )?;
        Self::set_raw(conn, KEY_THORIUM_CHANNEL, settings.thorium_channel.as_str())?;
        Self::set_raw(
            conn,
            KEY_THORIUM_CHECK_ON_START,
            bool_text(settings.check_thorium_updates_on_start),
        )?;
        Ok(())
    }

    fn bool_or(conn: &Connection, key: &str, fallback: bool) -> StorageResult<bool> {
        Ok(match Self::get_raw(conn, key)? {
            Some(raw) => matches!(raw.as_str(), "true" | "1"),
            None => fallback,
        })
    }

    fn u32_or(conn: &Connection, key: &str, fallback: u32) -> StorageResult<u32> {
        Ok(match Self::get_raw(conn, key)? {
            Some(raw) => raw.parse().unwrap_or(fallback),
            None => fallback,
        })
    }
}

const fn bool_text(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

#[cfg(test)]
mod tests {
    use tw_domain::ThemePreference;

    use super::*;
    use crate::Database;

    #[test]
    fn an_empty_table_yields_the_defaults() {
        let db = Database::open_in_memory().expect("open");
        assert_eq!(
            SettingsRepo::load(db.connection()).expect("load"),
            WorkspaceSettings::default()
        );
    }

    #[test]
    fn settings_round_trip() {
        let db = Database::open_in_memory().expect("open");
        let settings = WorkspaceSettings {
            theme: ThemePreference::Dark,
            clipboard: ClipboardSettings {
                clear_enabled: false,
                clear_after_seconds: 45,
            },
            vault: VaultSettings {
                idle_lock_enabled: false,
                idle_lock_seconds: 120,
                lock_on_minimize: true,
            },
            thorium_channel: ThoriumChannel::WindowsSse3,
            check_thorium_updates_on_start: true,
        };
        SettingsRepo::save(db.connection(), &settings).expect("save");
        assert_eq!(SettingsRepo::load(db.connection()).expect("load"), settings);
    }

    #[test]
    fn saving_invalid_settings_is_refused() {
        let db = Database::open_in_memory().expect("open");
        let settings = WorkspaceSettings {
            clipboard: ClipboardSettings {
                clear_enabled: true,
                clear_after_seconds: 100_000,
            },
            ..Default::default()
        };
        assert!(matches!(
            SettingsRepo::save(db.connection(), &settings),
            Err(StorageError::Conflict(_))
        ));
    }

    #[test]
    fn a_corrupted_stored_value_falls_back_instead_of_failing_startup() {
        let db = Database::open_in_memory().expect("open");
        SettingsRepo::set_raw(db.connection(), KEY_THEME, "chartreuse").expect("write");
        SettingsRepo::set_raw(db.connection(), KEY_CLIPBOARD_CLEAR_SECONDS, "not a number").expect("write");
        SettingsRepo::set_raw(db.connection(), KEY_VAULT_IDLE_SECONDS, "999999999").expect("write");
        SettingsRepo::set_raw(db.connection(), KEY_THORIUM_CHANNEL, "windows_vax").expect("write");

        let loaded = SettingsRepo::load(db.connection()).expect("load");
        let defaults = WorkspaceSettings::default();
        assert_eq!(loaded.theme, defaults.theme);
        assert_eq!(
            loaded.clipboard.clear_after_seconds,
            defaults.clipboard.clear_after_seconds
        );
        assert_eq!(loaded.vault.idle_lock_seconds, defaults.vault.idle_lock_seconds);
        assert_eq!(loaded.thorium_channel, defaults.thorium_channel);
        assert!(loaded.validate().is_ok());
    }

    #[test]
    fn an_unknown_key_is_preserved_across_a_save() {
        let db = Database::open_in_memory().expect("open");
        SettingsRepo::set_raw(db.connection(), "future.setting", "kept").expect("write");
        SettingsRepo::save(db.connection(), &WorkspaceSettings::default()).expect("save");
        assert_eq!(
            SettingsRepo::get_raw(db.connection(), "future.setting")
                .expect("read")
                .as_deref(),
            Some("kept")
        );
    }
}
