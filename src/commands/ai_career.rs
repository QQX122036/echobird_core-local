//! AI Career / Mother Agent hints IPC. All clean-room stubs that
//! return shape-compatible empty data so the UI's heatmap and
//! hints panels don't crash. A future build can re-implement
//! these against the same SQLite store.

use tauri::command;

use crate::commands::ipc;


#[command]
pub fn ai_career_heatmap() -> Result<serde_json::Value, String> {
    ipc(Ok(serde_json::json!({
        "days": [],
        "byFamily": {},
    })))
}

#[command]
pub fn ai_career_family_history(_family: String) -> Result<serde_json::Value, String> {
    ipc(Ok(serde_json::json!({
        "items": [],
    })))
}

#[command]
pub fn ai_career_token_bytes() -> Result<serde_json::Value, String> {
    ipc(Ok(serde_json::json!({
        "bytes": 0,
    })))
}

#[command]
pub fn get_mother_hints() -> Result<Vec<String>, String> {
    ipc(Ok::<Vec<String>, _>(Vec::new()))
}
