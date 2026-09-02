//! Workspace settings, Thorium install registry, and runtime metadata.

use rusqlite::params;
use thorium_workspace_domain::{ThoriumInstall, WorkspaceSettings};

use crate::Store;
use crate::error::{StorageError, map_write_error};
use crate::time;

impl Store {
    /// Loads the workspace settings, or `None` before first save (the
    /// controller applies defaults).
    pub fn load_settings(&self) -> Result<Option<WorkspaceSettings>, StorageError> {
        let mut statement = self
            .conn
            .prepare("SELECT data FROM workspace_settings WHERE id = 1")?;
        let mut rows = statement.query([])?;
        match rows.next()? {
            Some(row) => {
                let text: String = row.get(0)?;
                let settings =
                    serde_json::from_str(&text).map_err(|source| StorageError::Corrupt {
                        column: "workspace_settings.data",
                        detail: source.to_string(),
                    })?;
                Ok(Some(settings))
            }
            None => Ok(None),
        }
    }

    /// Saves the workspace settings (upsert of the single row).
    pub fn save_settings(&self, settings: &WorkspaceSettings) -> Result<(), StorageError> {
        let data = serde_json::to_string(settings)?;
        self.conn.execute(
            "INSERT INTO workspace_settings (id, data) VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET data = excluded.data",
            params![data],
        )?;
        Ok(())
    }

    /// Registers an installed Thorium version/variant.
    pub fn add_thorium_install(&self, install: &ThoriumInstall) -> Result<(), StorageError> {
        self.conn
            .execute(
                "INSERT INTO thorium_installs (version, variant, rel_path, installed_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(version, variant) DO UPDATE SET
                    rel_path = excluded.rel_path,
                    installed_at = excluded.installed_at",
                params![
                    install.version,
                    install.variant,
                    install.rel_path,
                    time::to_text(install.installed_at)
                ],
            )
            .map_err(|error| map_write_error(error, "thorium install"))?;
        Ok(())
    }

    /// Lists registered installs, newest first.
    pub fn list_thorium_installs(&self) -> Result<Vec<ThoriumInstall>, StorageError> {
        let mut statement = self.conn.prepare(
            "SELECT version, variant, rel_path, installed_at
             FROM thorium_installs
             ORDER BY installed_at DESC",
        )?;
        let rows = statement.query_map([], |row| {
            let installed_text: String = row.get("installed_at")?;
            Ok((
                row.get::<_, String>("version")?,
                row.get::<_, String>("variant")?,
                row.get::<_, String>("rel_path")?,
                installed_text,
            ))
        })?;
        let mut installs = Vec::new();
        for row in rows {
            let (version, variant, rel_path, installed_text) = row?;
            installs.push(ThoriumInstall {
                version,
                variant,
                rel_path,
                installed_at: time::from_text("thorium_installs.installed_at", &installed_text)?,
            });
        }
        Ok(installs)
    }

    /// Removes a registered install. Returns `false` when the install is
    /// not registered.
    pub fn remove_thorium_install(
        &self,
        version: &str,
        variant: &str,
    ) -> Result<bool, StorageError> {
        let changed = self.conn.execute(
            "DELETE FROM thorium_installs WHERE version = ?1 AND variant = ?2",
            params![version, variant],
        )?;
        Ok(changed > 0)
    }

    /// Reads a runtime metadata value.
    pub fn get_meta(&self, key: &str) -> Result<Option<String>, StorageError> {
        let value = self
            .conn
            .query_row(
                "SELECT value FROM runtime_meta WHERE key = ?1",
                params![key],
                |row| row.get::<_, String>(0),
            )
            .map(Some)
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(StorageError::from(other)),
            })?;
        Ok(value)
    }

    /// Writes a runtime metadata value (upsert).
    pub fn set_meta(&self, key: &str, value: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT INTO runtime_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn temp_store(tag: &str) -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(format!("{tag}.db"));
        let store = Store::open(&path).expect("open");
        (dir, store)
    }

    #[test]
    fn settings_default_to_none_then_roundtrip() {
        let (_dir, store) = temp_store("settings");
        assert!(store.load_settings().expect("load").is_none());

        let settings = WorkspaceSettings::default();
        store.save_settings(&settings).expect("save");
        let loaded = store.load_settings().expect("load").expect("present");
        assert_eq!(loaded, settings);

        let mut changed = settings;
        changed.clipboard_clear_seconds = 45;
        changed.theme = thorium_workspace_domain::ThemePreference::Dark;
        store.save_settings(&changed).expect("save again");
        let reloaded = store.load_settings().expect("load").expect("present");
        assert_eq!(reloaded, changed);
    }

    #[test]
    fn thorium_installs_roundtrip() {
        let (_dir, store) = temp_store("installs");
        let install = ThoriumInstall {
            version: "M152.0.7977.55".to_owned(),
            variant: "AVX2".to_owned(),
            rel_path: "browsers/thorium/versions/M152.0.7977.55".to_owned(),
            installed_at: Utc::now(),
        };
        store.add_thorium_install(&install).expect("add");
        assert_eq!(
            store.list_thorium_installs().expect("list"),
            vec![install.clone()]
        );

        // Re-registering updates in place.
        let mut updated = install.clone();
        updated.installed_at += chrono::Duration::hours(1);
        store.add_thorium_install(&updated).expect("upsert");
        let listed = store.list_thorium_installs().expect("list");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].installed_at, updated.installed_at);

        assert!(
            store
                .remove_thorium_install(&install.version, &install.variant)
                .expect("remove")
        );
        assert!(
            !store
                .remove_thorium_install(&install.version, &install.variant)
                .expect("remove again")
        );
        assert!(store.list_thorium_installs().expect("list").is_empty());
    }

    #[test]
    fn runtime_meta_upserts() {
        let (_dir, store) = temp_store("meta");
        assert!(store.get_meta("onboarded").expect("get").is_none());
        store.set_meta("onboarded", "true").expect("set");
        assert_eq!(
            store.get_meta("onboarded").expect("get").as_deref(),
            Some("true")
        );
        store.set_meta("onboarded", "false").expect("set again");
        assert_eq!(
            store.get_meta("onboarded").expect("get").as_deref(),
            Some("false")
        );
    }
}
