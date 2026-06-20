//! `models.*` IPC handlers. Mirror the public frontend's
//! `src/api/models.ts`.

use tauri::command;

use crate::commands::ipc;
use crate::services::models as svc;
use crate::storage::global;

#[command]
pub fn get_models() -> Result<Vec<svc::ModelDto>, String> {
    ipc(svc::list_models(global::get()))
}

#[command]
pub fn add_model(input: svc::NewModelDto) -> Result<svc::ModelDto, String> {
    ipc(svc::add_model(global::get(), input))
}

#[command]
pub fn update_model(
    internal_id: String,
    updates: serde_json::Value,
) -> Result<svc::ModelDto, String> {
    ipc(svc::update_model_from_json(global::get(), &internal_id, updates))
}

#[command]
pub fn delete_model(internal_id: String) -> Result<bool, String> {
    ipc(svc::delete_model(global::get(), &internal_id))
}

#[command]
pub async fn test_model(
    internal_id: String,
    prompt: String,
    protocol: Option<String>,
) -> Result<svc::TestModelResult, String> {
    ipc(svc::test_model(
        global::get(),
        &internal_id,
        &prompt,
        protocol.as_deref().unwrap_or("openai"),
    )
    .await)
}

#[command]
pub async fn ping_model(internal_id: String) -> Result<svc::PingResult, String> {
    ipc(svc::ping_model(global::get(), &internal_id).await)
}

#[command]
pub fn is_key_destroyed(_internal_id: String) -> Result<bool, String> {
    ipc(svc::is_key_destroyed(global::get(), &_internal_id))
}

/// `get_model_directory` — returns the curated right-panel
/// directory. The upstream reads from a remote URL with a local
/// JSON cache; for the clean-room build we ship the bundled
/// `src/data/modelDirectory.json` (via the binary's
/// `include_str!`). The frontend already has this same JSON
/// bundled and uses it as the offline fallback, so we return
/// `None` and let the frontend fall through to its own copy.
#[command]
pub fn get_model_directory() -> Result<Option<serde_json::Value>, String> {
    ipc(Ok(None))
}
