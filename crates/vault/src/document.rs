//! The decrypted vault payload.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tw_domain::{SecretRef, Timestamp};
use tw_secrets::SecretString;

/// What a stored secret is, so the UI can label a reveal and diagnostics can
/// count secrets by kind without reading any of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    /// An account password.
    Password,
    /// A shared OTP secret, stored as the Base32 text from the `otpauth://` URI.
    OtpSeed,
    /// A single recovery/backup code.
    RecoveryCode,
    /// Arbitrary secret text the user chose to protect.
    Note,
}

impl SecretKind {
    /// A short label for diagnostics and the UI.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::OtpSeed => "OTP secret",
            Self::RecoveryCode => "recovery code",
            Self::Note => "secure note",
        }
    }
}

/// One protected value inside the vault.
///
/// `value` serializes as plaintext *only* within the vault payload, which is
/// itself the AEAD plaintext. Everywhere else it renders as `[redacted]`
/// through [`SecretString`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretRecord {
    /// What this secret is.
    pub kind: SecretKind,
    /// The protected value.
    #[serde(with = "secret_string_plaintext")]
    pub value: SecretString,
    /// When it was first stored.
    pub created_at: Timestamp,
    /// When it was last replaced.
    pub updated_at: Timestamp,
}

/// Serializes a [`SecretString`] as raw text.
///
/// Used **only** for the vault payload, which is encrypted before it touches the
/// disk. `SecretString`'s own `Serialize` deliberately emits `[redacted]`, which
/// would silently destroy the user's data here.
mod secret_string_plaintext {
    use serde::{Deserialize, Deserializer, Serializer};
    use tw_secrets::SecretString;

    pub(super) fn serialize<S: Serializer>(value: &SecretString, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(value.expose())
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SecretString, D::Error> {
        String::deserialize(d).map(SecretString::new)
    }
}

/// The decrypted contents of a vault.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultDocument {
    /// Payload schema version, independent of the file format version.
    #[serde(default = "default_payload_version")]
    pub payload_version: u32,
    /// Every secret, keyed by the reference stored in the metadata database.
    #[serde(default)]
    pub secrets: BTreeMap<SecretRef, SecretRecord>,
}

fn default_payload_version() -> u32 {
    1
}

/// Current payload schema version.
pub const PAYLOAD_VERSION: u32 = 1;

impl VaultDocument {
    /// An empty document at the current payload version.
    #[must_use]
    pub fn new() -> Self {
        Self {
            payload_version: PAYLOAD_VERSION,
            secrets: BTreeMap::new(),
        }
    }

    /// Stores a new secret and returns its reference.
    pub fn insert(&mut self, kind: SecretKind, value: SecretString) -> SecretRef {
        let reference = SecretRef::new();
        let now = Timestamp::now();
        self.secrets.insert(
            reference,
            SecretRecord {
                kind,
                value,
                created_at: now,
                updated_at: now,
            },
        );
        reference
    }

    /// Replaces the value behind an existing reference, preserving its creation
    /// time. Returns `false` when the reference is unknown.
    pub fn replace(&mut self, reference: SecretRef, value: SecretString) -> bool {
        match self.secrets.get_mut(&reference) {
            Some(record) => {
                record.value = value;
                record.updated_at = Timestamp::now();
                true
            }
            None => false,
        }
    }

    /// Reads a secret.
    #[must_use]
    pub fn get(&self, reference: SecretRef) -> Option<&SecretRecord> {
        self.secrets.get(&reference)
    }

    /// Removes a secret. Returns `true` when something was removed.
    pub fn remove(&mut self, reference: SecretRef) -> bool {
        self.secrets.remove(&reference).is_some()
    }

    /// Number of stored secrets.
    #[must_use]
    pub fn len(&self) -> usize {
        self.secrets.len()
    }

    /// Whether the vault holds no secrets.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.secrets.is_empty()
    }

    /// Counts secrets by kind. Safe to show in diagnostics.
    #[must_use]
    pub fn counts_by_kind(&self) -> BTreeMap<SecretKind, usize> {
        let mut counts = BTreeMap::new();
        for record in self.secrets.values() {
            *counts.entry(record.kind).or_insert(0) += 1;
        }
        counts
    }

    /// Drops every secret whose reference is not in `live`.
    ///
    /// Called after deleting accounts or profiles so an orphaned password cannot
    /// linger in the vault indefinitely. Returns how many were removed.
    pub fn retain_only(&mut self, live: &std::collections::BTreeSet<SecretRef>) -> usize {
        let before = self.secrets.len();
        self.secrets.retain(|reference, _| live.contains(reference));
        before - self.secrets.len()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn insert_get_replace_and_remove() {
        let mut doc = VaultDocument::new();
        let reference = doc.insert(SecretKind::Password, SecretString::new("hunter2"));
        assert_eq!(doc.len(), 1);
        assert_eq!(doc.get(reference).expect("present").value.expose(), "hunter2");

        assert!(doc.replace(reference, SecretString::new("hunter3")));
        assert_eq!(doc.get(reference).expect("present").value.expose(), "hunter3");
        assert_eq!(
            doc.get(reference).expect("present").kind,
            SecretKind::Password,
            "replacing a value keeps its kind"
        );

        assert!(!doc.replace(SecretRef::new(), SecretString::new("x")));
        assert!(doc.remove(reference));
        assert!(!doc.remove(reference));
        assert!(doc.is_empty());
    }

    #[test]
    fn the_payload_round_trips_through_json_with_values_intact() {
        let mut doc = VaultDocument::new();
        let pw = doc.insert(SecretKind::Password, SecretString::new("hunter2"));
        let seed = doc.insert(SecretKind::OtpSeed, SecretString::new("JBSWY3DPEHPK3PXP"));

        let json = serde_json::to_string(&doc).expect("serialize");
        // The payload is the AEAD plaintext, so it must contain the real values.
        assert!(json.contains("hunter2"));
        assert!(
            !json.contains("[redacted]"),
            "the payload must not redact itself into oblivion"
        );

        let restored: VaultDocument = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(restored.get(pw).expect("present").value.expose(), "hunter2");
        assert_eq!(
            restored.get(seed).expect("present").value.expose(),
            "JBSWY3DPEHPK3PXP"
        );
        assert_eq!(restored.payload_version, PAYLOAD_VERSION);
    }

    #[test]
    fn debug_output_of_the_document_reveals_nothing() {
        let mut doc = VaultDocument::new();
        doc.insert(SecretKind::Password, SecretString::new("hunter2"));
        let rendered = format!("{doc:#?}");
        assert!(!rendered.contains("hunter2"), "{rendered}");
        assert!(rendered.contains("[redacted]"));
    }

    #[test]
    fn counts_by_kind_summarises_without_exposing_values() {
        let mut doc = VaultDocument::new();
        doc.insert(SecretKind::Password, SecretString::new("a-very-secret-value"));
        doc.insert(SecretKind::RecoveryCode, SecretString::new("code-1"));
        doc.insert(SecretKind::RecoveryCode, SecretString::new("code-2"));
        let counts = doc.counts_by_kind();
        assert_eq!(counts.get(&SecretKind::Password), Some(&1));
        assert_eq!(counts.get(&SecretKind::RecoveryCode), Some(&2));
        assert_eq!(counts.get(&SecretKind::OtpSeed), None);
        assert!(!format!("{counts:?}").contains("a-very-secret-value"));
    }

    #[test]
    fn orphaned_secrets_are_collected() {
        let mut doc = VaultDocument::new();
        let keep = doc.insert(SecretKind::Password, SecretString::new("keep me"));
        doc.insert(SecretKind::Password, SecretString::new("orphan"));
        let live: BTreeSet<SecretRef> = [keep].into_iter().collect();
        assert_eq!(doc.retain_only(&live), 1);
        assert_eq!(doc.len(), 1);
        assert!(doc.get(keep).is_some());
    }

    #[test]
    fn a_payload_missing_optional_fields_still_loads() {
        let doc: VaultDocument = serde_json::from_str("{}").expect("deserialize");
        assert_eq!(doc.payload_version, 1);
        assert!(doc.is_empty());
    }
}
