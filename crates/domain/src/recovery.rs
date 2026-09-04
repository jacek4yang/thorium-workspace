//! Recovery code metadata.
//!
//! Recovery code values are secrets: each value lives in the encrypted
//! vault behind a [`crate::SecretRef`]. This record tracks only the
//! non-secret slot metadata: order, used/unused state, and the timestamp
//! when the user marked the code used.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::ids::{AccountId, RecoveryCodeId};
use crate::secret_ref::SecretRef;

/// Metadata record for one recovery code slot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCode {
    /// Unique identifier.
    pub id: RecoveryCodeId,
    /// Owning account.
    pub account_id: AccountId,
    /// Display order within the account (0-based).
    pub position: u32,
    /// Whether the user has marked this code used.
    pub used: bool,
    /// When the user marked this code used, if applicable.
    pub marked_used_at: Option<DateTime<Utc>>,
    /// Reference to the encrypted code value in the vault.
    pub secret_ref: SecretRef,
}

impl RecoveryCode {
    /// Marks this code used at the given time (idempotent).
    pub fn mark_used(&mut self, at: DateTime<Utc>) {
        if !self.used {
            self.used = true;
            self.marked_used_at = Some(at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RecoveryCode {
        RecoveryCode {
            id: RecoveryCodeId::new(),
            account_id: AccountId::new(),
            position: 0,
            used: false,
            marked_used_at: None,
            secret_ref: SecretRef::for_recovery_code(&RecoveryCodeId::new()),
        }
    }

    #[test]
    fn mark_used_is_idempotent_and_keeps_first_timestamp() {
        let mut code = sample();
        let first = Utc::now();
        code.mark_used(first);
        assert!(code.used);
        assert_eq!(code.marked_used_at, Some(first));

        let second = first + chrono::Duration::seconds(10);
        code.mark_used(second);
        assert_eq!(code.marked_used_at, Some(first));
    }

    #[test]
    fn recovery_record_carries_no_secret_value() {
        // The record type has no field for the code text itself; the value
        // only exists behind secret_ref in the vault. This test documents
        // that invariant structurally by roundtripping through serde and
        // confirming the serialized form contains a ref, not a code.
        let code = sample();
        let json = serde_json::to_string(&code).expect("serializable");
        assert!(json.contains("secretRef"));
        assert!(!json.to_lowercase().contains("value\":"));
    }
}
