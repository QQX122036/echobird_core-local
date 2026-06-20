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
pub fn parasite_reset(_agent_id: String) -> Result<(), String> {
    // Shape-compatible stub — see parasite_abort.
    ipc(Ok(()))
}

#[command]
pub fn parasite_abort(_agent_id: String) -> Result<bool, String> {
    // Shape-compatible stub. The frontend types this as
    // `Promise<boolean>` and passes `agentId`. The
    // previous stub took zero args and returned `()`, so
    // the IPC threw "unexpected argument agentId" and the
    // page's `.then((ok) => ...)` got `undefined`. Now we
    // accept the agentId and return `false` ("no active
    // send to abort" — the clean-room build never started
    // one).
    ipc(Ok(false))
}

#[command]
pub fn parasite_list_installed() -> Result<serde_json::Value, String> {
    ipc(Ok(serde_json::json!([])))
}
