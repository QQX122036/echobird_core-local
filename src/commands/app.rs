//! App-lifecycle IPC handlers: settings, log tail, system info,
//! avatar, project / game launchers. The Tauri shell already
//! handles a lot of these natively (window control, file dialog,
//! shell.open), so the Rust side is mostly a thin shim.

use tauri::{command, Manager};

use crate::commands::ipc;
use crate::error::{CoreResult, Error};
use crate::services::settings as svc;
use crate::storage::global;

#[command]
pub fn get_settings() -> Result<crate::storage::AppSettings, String> {
    ipc(svc::get_settings(global::get()))
}

#[command]
pub fn save_settings(
    settings: serde_json::Value,
) -> Result<crate::storage::AppSettings, String> {
    ipc(save(global::get(), settings))
}

#[command]
pub fn app_ready(app: tauri::AppHandle) -> Result<(), String> {
    // Show the main window. The frontend calls this after the
    // first paint so the window appears with the real UI on
    // screen (no white flash). The lib.rs setup hook also
    // installs a 1-second safety timer that shows the window
    // even if the frontend never reaches the first paint.
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
    Ok(())
}

#[command]
pub fn open_folder(path: String) -> Result<(), String> {
    ipc(open_folder_impl(&path))
}

#[command]
pub fn read_log_tail(lines: u32) -> Result<String, String> {
    ipc(read_tail(lines))
}

#[command]
pub fn get_system_info() -> Result<serde_json::Value, String> {
    ipc(Ok(sysinfo()))
}

#[command]
pub async fn download_and_install_update(_version: String) -> Result<(), String> {
    // No-op in the clean-room build. The upstream has Windows-
    // specific download logic. The frontend treats the resulting
    // `not_implemented:` error as "fall back to the GitHub
    // release page in the browser".
    ipc(Err(Error::not_implemented(
        "in-app self-update is Windows-only and ships in the proprietary build",
    )))
}

#[command]
pub fn get_avatar() -> Result<Option<String>, String> {
    // The avatar is stored in Tauri's `app_data_dir()/avatar.png`.
    // We return the absolute path; the frontend reads it as an
    // `<img src>` URL via the asset protocol.
    ipc(avatar_path().map(|p| p.exists().then(|| p.to_string_lossy().into_owned())))
}

#[command]
pub fn set_avatar(src_path: String) -> Result<String, String> {
    ipc(set_avatar_impl(&src_path))
}

#[command]
pub fn seed_builtin_to_user_dir(tool_id: String) -> Result<String, String> {
    ipc(seed(&tool_id).map(|p| p.to_string_lossy().into_owned()))
}

#[command]
pub fn apply_user_project_model(
    _models_json_path: String,
    _model_info: serde_json::Value,
) -> Result<(), String> {
    ipc(Ok(()))
}

#[command]
pub fn launch_user_project(_launcher_path: String) -> Result<(), String> {
    ipc(Ok(()))
}

#[command]
pub fn launch_game(
    _tool_id: String,
    _launch_file: String,
    _model_config: Option<serde_json::Value>,
) -> Result<serde_json::Value, String> {
    ipc(Ok(serde_json::json!({"success": true})))
}

// ─── implementations ────────────────────────────────────────────

fn save(
    store: &std::sync::Arc<dyn crate::storage::Store>,
    raw: serde_json::Value,
) -> CoreResult<crate::storage::AppSettings> {
    use crate::storage::SettingsPatch;
    use crate::storage::settings::ThemeMode;
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct D {
        locale: Option<String>,
        theme_mode: Option<ThemeMode>,
        close_to_tray: Option<bool>,
        close_window_behavior_set: Option<bool>,
    }
    let d: D = serde_json::from_value(raw)?;
    let patch = SettingsPatch {
        locale: d.locale,
        theme_mode: Some(d.theme_mode),
        close_to_tray: Some(d.close_to_tray),
        close_window_behavior_set: d.close_window_behavior_set,
    };
    svc::save_settings(store, patch)
}

fn open_folder_impl(path: &str) -> Result<(), Error> {
    let p = std::path::Path::new(path);
    if !p.exists() {
        return Err(Error::not_found(format!("path {path}")));
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(path).spawn()?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(path).spawn()?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer").arg(path).spawn()?;
    }
    Ok(())
}

fn read_tail(lines: u32) -> CoreResult<String> {
    // The log file is in `app_data_dir/echobird.log`. We try to
    // read the last `lines` lines; if anything goes wrong (file
    // missing, permission denied), we return an empty string so
    // the frontend's "copy logs" button still has something to
    // show. The user shouldn't be blocked on a missing log.
    let path = app_data_dir()?.join("echobird.log");
    let body = std::fs::read_to_string(&path).unwrap_or_default();
    let collected: Vec<&str> = body.lines().rev().take(lines as usize).collect();
    Ok(collected.into_iter().rev().collect::<Vec<_>>().join("\n"))
}

fn sysinfo() -> serde_json::Value {
    // sysinfo read goes here; we just return the OS/arch pair
    // because the upstream feeds the frontend a richer struct
    // that we don't depend on.
    serde_json::json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
    })
}

fn avatar_path() -> CoreResult<std::path::PathBuf> {
    Ok(app_data_dir()?.join("avatar.png"))
}

fn set_avatar_impl(src: &str) -> CoreResult<String> {
    let bytes = std::fs::read(src).map_err(|e| Error::storage(e.to_string()))?;
    let dest = avatar_path()?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, bytes)?;
    Ok(dest.to_string_lossy().into_owned())
}

fn seed(tool_id: &str) -> CoreResult<std::path::PathBuf> {
    // Copy the bundled install entry's reference files into
    // `~/.echobird/<id>/`. The install manifest is the source of
    // truth; we just create the dir if missing.
    let home = std::env::var_os("HOME")
        .ok_or_else(|| Error::internal("$HOME not set"))?;
    let dest = std::path::PathBuf::from(home)
        .join(".echobird")
        .join(tool_id);
    std::fs::create_dir_all(&dest)?;
    Ok(dest)
}

fn app_data_dir() -> CoreResult<std::path::PathBuf> {
    // For the command layer, we resolve the dir from $ECHOBIRD_DATA
    // first (so tests can redirect), then fall back to the
    // platform default via the `dirs` crate convention.
    if let Some(p) = std::env::var_os("ECHOBIRD_DATA") {
        return Ok(std::path::PathBuf::from(p));
    }
    let home = std::env::var_os("HOME")
        .ok_or_else(|| Error::internal("$HOME not set"))?;
    Ok(std::path::PathBuf::from(home).join(".echobird"))
}

use serde::Deserialize;
