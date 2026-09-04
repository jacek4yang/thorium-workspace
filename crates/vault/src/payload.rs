//! The decrypted vault payload and its serde view.
//!
//! [`VaultEntry`] values are secret material ([`SecretBytes`], no
//! `Serialize`). Serialization happens only through [`StoredEntry`], a
//! base64 DTO that exists exclusively inside the encryption boundary and
//! never leaves the process in plaintext form.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thorium_workspace_domain::SecretRef;
use thorium_workspace_secrets::SecretBytes;

/// What kind of secret an entry holds. Only a label: the vault treats all
/// values identically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VaultEntryKind {
    /// Account password.
    Password,
    /// TOTP/HOTP seed (raw seed bytes; base32 forms are decoded by the
    /// importer before storage).
    OtpSeed,
    /// One recovery code text.
    RecoveryCode,
    /// Any other explicitly requested secret text.
    Note,
}

/// One secret stored in the vault.
///
/// `Debug` renders through [`SecretBytes`] and is always redacted.
#[derive(Clone)]
pub struct VaultEntry {
    /// Structured reference (plaintext metadata, mirrors SQLite).
    pub secret_ref: SecretRef,
    /// Kind label.
    pub kind: VaultEntryKind,
    /// Secret value.
    pub value: SecretBytes,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last modification timestamp.
    pub updated_at: DateTime<Utc>,
}

impl core::fmt::Debug for VaultEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("VaultEntry")
            .field("secret_ref", &self.secret_ref.as_str())
            .field("kind", &self.kind)
            .field("value", &"<redacted>")
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

/// The complete decrypted vault content.
#[derive(Debug, Clone)]
pub struct VaultPayload {
    /// When the vault was created.
    pub created_at: DateTime<Utc>,
    /// Last content modification.
    pub updated_at: DateTime<Utc>,
    /// All entries, ordered by reference.
    pub entries: Vec<VaultEntry>,
}

impl VaultPayload {
    /// An empty payload stamped at `now`.
    pub fn empty(now: DateTime<Utc>) -> Self {
        Self {
            created_at: now,
            updated_at: now,
            entries: Vec::new(),
        }
    }

    /// Finds the entry behind `secret_ref`, if stored.
    pub fn get(&self, secret_ref: &SecretRef) -> Option<&VaultEntry> {
        self.entries
            .iter()
            .find(|entry| entry.secret_ref == *secret_ref)
    }

    /// Inserts or replaces the entry behind its reference.
    pub fn put(&mut self, entry: VaultEntry) {
        match self
            .entries
            .iter_mut()
            .find(|existing| existing.secret_ref == entry.secret_ref)
        {
            Some(existing) => *existing = entry,
            None => self.entries.push(entry),
        }
        self.entries.sort_by(|a, b| a.secret_ref.cmp(&b.secret_ref));
    }

    /// Removes the entry behind `secret_ref`. Returns `true` when a value
    /// was removed.
    pub fn remove(&mut self, secret_ref: &SecretRef) -> bool {
        let before = self.entries.len();
        self.entries.retain(|entry| &entry.secret_ref != secret_ref);
        self.entries.len() != before
    }
}

/// Serde view of a [`VaultEntry`]. The base64 value stays confidential
/// because the whole payload is encrypted before it reaches disk.
#[derive(Serialize, Deserialize)]
struct StoredEntry {
    #[serde(rename = "ref")]
    secret_ref: String,
    kind: VaultEntryKind,
    #[serde(rename = "valueB64")]
    value_b64: String,
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
}

/// Serde view of a [`VaultPayload`].
#[derive(Serialize, Deserialize)]
struct StoredPayload {
    #[serde(rename = "createdAt")]
    created_at: String,
    #[serde(rename = "updatedAt")]
    updated_at: String,
    entries: Vec<StoredEntry>,
}

fn timestamp_to_text(value: DateTime<Utc>) -> String {
    value.to_rfc3339()
}

fn timestamp_from_text(value: &str) -> Result<DateTime<Utc>, crate::error::VaultError> {
    DateTime::parse_from_rfc3339(value)
        .map(|parsed| parsed.with_timezone(&Utc))
        .map_err(|error| crate::error::VaultError::Payload(format!("invalid timestamp: {error}")))
}

fn entry_to_stored(entry: &VaultEntry) -> StoredEntry {
    StoredEntry {
        secret_ref: entry.secret_ref.as_str().to_owned(),
        kind: entry.kind,
        value_b64: data_encoding::BASE64.encode(entry.value.expose()),
        created_at: timestamp_to_text(entry.created_at),
        updated_at: timestamp_to_text(entry.updated_at),
    }
}

fn entry_from_stored(stored: StoredEntry) -> Result<VaultEntry, crate::error::VaultError> {
    let value_b64 = stored.value_b64;
    let value = data_encoding::BASE64
        .decode(value_b64.as_bytes())
        .map_err(|error| {
            crate::error::VaultError::Payload(format!("invalid entry value: {error}"))
        })?;
    Ok(VaultEntry {
        secret_ref: stored
            .secret_ref
            .parse()
            .map_err(|_| crate::error::VaultError::Payload("invalid entry reference".to_owned()))?,
        kind: stored.kind,
        value: SecretBytes::new(&value),
        created_at: timestamp_from_text(&stored.created_at)?,
        updated_at: timestamp_from_text(&stored.updated_at)?,
    })
}

/// Serializes a payload to JSON bytes (to be encrypted immediately).
pub(crate) fn serialize_payload(
    payload: &VaultPayload,
) -> Result<Vec<u8>, crate::error::VaultError> {
    let stored = StoredPayload {
        created_at: timestamp_to_text(payload.created_at),
        updated_at: timestamp_to_text(payload.updated_at),
        entries: payload.entries.iter().map(entry_to_stored).collect(),
    };
    serde_json::to_vec(&stored)
        .map_err(|error| crate::error::VaultError::Payload(error.to_string()))
}

/// Parses a decrypted JSON payload and scrubs the plaintext buffer.
pub(crate) fn deserialize_payload(
    mut json: Vec<u8>,
) -> Result<VaultPayload, crate::error::VaultError> {
    let result = serde_json::from_slice::<StoredPayload>(&json)
        .map_err(|error| crate::error::VaultError::Payload(error.to_string()));
    crate::crypto::scrub(&mut json);
    let stored = result?;
    let entries = stored
        .entries
        .into_iter()
        .map(entry_from_stored)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(VaultPayload {
        created_at: timestamp_from_text(&stored.created_at)?,
        updated_at: timestamp_from_text(&stored.updated_at)?,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_entry(reference: &str) -> VaultEntry {
        VaultEntry {
            secret_ref: reference.parse().expect("valid ref"),
            kind: VaultEntryKind::Password,
            value: SecretBytes::new(b"synthetic-secret-value"),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn payload_json_roundtrip_preserves_entries() {
        let mut payload = VaultPayload::empty(Utc::now());
        payload.put(synthetic_entry(
            "account/11111111-1111-1111-1111-111111111111/password",
        ));
        payload.put(synthetic_entry(
            "account/22222222-2222-2222-2222-222222222222/password",
        ));

        let json = serialize_payload(&payload).expect("serialize");
        let rebuilt = deserialize_payload(json).expect("deserialize");
        assert_eq!(rebuilt.entries.len(), payload.entries.len());
        for (original, loaded) in payload.entries.iter().zip(rebuilt.entries.iter()) {
            assert_eq!(original.secret_ref, loaded.secret_ref);
            assert_eq!(original.kind, loaded.kind);
            assert_eq!(original.value, loaded.value);
        }
    }

    #[test]
    fn get_put_remove_work_by_reference() {
        let mut payload = VaultPayload::empty(Utc::now());
        let entry = synthetic_entry("account/33333333-3333-3333-3333-333333333333/password");
        let reference = entry.secret_ref.clone();
        payload.put(entry);
        assert!(payload.get(&reference).is_some());
        assert!(payload.remove(&reference));
        assert!(payload.get(&reference).is_none());
        assert!(!payload.remove(&reference));
    }

    #[test]
    fn debug_rendering_stays_redacted() {
        let mut payload = VaultPayload::empty(Utc::now());
        // An empty payload trivially leaks nothing.
        assert!(!format!("{payload:?}").contains("synthetic-secret-value"));

        payload.put(synthetic_entry(
            "account/44444444-4444-4444-4444-444444444444/password",
        ));
        let rendered = format!("{payload:?}");
        assert!(rendered.contains("<redacted>"));
        assert!(!rendered.contains("synthetic-secret-value"));
    }

    #[test]
    fn invalid_base64_values_are_rejected_as_payload_errors() {
        let json = br#"{"createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z","entries":[{"ref":"account/55555555-5555-5555-5555-555555555555/password","kind":"password","valueB64":"!!!","createdAt":"2026-01-01T00:00:00Z","updatedAt":"2026-01-01T00:00:00Z"}]}"#;
        let error = deserialize_payload(json.to_vec()).expect_err("must reject");
        assert!(matches!(error, crate::error::VaultError::Payload(_)));
    }
}
