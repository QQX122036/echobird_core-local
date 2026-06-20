//! Parasite (Mother Agent Connect mode) IPC. The clean-room build
//! is a stub: it accepts the inputs, validates the shape, and
//! returns `not_implemented` so the frontend surfaces a clear
//! "feature is on the roadmap" message instead of silently
//! hanging.

use serde::{Deserialize, Serialize};
use tauri::command;

use crate::commands::ipc;
use crate::error::Error;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParasiteSendRequest {
    pub agent_id: String,
    pub message: String,
}

#[command]
pub async fn parasite_send_message(
    _request: ParasiteSendRequest,
) -> Result<serde_json::Value, String> {
    ipc(Err(Error::not_implemented(
        "Parasite mode requires the proprietary Claude Code adapter",
    )))
}

#[command]
pub fn parasite_reset() -> Result<(), String> {
    ipc(Ok(()))
}

#[command]
pub fn parasite_abort() -> Result<(), String> {
    ipc(Ok(()))
}

#[command]
pub fn parasite_list_installed() -> Result<serde_json::Value, String> {
    ipc(Ok(serde_json::json!([])))
}
