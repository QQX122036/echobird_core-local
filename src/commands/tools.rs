//! Tool-related IPC handlers: scan, start, apply-model.

use tauri::command;

use crate::commands::ipc;
use crate::error::Error;
use crate::services::model_proxy as svc;
use crate::services::tool_installer;

#[command]
pub fn scan_tools() -> Result<Vec<tool_installer::DetectedTool>, String> {
    ipc(tool_installer::scan_tools())
}

#[command]
pub async fn start_tool(tool_id: String, start_command: Option<String>) -> Result<(), String> {
    ipc(start(tool_id, start_command).await)
}

#[command]
pub fn apply_model_to_tool(
    tool_id: String,
    model_info: svc::ApplyModelInput,
) -> Result<svc::ApplyResult, String> {
    ipc(svc::apply_model_to_tool(&tool_id, model_info))
}

#[command]
pub fn restore_tool_to_official(tool_id: String) -> Result<svc::ApplyResult, String> {
    ipc(svc::restore_tool_to_official(&tool_id))
}

/// `start` — launch a tool's binary via the OS shell. The
/// upstream distinguishes between installed and not-installed
/// tools; we mirror that. The `start_command` field in the
/// install manifest is the actual command we run; a `None` arg
/// here lets the user override.
async fn start(tool_id: String, override_cmd: Option<String>) -> Result<(), Error> {
    use std::process::Command;
    let entry = crate::services::bundled_assets::install_entry(&tool_id)?;
    let cmd = override_cmd
        .as_deref()
        .or(entry.start_command.as_deref())
        .or(entry.command.as_deref())
        .ok_or_else(|| Error::validation(format!("tool {tool_id} has no start command")))?;
    // Run detached — we don't wait for the child to exit, and
    // we don't pipe stdio. If the tool crashes, the OS will
    // surface the error to the user; we just want to fire it.
    Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .spawn()
        .map_err(|e| Error::network(format!("failed to start {tool_id}: {e}")))?;
    Ok(())
}
