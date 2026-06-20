//! Persistence layer. The [`Store`] trait is the only thing the
//! service layer knows about — every implementation choice (SQLite,
//! in-memory, Postgres, …) is hidden behind it.
//!
//! The concrete [`sqlite::SqliteStore`] is the production
//! implementation. Tests construct an `InMemoryStore` (see
//! `storage::memory`) so they can run without disk I/O.
//!
//! Design rules:
//!   * No `serde_json::Value` in the storage layer. Every read
//!     returns a typed value or `Error::NotFound`.
//!   * No SQL outside the SQLite module. The trait surface uses
//!     domain types only.
//!   * Migration lives in [`sqlite::migrations`]. The `Store` trait
//!     doesn't expose schema — callers don't need to know.

pub mod global;
pub mod memory;
pub mod model;
pub mod settings;
pub mod sqlite;

use crate::error::CoreResult;

pub use model::{Model, ModelPatch, ModelType, NewModel};
pub use settings::{AppSettings, SettingsPatch, ThemeMode};

/// Persistence contract used by the service layer. Every method
/// returns `Result<T>`. `NotFound` is the only error variant a
/// caller should ever need to special-case; everything else is a
/// genuine failure that should bubble up to the IPC layer.
pub trait Store: Send + Sync {
    // ─── Models ─────────────────────────────────────────────
    fn list_models(&self) -> CoreResult<Vec<Model>>;
    fn get_model(&self, internal_id: &str) -> CoreResult<Model>;
    fn insert_model(&self, new: NewModel) -> CoreResult<Model>;
    fn update_model(&self, internal_id: &str, patch: ModelPatch) -> CoreResult<Model>;
    fn delete_model(&self, internal_id: &str) -> CoreResult<bool>;

    // ─── Settings ───────────────────────────────────────────
    fn get_settings(&self) -> CoreResult<AppSettings>;
    fn save_settings(&self, patch: SettingsPatch) -> CoreResult<AppSettings>;
}
