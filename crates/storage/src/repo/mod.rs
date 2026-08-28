//! Repositories.
//!
//! Each repository owns the SQL for one aggregate and maps rows to and from
//! domain types. They take a `&Connection` rather than owning one so a caller
//! can run several of them inside a single transaction.

mod accounts;
mod factors;
mod profiles;
mod recovery;
mod runtime;
mod settings;
mod thorium;

pub use accounts::{AccountRecord, AccountRepo};
pub use factors::SecondFactorRepo;
pub use profiles::ProfileRepo;
pub use recovery::RecoveryCodeRepo;
pub use runtime::{RuntimeRepo, RuntimeSession};
pub use settings::SettingsRepo;
pub use thorium::ThoriumRepo;
