//! Browser profile metadata.

use rusqlite::{Connection, OptionalExtension, Row, params};
use tw_domain::{AccountId, BrowserProfile, LocaleTag, ProfileId, ThoriumSelection, TimeZoneId, Timestamp};

use crate::error::{StorageError, StorageResult};

/// Reads and writes browser profiles.
pub struct ProfileRepo;

const SELECT: &str = "SELECT id, name, thorium_mode, thorium_version, user_data_dir, startup_urls, \
                      locale, timezone, notes, network_route_id, created_at, updated_at \
                      FROM browser_profiles";

impl ProfileRepo {
    /// Inserts a profile.
    ///
    /// `user_data_dir` is derived from the profile id and is `UNIQUE` in the
    /// schema, so two profiles can never share browser state.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Conflict`] on a duplicate id or directory.
    pub fn insert(conn: &Connection, profile: &BrowserProfile) -> StorageResult<()> {
        conn.execute(
            "INSERT INTO browser_profiles (id, name, thorium_mode, thorium_version, user_data_dir, \
             startup_urls, locale, timezone, notes, network_route_id, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                profile.id.to_string(),
                profile.name,
                profile.thorium.discriminant(),
                profile.thorium.pinned_version(),
                profile.user_data_dir_name(),
                serde_json::to_string(&profile.startup_urls).unwrap_or_else(|_| "[]".to_owned()),
                profile.locale.as_str(),
                profile.timezone.as_str(),
                profile.notes,
                profile.network_route_id,
                profile.created_at.as_unix_seconds(),
                profile.updated_at.as_unix_seconds(),
            ],
        )?;
        Ok(())
    }

    /// Updates a profile's configuration.
    ///
    /// The `User Data` directory is intentionally not updatable: it is derived
    /// from the immutable id, and moving it would orphan the browser's state.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] when no such profile exists.
    pub fn update(conn: &Connection, profile: &BrowserProfile) -> StorageResult<()> {
        let changed = conn.execute(
            "UPDATE browser_profiles SET name = ?2, thorium_mode = ?3, thorium_version = ?4, \
             startup_urls = ?5, locale = ?6, timezone = ?7, notes = ?8, updated_at = ?9 WHERE id = ?1",
            params![
                profile.id.to_string(),
                profile.name,
                profile.thorium.discriminant(),
                profile.thorium.pinned_version(),
                serde_json::to_string(&profile.startup_urls).unwrap_or_else(|_| "[]".to_owned()),
                profile.locale.as_str(),
                profile.timezone.as_str(),
                profile.notes,
                profile.updated_at.as_unix_seconds(),
            ],
        )?;
        if changed == 0 {
            return Err(StorageError::not_found("browser profile", profile.id));
        }
        Ok(())
    }

    /// Deletes a profile row. Does not touch its `User Data` directory; that is
    /// the caller's explicit decision.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] when no such profile exists.
    pub fn delete(conn: &Connection, id: ProfileId) -> StorageResult<()> {
        let changed = conn.execute(
            "DELETE FROM browser_profiles WHERE id = ?1",
            params![id.to_string()],
        )?;
        if changed == 0 {
            return Err(StorageError::not_found("browser profile", id));
        }
        Ok(())
    }

    /// Fetches one profile, with its account associations.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] when no such profile exists.
    pub fn get(conn: &Connection, id: ProfileId) -> StorageResult<BrowserProfile> {
        let row = conn
            .query_row(
                &format!("{SELECT} WHERE id = ?1"),
                params![id.to_string()],
                map_profile,
            )
            .optional()?;
        let mut profile = row.ok_or_else(|| StorageError::not_found("browser profile", id))?;
        profile.account_ids = Self::account_ids(conn, id)?;
        Ok(profile)
    }

    /// Lists every profile by name, with account associations.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn list(conn: &Connection) -> StorageResult<Vec<BrowserProfile>> {
        let mut stmt = conn.prepare(&format!("{SELECT} ORDER BY name COLLATE NOCASE"))?;
        let rows = stmt.query_map([], map_profile)?;
        let mut profiles = Vec::new();
        for profile in rows {
            let mut profile = profile?;
            profile.account_ids = Self::account_ids(conn, profile.id)?;
            profiles.push(profile);
        }
        Ok(profiles)
    }

    /// The `User Data` directory name stored for a profile.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::NotFound`] when no such profile exists.
    pub fn user_data_dir_name(conn: &Connection, id: ProfileId) -> StorageResult<String> {
        conn.query_row(
            "SELECT user_data_dir FROM browser_profiles WHERE id = ?1",
            params![id.to_string()],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| StorageError::not_found("browser profile", id))
    }

    /// Replaces a profile's account associations, preserving the given order.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Conflict`] when an account id does not exist.
    pub fn set_accounts(
        conn: &mut Connection,
        profile_id: ProfileId,
        account_ids: &[AccountId],
    ) -> StorageResult<()> {
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM profile_accounts WHERE profile_id = ?1",
            params![profile_id.to_string()],
        )?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO profile_accounts (profile_id, account_id, position) VALUES (?1, ?2, ?3)",
            )?;
            for (position, account_id) in account_ids.iter().enumerate() {
                stmt.execute(params![
                    profile_id.to_string(),
                    account_id.to_string(),
                    i64::try_from(position).unwrap_or(i64::MAX)
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The accounts associated with a profile, in stored order.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn account_ids(conn: &Connection, profile_id: ProfileId) -> StorageResult<Vec<AccountId>> {
        let mut stmt =
            conn.prepare("SELECT account_id FROM profile_accounts WHERE profile_id = ?1 ORDER BY position")?;
        let rows = stmt.query_map(params![profile_id.to_string()], |row| row.get::<_, String>(0))?;
        let mut ids = Vec::new();
        for value in rows {
            ids.push(
                value?
                    .parse()
                    .map_err(|_| StorageError::Corrupt("a stored account id is not a UUID".to_owned()))?,
            );
        }
        Ok(ids)
    }

    /// Profiles pinned to a specific Thorium version.
    ///
    /// Used to refuse deleting a version something still depends on.
    ///
    /// # Errors
    ///
    /// Returns [`StorageError::Query`] on a read failure.
    pub fn pinned_to_version(conn: &Connection, version: &str) -> StorageResult<Vec<ProfileId>> {
        let mut stmt = conn.prepare(
            "SELECT id FROM browser_profiles WHERE thorium_mode = 'pinned' AND thorium_version = ?1",
        )?;
        let rows = stmt.query_map(params![version], |row| row.get::<_, String>(0))?;
        let mut ids = Vec::new();
        for value in rows {
            ids.push(
                value?
                    .parse()
                    .map_err(|_| StorageError::Corrupt("a stored profile id is not a UUID".to_owned()))?,
            );
        }
        Ok(ids)
    }
}

fn map_profile(row: &Row<'_>) -> rusqlite::Result<BrowserProfile> {
    let id: String = row.get(0)?;
    let mode: String = row.get(2)?;
    let version: Option<String> = row.get(3)?;
    let startup_json: String = row.get(5)?;
    let locale: String = row.get(6)?;
    let timezone: String = row.get(7)?;
    Ok(BrowserProfile {
        id: id
            .parse()
            .map_err(|_| bad_column(0, "profile id is not a UUID"))?,
        name: row.get(1)?,
        thorium: ThoriumSelection::from_parts(&mode, version.as_deref())
            .map_err(|_| bad_column(2, "unknown Thorium selection"))?,
        // A malformed URL list must not make the profile unopenable: the user
        // can still fix it in the UI, which is impossible if loading fails.
        startup_urls: serde_json::from_str(&startup_json).unwrap_or_default(),
        locale: LocaleTag::parse(&locale).unwrap_or_default(),
        timezone: TimeZoneId::parse(&timezone).unwrap_or_default(),
        account_ids: Vec::new(),
        notes: row.get(8)?,
        network_route_id: row.get(9)?,
        created_at: Timestamp::from_unix_seconds(row.get(10)?),
        updated_at: Timestamp::from_unix_seconds(row.get(11)?),
    })
}

fn bad_column(index: usize, message: &'static str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(index, rusqlite::types::Type::Text, message.into())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{AccountRepo, Database};

    pub(crate) fn sample_profile(id: ProfileId, name: &str) -> BrowserProfile {
        let now = Timestamp::from_unix_seconds(1_700_000_000);
        BrowserProfile {
            id,
            name: name.to_owned(),
            thorium: ThoriumSelection::Current,
            startup_urls: vec!["https://example.test/".to_owned()],
            locale: LocaleTag::parse("pl-PL").expect("locale"),
            timezone: TimeZoneId::parse("Europe/Warsaw").expect("timezone"),
            account_ids: Vec::new(),
            notes: "notes".to_owned(),
            network_route_id: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn sample_account(name: &str) -> tw_domain::Account {
        let now = Timestamp::from_unix_seconds(1_700_000_000);
        tw_domain::Account {
            id: AccountId::new(),
            display_name: name.to_owned(),
            service: tw_domain::ServiceKind::GitHub,
            username: None,
            email: None,
            login_url: None,
            tags: Vec::new(),
            notes: String::new(),
            password_ref: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn profiles_round_trip_with_every_field() {
        let db = Database::open_in_memory().expect("open");
        let profile = sample_profile(ProfileId::new(), "Work");
        ProfileRepo::insert(db.connection(), &profile).expect("insert");
        assert_eq!(
            ProfileRepo::get(db.connection(), profile.id).expect("get"),
            profile
        );
    }

    #[test]
    fn a_pinned_version_round_trips() {
        let db = Database::open_in_memory().expect("open");
        let mut profile = sample_profile(ProfileId::new(), "Pinned");
        profile.thorium = ThoriumSelection::Pinned("M152.0.7977.55".to_owned());
        ProfileRepo::insert(db.connection(), &profile).expect("insert");
        let loaded = ProfileRepo::get(db.connection(), profile.id).expect("get");
        assert_eq!(
            loaded.thorium,
            ThoriumSelection::Pinned("M152.0.7977.55".to_owned())
        );
        assert_eq!(
            ProfileRepo::pinned_to_version(db.connection(), "M152.0.7977.55").expect("query"),
            vec![profile.id]
        );
        assert!(
            ProfileRepo::pinned_to_version(db.connection(), "M999")
                .expect("query")
                .is_empty()
        );
    }

    #[test]
    fn each_profile_gets_its_own_user_data_directory() {
        let db = Database::open_in_memory().expect("open");
        let a = sample_profile(ProfileId::new(), "A");
        let b = sample_profile(ProfileId::new(), "B");
        ProfileRepo::insert(db.connection(), &a).expect("insert a");
        ProfileRepo::insert(db.connection(), &b).expect("insert b");
        let dir_a = ProfileRepo::user_data_dir_name(db.connection(), a.id).expect("dir a");
        let dir_b = ProfileRepo::user_data_dir_name(db.connection(), b.id).expect("dir b");
        assert_ne!(dir_a, dir_b);
        assert_eq!(dir_a, a.user_data_dir_name());
    }

    #[test]
    fn renaming_a_profile_does_not_move_its_user_data() {
        let db = Database::open_in_memory().expect("open");
        let mut profile = sample_profile(ProfileId::new(), "Before");
        ProfileRepo::insert(db.connection(), &profile).expect("insert");
        let before = ProfileRepo::user_data_dir_name(db.connection(), profile.id).expect("dir");
        profile.name = "After".to_owned();
        ProfileRepo::update(db.connection(), &profile).expect("update");
        assert_eq!(
            ProfileRepo::user_data_dir_name(db.connection(), profile.id).expect("dir"),
            before
        );
        assert_eq!(
            ProfileRepo::get(db.connection(), profile.id).expect("get").name,
            "After"
        );
    }

    #[test]
    fn account_associations_are_ordered_and_replaceable() {
        let mut db = Database::open_in_memory().expect("open");
        let profile = sample_profile(ProfileId::new(), "Work");
        ProfileRepo::insert(db.connection(), &profile).expect("insert");
        let a = sample_account("A");
        let b = sample_account("B");
        let c = sample_account("C");
        for account in [&a, &b, &c] {
            AccountRepo::insert(db.connection(), account).expect("insert account");
        }

        ProfileRepo::set_accounts(db.connection_mut(), profile.id, &[c.id, a.id]).expect("link");
        assert_eq!(
            ProfileRepo::account_ids(db.connection(), profile.id).expect("read"),
            vec![c.id, a.id]
        );

        ProfileRepo::set_accounts(db.connection_mut(), profile.id, &[b.id]).expect("relink");
        assert_eq!(
            ProfileRepo::account_ids(db.connection(), profile.id).expect("read"),
            vec![b.id]
        );

        ProfileRepo::set_accounts(db.connection_mut(), profile.id, &[]).expect("unlink");
        assert!(
            ProfileRepo::account_ids(db.connection(), profile.id)
                .expect("read")
                .is_empty()
        );
    }

    #[test]
    fn linking_a_missing_account_is_refused_and_rolls_back() {
        let mut db = Database::open_in_memory().expect("open");
        let profile = sample_profile(ProfileId::new(), "Work");
        ProfileRepo::insert(db.connection(), &profile).expect("insert");
        let good = sample_account("Good");
        AccountRepo::insert(db.connection(), &good).expect("insert account");
        ProfileRepo::set_accounts(db.connection_mut(), profile.id, &[good.id]).expect("link");

        let result = ProfileRepo::set_accounts(db.connection_mut(), profile.id, &[good.id, AccountId::new()]);
        assert!(result.is_err());
        assert_eq!(
            ProfileRepo::account_ids(db.connection(), profile.id).expect("read"),
            vec![good.id],
            "the failed call must not have cleared the existing links"
        );
    }

    #[test]
    fn deleting_an_account_removes_it_from_every_profile() {
        let mut db = Database::open_in_memory().expect("open");
        let profile = sample_profile(ProfileId::new(), "Work");
        ProfileRepo::insert(db.connection(), &profile).expect("insert");
        let account = sample_account("Doomed");
        AccountRepo::insert(db.connection(), &account).expect("insert");
        ProfileRepo::set_accounts(db.connection_mut(), profile.id, &[account.id]).expect("link");
        AccountRepo::delete(db.connection(), account.id).expect("delete");
        assert!(
            ProfileRepo::account_ids(db.connection(), profile.id)
                .expect("read")
                .is_empty()
        );
    }

    #[test]
    fn a_corrupt_startup_url_list_does_not_make_the_profile_unloadable() {
        let db = Database::open_in_memory().expect("open");
        let profile = sample_profile(ProfileId::new(), "Broken");
        ProfileRepo::insert(db.connection(), &profile).expect("insert");
        db.connection()
            .execute(
                "UPDATE browser_profiles SET startup_urls = 'not json', locale = 'zz_ZZ', \
                 timezone = 'Mars/Olympus' WHERE id = ?1",
                params![profile.id.to_string()],
            )
            .expect("corrupt");
        let loaded = ProfileRepo::get(db.connection(), profile.id).expect("still loads");
        assert!(loaded.startup_urls.is_empty());
        assert_eq!(loaded.locale, LocaleTag::default());
        assert_eq!(loaded.timezone, TimeZoneId::default());
    }

    #[test]
    fn missing_profiles_are_reported() {
        let db = Database::open_in_memory().expect("open");
        let id = ProfileId::new();
        assert!(matches!(
            ProfileRepo::get(db.connection(), id),
            Err(StorageError::NotFound { .. })
        ));
        assert!(matches!(
            ProfileRepo::delete(db.connection(), id),
            Err(StorageError::NotFound { .. })
        ));
        assert!(matches!(
            ProfileRepo::user_data_dir_name(db.connection(), id),
            Err(StorageError::NotFound { .. })
        ));
    }

    #[test]
    fn the_reserved_network_route_column_is_never_populated_by_v1() {
        let db = Database::open_in_memory().expect("open");
        let profile = sample_profile(ProfileId::new(), "Work");
        ProfileRepo::insert(db.connection(), &profile).expect("insert");
        assert_eq!(
            ProfileRepo::get(db.connection(), profile.id)
                .expect("get")
                .network_route_id,
            None
        );
    }
}
