//! Browser Profile persistence.
//!
//! The stored form mirrors [`thorium_workspace_domain::BrowserProfile`].
//! `user_data_rel_path` is written once at creation and never updated: a
//! profile's data directory must remain stable across the profile's life.

use rusqlite::{Row, params};
use thorium_workspace_domain::{BrowserProfile, ProfileId, ThoriumSelection};

use crate::Store;
use crate::error::{StorageError, map_write_error};
use crate::time;

/// Column tags used in `Corrupt` diagnostics.
const COL_ID: &str = "profiles.id";
const COL_SELECTION: &str = "profiles.selection";
const COL_URLS: &str = "profiles.startup_urls";
const COL_CREATED: &str = "profiles.created_at";
const COL_UPDATED: &str = "profiles.updated_at";
const COL_LAUNCHED: &str = "profiles.last_launched_at";

impl Store {
    /// Inserts a new Browser Profile.
    pub fn create_profile(&self, profile: &BrowserProfile) -> Result<(), StorageError> {
        let (selection, pinned) = profile.thorium_version.storage_parts();
        let startup_urls = serde_json::to_string(&profile.startup_urls)?;
        self.conn
            .execute(
                "INSERT INTO profiles (
                    id, name, selection, pinned_version, user_data_rel_path,
                    startup_urls, locale, timezone, created_at, updated_at, last_launched_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    profile.id.to_string(),
                    profile.name,
                    selection,
                    pinned,
                    profile.user_data_rel_path,
                    startup_urls,
                    profile.locale,
                    profile.timezone,
                    time::to_text(profile.created_at),
                    time::to_text(profile.updated_at),
                    profile.last_launched_at.map(time::to_text),
                ],
            )
            .map_err(|error| map_write_error(error, "profile"))?;
        Ok(())
    }

    /// Loads every profile, ordered by name.
    pub fn list_profiles(&self) -> Result<Vec<BrowserProfile>, StorageError> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, selection, pinned_version, user_data_rel_path,
                    startup_urls, locale, timezone, created_at, updated_at, last_launched_at
             FROM profiles
             ORDER BY name COLLATE NOCASE",
        )?;
        let mut rows = statement.query([])?;
        let mut profiles = Vec::new();
        while let Some(row) = rows.next()? {
            profiles.push(profile_from_row(row)?);
        }
        Ok(profiles)
    }

    /// Loads one profile by id.
    pub fn get_profile(&self, id: ProfileId) -> Result<Option<BrowserProfile>, StorageError> {
        let mut statement = self.conn.prepare(
            "SELECT id, name, selection, pinned_version, user_data_rel_path,
                    startup_urls, locale, timezone, created_at, updated_at, last_launched_at
             FROM profiles
             WHERE id = ?1",
        )?;
        let mut rows = statement.query(params![id.to_string()])?;
        match rows.next()? {
            Some(row) => Ok(Some(profile_from_row(row)?)),
            None => Ok(None),
        }
    }

    /// Updates mutable profile fields. Returns `false` when the profile
    /// does not exist.
    pub fn update_profile(&self, profile: &BrowserProfile) -> Result<bool, StorageError> {
        let (selection, pinned) = profile.thorium_version.storage_parts();
        let startup_urls = serde_json::to_string(&profile.startup_urls)?;
        let changed = self.conn.execute(
            "UPDATE profiles SET
                name = ?2, selection = ?3, pinned_version = ?4, startup_urls = ?5,
                locale = ?6, timezone = ?7, updated_at = ?8, last_launched_at = ?9
             WHERE id = ?1",
            params![
                profile.id.to_string(),
                profile.name,
                selection,
                pinned,
                startup_urls,
                profile.locale,
                profile.timezone,
                time::to_text(profile.updated_at),
                profile.last_launched_at.map(time::to_text),
            ],
        )?;
        Ok(changed > 0)
    }

    /// Deletes a profile (and, via foreign keys, its accounts, factors,
    /// and recovery codes). Returns `false` when the profile does not
    /// exist. Secret values behind deleted `SecretRef`s are orphaned in
    /// the vault; the caller (controller) is responsible for purging them.
    pub fn delete_profile(&self, id: ProfileId) -> Result<bool, StorageError> {
        let changed = self.conn.execute(
            "DELETE FROM profiles WHERE id = ?1",
            params![id.to_string()],
        )?;
        Ok(changed > 0)
    }
}

fn profile_from_row(row: &Row<'_>) -> Result<BrowserProfile, StorageError> {
    let id_text: String = row.get("id")?;
    let id: ProfileId = id_text.parse().map_err(|_| StorageError::Corrupt {
        column: COL_ID,
        detail: "invalid identifier".to_owned(),
    })?;
    let selection_text: String = row.get("selection")?;
    let pinned_version: Option<String> = row.get("pinned_version")?;
    let thorium_version =
        ThoriumSelection::from_storage_parts(&selection_text, pinned_version.as_deref()).ok_or(
            StorageError::Corrupt {
                column: COL_SELECTION,
                detail: format!("unknown selection {selection_text:?}"),
            },
        )?;
    let startup_urls_text: String = row.get("startup_urls")?;
    let startup_urls: Vec<String> =
        serde_json::from_str(&startup_urls_text).map_err(|source| StorageError::Corrupt {
            column: COL_URLS,
            detail: source.to_string(),
        })?;
    let created_at_text: String = row.get("created_at")?;
    let updated_at_text: String = row.get("updated_at")?;
    let last_launched: Option<String> = row.get("last_launched_at")?;
    Ok(BrowserProfile {
        id,
        name: row.get("name")?,
        thorium_version,
        user_data_rel_path: row.get("user_data_rel_path")?,
        startup_urls,
        locale: row.get("locale")?,
        timezone: row.get("timezone")?,
        account_ids: Vec::new(),
        created_at: time::from_text(COL_CREATED, &created_at_text)?,
        updated_at: time::from_text(COL_UPDATED, &updated_at_text)?,
        last_launched_at: last_launched
            .map(|text| time::from_text(COL_LAUNCHED, &text))
            .transpose()?,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use thorium_workspace_domain::ProfileInput;

    fn temp_store(tag: &str) -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(format!("{tag}.db"));
        let store = Store::open(&path).expect("open");
        (dir, store)
    }

    pub(crate) fn sample_profile(name: &str) -> BrowserProfile {
        let input = ProfileInput {
            name: name.to_owned(),
            thorium_version: ThoriumSelection::Current,
            startup_urls: vec!["https://github.com".to_owned()],
            locale: Some("en-US".to_owned()),
            timezone: Some("America/Los_Angeles".to_owned()),
        };
        BrowserProfile::from_validated(input.validate().expect("valid input"))
    }

    #[test]
    fn profile_roundtrips_with_all_fields() {
        let (_dir, store) = temp_store("profiles");
        let mut profile = sample_profile("Test Profile A");
        profile.thorium_version = ThoriumSelection::Pinned {
            version: "M152.0.7977.55".to_owned(),
        };

        store.create_profile(&profile).expect("insert");
        let loaded = store
            .get_profile(profile.id)
            .expect("query")
            .expect("present");
        assert_eq!(loaded, profile);

        let all = store.list_profiles().expect("list");
        assert_eq!(all, vec![profile]);
    }

    #[test]
    fn missing_profile_returns_none() {
        let (_dir, store) = temp_store("missing");
        let found = store.get_profile(ProfileId::new()).expect("query");
        assert!(found.is_none());
    }

    #[test]
    fn update_changes_mutable_fields_only() {
        let (_dir, store) = temp_store("update");
        let mut profile = sample_profile("Original");
        store.create_profile(&profile).expect("insert");
        let original_created = profile.created_at;
        let original_path = profile.user_data_rel_path.clone();

        profile.name = "Renamed".to_owned();
        profile.locale = Some("pl-PL".to_owned());
        profile.timezone = None;
        profile.startup_urls.clear();
        profile.updated_at += chrono::Duration::minutes(5);
        profile.last_launched_at = Some(profile.updated_at);
        assert!(
            store.update_profile(&profile).expect("update"),
            "existing row must update"
        );

        let loaded = store
            .get_profile(profile.id)
            .expect("query")
            .expect("present");
        assert_eq!(loaded.name, "Renamed");
        assert_eq!(loaded.locale.as_deref(), Some("pl-PL"));
        assert!(loaded.timezone.is_none());
        assert!(loaded.startup_urls.is_empty());
        assert!(loaded.last_launched_at.is_some());
        assert_eq!(loaded.created_at, original_created);
        assert_eq!(loaded.user_data_rel_path, original_path);
    }

    #[test]
    fn update_missing_profile_reports_not_changed() {
        let (_dir, store) = temp_store("update-missing");
        let profile = sample_profile("Ghost");
        assert!(!store.update_profile(&profile).expect("update"));
    }

    #[test]
    fn duplicate_name_and_user_data_path_conflict() {
        let (_dir, store) = temp_store("conflict");
        let first = sample_profile("Same Name");
        store.create_profile(&first).expect("first insert");

        let mut second = sample_profile("Same Name");
        second.id = ProfileId::new();
        let error = store.create_profile(&second).expect_err("name conflict");
        assert!(
            matches!(error, StorageError::Conflict { field: "name", .. }),
            "got: {error}"
        );

        let mut third = sample_profile("Other Name");
        third.id = ProfileId::new();
        third.user_data_rel_path = first.user_data_rel_path.clone();
        let error = store.create_profile(&third).expect_err("path conflict");
        assert!(
            matches!(
                error,
                StorageError::Conflict {
                    field: "user_data_rel_path",
                    ..
                }
            ),
            "got: {error}"
        );
    }

    #[test]
    fn delete_removes_profile_and_cascades_accounts() {
        let (_dir, store) = temp_store("cascade");
        let profile = sample_profile("Doomed");
        store.create_profile(&profile).expect("insert");
        let account = crate::accounts::tests::sample_account(profile.id);
        store.create_account(&account).expect("account insert");

        assert!(store.delete_profile(profile.id).expect("delete"));
        assert!(store.get_profile(profile.id).expect("query").is_none());
        let remaining = store.list_accounts(profile.id).expect("accounts");
        assert!(remaining.is_empty(), "accounts must cascade");
        assert!(!store.delete_profile(profile.id).expect("delete again"));
    }

    #[test]
    fn list_is_ordered_by_name() {
        let (_dir, store) = temp_store("ordering");
        let mut b = sample_profile("Beta");
        b.id = ProfileId::new();
        let mut a = sample_profile("alpha");
        a.id = ProfileId::new();
        store.create_profile(&b).expect("b");
        store.create_profile(&a).expect("a");
        let names: Vec<String> = store
            .list_profiles()
            .expect("list")
            .into_iter()
            .map(|profile| profile.name)
            .collect();
        assert_eq!(names, vec!["alpha", "Beta"]);
    }
}
