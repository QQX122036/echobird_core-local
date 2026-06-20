//! Tauri IPC command layer. One module per public domain. Each
//! command is a thin shim that:
//!   1. Deserializes the input (via Tauri's auto-derive or an
//!      explicit struct).
//!   2. Calls the relevant `services::` function.
//!   3. Serializes the output (or returns the `Error` verbatim so
//!      the frontend sees the `error_code: message` string).
//!
//! No business logic lives here. If a command handler grows past
//! ~20 lines, the right move is to push the logic into a service
//! and have the command call it.

pub mod agent;
pub mod ai_career;
pub mod app;
pub mod bundled;
pub mod local_server;
pub mod models;
pub mod parasite;
pub mod secret;
pub mod ssh;
pub mod tools;

use crate::error::Error;

/// Helper: Tauri command handlers must return `Result<T, E>` where
/// `E: serde::Serialize`. We serialize our [`Error`] by reusing
/// the `Display` output (which already has the `code:` prefix the
/// frontend parses). This means the JS side sees a `string`, not a
/// structured object, which matches how `tauri::command` already
/// surfaces errors in the upstream.
pub fn ipc<T: serde::Serialize>(r: Result<T, Error>) -> std::result::Result<T, String> {
    r.map_err(|e| e.to_string())
}
