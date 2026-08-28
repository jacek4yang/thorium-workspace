//! Application services.
//!
//! The controller is where the platform-independent pieces meet: it owns the
//! portable workspace layout, the metadata database, the vault session, the
//! Thorium manager and the running browser sessions, and it exposes one
//! coherent API for the Tauri command layer to call.
//!
//! Everything above this layer (Tauri commands, the React frontend) is
//! presentation. Everything below is a single concern. No policy about
//! encryption, persistence or process lifetime lives outside this crate and the
//! ones it wraps.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod accounts;
pub mod backup;
pub mod bootstrap;
pub mod clipboard;
pub mod diagnostics;
pub mod error;
pub mod logging;
pub mod profiles;
pub mod thorium;
pub mod vault;
pub mod workspace;

pub use bootstrap::{Bootstrap, BootstrapReport, WorkspaceLayout, WorkspacePaths};
pub use clipboard::{ClipboardGuard, CopyKind};
pub use diagnostics::{DiagnosticReport, DiagnosticsBuilder};
pub use error::{AppError, AppResult};
pub use workspace::Workspace;
