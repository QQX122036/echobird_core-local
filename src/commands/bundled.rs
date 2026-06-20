//! Bundled-asset IPC handlers. The public frontend treats the
//! install manifest as the source of truth for which tools ship
//! in the binary; we read it from the registered `BundledAssets`
//! table the thin shell provided.

use tauri::command;

use crate::commands::ipc;
use crate::services::bundled_assets;

#[command]
pub fn get_install_index() -> Result<serde_json::Value, String> {
    ipc(bundled_assets::install_index()
        .map(|i| serde_json::to_value(i).unwrap_or(serde_json::Value::Null)))
}

/// `get_store_models` — community GGUF store. The upstream
/// fetches a remote JSON + has a local cache; the clean-room
/// build returns an empty list. The frontend treats `[]` as "no
/// store items available" and renders a placeholder.
#[command]
pub fn get_store_models() -> Result<serde_json::Value, String> {
    ipc(Ok(serde_json::json!([])))
}
