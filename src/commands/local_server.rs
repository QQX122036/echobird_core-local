//! Local LLM server (llama-server / ollama) IPC. The clean-room
//! build is a comprehensive stub. The shape of every command
//! matches the upstream; the implementation is "not started"
//! or "no local LLM installed" as appropriate. The frontend
//! already handles the `not_implemented` prefix as a graceful
//! "Local Server page is read-only" state.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::command;

use crate::commands::ipc;
use crate::error::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmServerInfo {
    pub running: bool,
    pub port: u32,
    pub model_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmCustomCommand {
    pub command: String,
}

static CUSTOM_COMMAND: once_cell::sync::Lazy<Arc<Mutex<Option<String>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));

static MODELS_DIRS: once_cell::sync::Lazy<Arc<Mutex<Vec<PathBuf>>>> =
    once_cell::sync::Lazy::new(|| {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        Arc::new(Mutex::new(vec![home.join("models")]))
    });

#[command]
pub fn start_llm_server() -> Result<LlmServerInfo, String> {
    ipc(Err(Error::not_implemented(
        "local LLM server is on the clean-room roadmap; \
         the proprietary build shells out to llama-server / ollama",
    )))
}

#[command]
pub fn stop_llm_server() -> Result<(), String> {
    ipc(Ok(()))
}

#[command]
pub fn get_llm_server_info() -> Result<LlmServerInfo, String> {
    ipc(Ok(LlmServerInfo {
        running: false,
        port: 0,
        model_name: String::new(),
        pid: None,
        api_key: String::new(),
    }))
}

#[command]
pub fn get_llm_server_logs(_lines: u32) -> Result<String, String> {
    ipc(Ok(String::new()))
}

#[command]
pub fn get_llm_default_command() -> Result<String, String> {
    ipc(Ok(String::from("llama-server -m {model} --port {port}")))
}

#[command]
pub fn get_llm_custom_command() -> Result<Option<LlmCustomCommand>, String> {
    ipc(Ok(CUSTOM_COMMAND.lock().as_ref().map(|c| LlmCustomCommand {
        command: c.clone(),
    })))
}

#[command]
pub fn set_llm_custom_command(command: String) -> Result<(), String> {
    *CUSTOM_COMMAND.lock() = Some(command);
    ipc(Ok(()))
}

#[command]
pub fn clear_llm_custom_command() -> Result<(), String> {
    *CUSTOM_COMMAND.lock() = None;
    ipc(Ok(()))
}

#[command]
pub fn add_models_dir(path: String) -> Result<(), String> {
    MODELS_DIRS.lock().push(PathBuf::from(path));
    ipc(Ok(()))
}

#[command]
pub fn remove_models_dir(path: String) -> Result<(), String> {
    MODELS_DIRS.lock().retain(|p| p.to_string_lossy() != path);
    ipc(Ok(()))
}

#[command]
pub fn get_models_dirs() -> Result<Vec<String>, String> {
    ipc(Ok(MODELS_DIRS
        .lock()
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()))
}

#[command]
pub fn get_download_dir() -> Result<Option<String>, String> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    ipc(Ok(Some(home.join("Downloads").to_string_lossy().into_owned())))
}

#[command]
pub fn set_download_dir(_path: String) -> Result<(), String> {
    ipc(Ok(()))
}

#[command]
pub fn scan_gguf_files(_dir: String) -> Result<Vec<serde_json::Value>, String> {
    ipc(Ok(Vec::new()))
}

#[command]
pub fn scan_hf_models(_repo: String) -> Result<Vec<serde_json::Value>, String> {
    ipc(Ok(Vec::new()))
}

#[command]
pub fn download_model(_url: String, _dest: String) -> Result<(), String> {
    ipc(Err(Error::not_implemented(
        "model download is on the clean-room roadmap",
    )))
}

#[command]
pub fn pause_download() -> Result<(), String> {
    ipc(Ok(()))
}

#[command]
pub fn cancel_download() -> Result<(), String> {
    ipc(Ok(()))
}

#[command]
pub fn detect_gpu() -> Result<serde_json::Value, String> {
    ipc(Ok(serde_json::json!({"available": false, "name": null, "vramGb": null})))
}

#[command]
pub fn get_gpu_info() -> Result<serde_json::Value, String> {
    ipc(Ok(serde_json::json!({"available": false})))
}

#[command]
pub fn install_local_engine() -> Result<(), String> {
    ipc(Err(Error::not_implemented(
        "local engine install is on the clean-room roadmap",
    )))
}

#[command]
pub fn get_local_engine_status() -> Result<serde_json::Value, String> {
    ipc(Ok(serde_json::json!({"installed": false, "version": null})))
}

#[command]
pub fn list_engine_release_options() -> Result<Vec<serde_json::Value>, String> {
    ipc(Ok(Vec::new()))
}
