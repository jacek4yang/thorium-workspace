//! Strongly-typed identifiers for domain entities.
//!
//! Identifiers are UUID v4 values wrapped in newtypes so they cannot be
//! confused with one another. They serialize transparently as strings.

use std::fmt;
use std::str::FromStr;

use uuid::Uuid;

use crate::error::DomainError;

macro_rules! define_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(
            Debug,
            Clone,
            Copy,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            serde::Serialize,
            serde::Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Generates a fresh random identifier.
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            /// Returns the underlying UUID.
            pub fn as_uuid(&self) -> Uuid {
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
                write!(f, "{}", self.0)
            }
        }

        impl FromStr for $name {
            type Err = DomainError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Uuid::parse_str(s).map(Self).map_err(|_| DomainError::InvalidId)
            }
        }
    };
}

define_id!(
    /// Identifier of a Browser Profile.
    ProfileId
);
define_id!(
    /// Identifier of an Account within a Browser Profile.
    AccountId
);
define_id!(
    /// Identifier of a second factor attached to an Account.
    FactorId
);
define_id!(
    /// Identifier of a recovery code slot attached to an Account.
    RecoveryCodeId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_roundtrip_through_strings() {
        let id = ProfileId::new();
        let text = id.to_string();
        let parsed: ProfileId = text.parse().expect("valid uuid text");
        assert_eq!(id, parsed);
    }

    #[test]
    fn ids_reject_garbage() {
        let parsed = "not-a-uuid".parse::<ProfileId>();
        assert!(parsed.is_err());
    }

    #[test]
    fn ids_are_distinct_per_call() {
        assert_ne!(ProfileId::new(), ProfileId::new());
    }

    #[test]
    fn ids_serialize_as_plain_strings() {
        let id = AccountId::new();
        let json = serde_json::to_string(&id).expect("serializable");
        assert!(json.starts_with('"'));
        assert!(json.contains(id.as_uuid().hyphenated().to_string()));
    }
}
