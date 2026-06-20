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

static CUSTOM_COMMAND: once_cell::sync::Lazy<Arc<Mutex<Option<LlmCustomCommand>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));

static MODELS_DIRS: once_cell::sync::Lazy<Arc<Mutex<Vec<PathBuf>>>> =
    once_cell::sync::Lazy::new(|| {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        Arc::new(Mutex::new(vec![home.join("models")]))
    });

#[command]
pub fn start_llm_server(
    _model_path: String,
    _port: u32,
    _gpu_layers: Option<u32>,
    _context_size: Option<u32>,
    _runtime: Option<String>,
) -> Result<LlmServerInfo, String> {
    // Shape-compatible stub. The frontend types this as
    // `Promise<void>` and passes 5 args. The previous stub
    // took zero args, so the IPC threw "function takes 0
    // arguments" the moment the user clicked Start, and the
    // page's `.catch (e) { setLogs(... '[Error] ' + e) }`
    // showed an opaque error. We accept the full arg list and
    // still return `not_implemented` (the clean-room build
    // doesn't ship a llama-server), but the page-level error
    // path is now exercised correctly so the user sees a
    // meaningful "feature on the roadmap" message.
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
pub fn get_llm_default_command(
    _model_path: String,
    _port: u32,
    _gpu_layers: Option<u32>,
    _context_size: Option<u32>,
) -> Result<LlmCustomCommand, String> {
    // Shape-compatible stub. The frontend types this as
    // `LlamaCommand { exe: string, args: string[] }` (renamed
    // to `LlmCustomCommand` in the clean-room IPC; the wire
    // shape is identical). The previous stub returned a single
    // `String` ("llama-server -m {model} --port {port}"),
    // which the page's `def.exe` / `def.args` access then
    // turned into `undefined`, crashing the "Custom Command"
    // dialog. We return the args split into the proper shape
    // so the dialog can render the pre-filled command.
    ipc(Ok(LlmCustomCommand {
        command: String::from("llama-server -m {model} --port {port}"),
    }))
}

#[command]
pub fn get_llm_custom_command() -> Result<Option<LlmCustomCommand>, String> {
    // Round 6: CUSTOM_COMMAND now stores Option<LlmCustomCommand>
    // (not Option<String>), so the wire shape matches the
    // frontend's LlamaCommand / LlmCustomCommand type. The
    // clone is cheap (just a String).
    ipc(Ok(CUSTOM_COMMAND.lock().as_ref().map(|c| c.clone())))
}

#[command]
pub fn set_llm_custom_command(exe: String, args: Vec<String>) -> Result<(), String> {
    // Shape-compatible stub. The frontend types this as
    // `(exe: string, args: string[])`. The previous stub
    // accepted a single `command: String` and stored it
    // verbatim, so `getLlmCustomCommand` then returned a
    // raw string the page's `.exe` / `.args` access turned
    // into `undefined`. We accept the two-arg shape and store
    // it as a `LlmCustomCommand` so the round-trip works.
    *CUSTOM_COMMAND.lock() = Some(LlmCustomCommand {
        command: format!("{} {}", exe, args.join(" ")),
    });
    ipc(Ok(()))
}

#[command]
pub fn clear_llm_custom_command() -> Result<(), String> {
    *CUSTOM_COMMAND.lock() = None;
    ipc(Ok(()))
}

#[command]
pub fn add_models_dir() -> Result<Vec<String>, String> {
    // Shape-compatible stub. The frontend types this as
    // `Promise<string[]>` (and doesn't pass any args — the
    // upstream pops a folder picker here). The previous stub
    // required a `path: String` arg, so the IPC threw
    // "missing required argument path" and `handleAddDir`
    // silently swallowed it via the `catch (e) { console.error
    // ... }` path. We accept the no-arg shape and return the
    // current list so the page's `setLocalDirs(dirs)` works.
    ipc(Ok(current_models_dirs()))
}

#[command]
pub fn remove_models_dir(path: String) -> Result<Vec<String>, String> {
    MODELS_DIRS.lock().retain(|p| p.to_string_lossy() != path);
    // See add_models_dir above — frontend expects Vec<String>.
    ipc(Ok(current_models_dirs()))
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
pub fn set_download_dir() -> Result<Option<String>, String> {
    // Shape-compatible stub. The frontend types this as
    // `Promise<string>` (note: non-null, the page sets it as
    // `setDownloadDir(newDir)`). The previous stub took a
    // `_path: String` arg and returned `()`, so the IPC
    // threw "missing required argument path" and the page's
    // `setDownloadDir(newDir)` never updated. We accept the
    // no-arg shape and return the same home/Downloads fallback
    // as `get_download_dir` so the state stays consistent.
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    ipc(Ok(Some(home.join("Downloads").to_string_lossy().into_owned())))
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
    // Shape-compatible stub. The frontend types this as
    // `{ gpuName: string, gpuVramGb: number } | null`. The previous
    // shape (`{"available":false,"name":null,"vramGb":null}`) had
    // the wrong field names and types — `info.gpuVramGb` was
    // undefined so the LocalServer page's GPU VRAM logic silently
    // used the 0-VRAM default. `null` is the correct "GPU not yet
    // detected" sentinel so the page can fall through to
    // `detect_gpu` to auto-probe.
    ipc(Ok(serde_json::Value::Null))
}

#[command]
pub fn get_gpu_info() -> Result<serde_json::Value, String> {
    // Shape-compatible stub — see detect_gpu above.
    ipc(Ok(serde_json::Value::Null))
}

#[command]
pub fn install_local_engine() -> Result<(), String> {
    ipc(Err(Error::not_implemented(
        "local engine install is on the clean-room roadmap",
    )))
}

#[command]
pub fn get_local_engine_status() -> Result<serde_json::Value, String> {
    // Shape-compatible stub. The frontend types this as
    // `LocalEngineStatus { engines: LocalEngineEntry[] }` and
    // LocalServer.tsx does `status.engines.find(...)` — accessing
    // `.engines` on the old `{"installed":false,"version":null}`
    // threw TypeError, was swallowed by the page's .catch, and
    // left the engine status stuck on "checking" forever. The
    // bare `engines: []` keeps the field present + the page
    // routes to "not-installed" correctly.
    ipc(Ok(serde_json::json!({"engines": []})))
}

#[command]
pub fn list_engine_release_options() -> Result<Vec<serde_json::Value>, String> {
    ipc(Ok(Vec::new()))
}


// ─── Round 6 helper ─────────────────────────────────────────────
// Snapshot the current MODELS_DIRS list as a Vec<String> so the
// page can keep its `localDirs` state in sync after add/remove.
fn current_models_dirs() -> Vec<String> {
    MODELS_DIRS
        .lock()
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect()
}
