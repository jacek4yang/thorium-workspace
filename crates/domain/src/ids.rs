//! Strongly typed identifiers.
//!
//! Each identifier is a distinct type so a profile id can never be passed where
//! an account id is expected, and every id renders as a plain UUID string in
//! storage and across the Tauri boundary.

use core::fmt;
use core::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! uuid_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generates a new random identifier.
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Wraps an existing UUID.
            #[must_use]
            pub const fn from_uuid(value: Uuid) -> Self {
                Self(value)
            }

            /// Returns the underlying UUID.
            #[must_use]
            pub const fn as_uuid(&self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(&self.0, f)
            }
        }

        impl FromStr for $name {
            type Err = uuid::Error;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self)
            }
        }
    };
}

uuid_newtype!(
    /// Identifies a Browser Profile.
    ProfileId
);
uuid_newtype!(
    /// Identifies an Account.
    AccountId
);
uuid_newtype!(
    /// Identifies a second factor attached to an Account.
    FactorId
);
uuid_newtype!(
    /// Identifies a single recovery code.
    RecoveryCodeId
);
uuid_newtype!(
    /// References a secret held in the vault.
    ///
    /// The metadata database stores only this reference; the protected value
    /// itself never leaves the encrypted vault file.
    SecretRef
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_through_strings() {
        let id = ProfileId::new();
        let parsed: ProfileId = id.to_string().parse().expect("parse");
        assert_eq!(id, parsed);
    }

    #[test]
    fn ids_serialize_as_bare_strings() {
        let id = AccountId::new();
        let json = serde_json::to_string(&id).expect("serialize");
        assert_eq!(json, format!("\"{id}\""));
    }

    #[test]
    fn distinct_ids_do_not_collide() {
        assert_ne!(ProfileId::new(), ProfileId::new());
    }
}
