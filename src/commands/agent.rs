//! `agent.*` IPC handlers. The real work is in
//! `services::agent::send_message`; these handlers exist to
//! bridge Tauri command input → service call.

use tauri::command;

use crate::commands::ipc;
use crate::error::Error;
use crate::services::agent as svc;
use crate::storage::global;

#[command]
pub async fn agent_send_message(
    app: tauri::AppHandle,
    input: svc::AgentSendInput,
) -> Result<(), String> {
    ipc(svc::send_message(&app, global::get(), input).await.map(|_| ()))
}

#[command]
pub fn agent_reset() -> Result<(), String> {
    ipc(reset())
}

#[command]
pub fn agent_abort() -> Result<(), String> {
    ipc(abort())
}

/// `reset` / `abort` are stateless for the clean-room build —
/// each `agent_send_message` call is a fresh request. The
/// upstream tracks conversation state, but the public IPC
/// contract is "every send is a new conversation", so there's
/// nothing to reset. We still expose the commands (returning
/// `Ok(())`) so the frontend's "Reset" / "Stop" buttons don't
/// have to handle a "command not found" error.
fn reset() -> Result<(), Error> { Ok(()) }
fn abort() -> Result<(), Error> { Ok(()) }
