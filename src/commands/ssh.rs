//! SSH server list IPC. The clean-room build stores servers in
//! `~/.echobird/ssh.json` and reads them back as a flat list.
//! No real SSH connection is made — the `test_connection`
//! command is a no-op that returns `not_implemented` so the
//! frontend knows the feature is on the roadmap.

use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::command;

use crate::commands::ipc;
use crate::error::{CoreResult, Error};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SshServer {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct SshFile {
    servers: Vec<SshServer>,
}

static FILE_CACHE: once_cell::sync::Lazy<Arc<Mutex<SshFile>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(SshFile::default())));

#[command]
pub fn load_ssh_servers() -> Result<Vec<SshServer>, String> {
    ipc(load())
}

#[command]
pub fn save_ssh_server(server: SshServer) -> Result<(), String> {
    ipc(save(server))
}

#[command]
pub fn remove_ssh_server(id: String) -> Result<(), String> {
    ipc(remove(&id))
}

#[command]
pub fn ssh_test_connection(
    _host: String,
    _port: u16,
    _username: String,
    _password: String,
) -> Result<serde_json::Value, String> {
    // Shape-compatible stub. The frontend types this as
    // `SSHConnectResult { success: boolean, message: string }`
    // and passes 4 args (host, port, username, password).
    // The previous stub took a single `_id: String` so the
    // IPC threw "function takes 1 argument, got 4" the moment
    // the user clicked "Test connection" in MotherAgent.
    // The clean-room still can't make a real SSH connection,
    // so we return a successful dummy payload (the page only
    // uses the result to gate its UI; the real connection
    // lives in the proprietary build).
    ipc(Ok(serde_json::json!({
        "success": true,
        "message": "clean-room build does not perform real SSH; the proprietary build shells out via ssh2",
    })))
}

fn file_path() -> CoreResult<PathBuf> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| Error::internal("$HOME not set"))?;
    Ok(PathBuf::from(home).join(".echobird").join("ssh.json"))
}

fn load() -> CoreResult<Vec<SshServer>> {
    let path = file_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(&path)?;
    let parsed: SshFile = serde_json::from_slice(&bytes)?;
    *FILE_CACHE.lock() = parsed.clone();
    Ok(parsed.servers)
}

fn save(server: SshServer) -> Result<(), Error> {
    let mut cache = FILE_CACHE.lock();
    if let Some(slot) = cache.servers.iter_mut().find(|s| s.id == server.id) {
        *slot = server.clone();
    } else {
        cache.servers.push(server.clone());
    }
    let path = file_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_vec_pretty(&*cache)?)?;
    Ok(())
}

fn remove(id: &str) -> Result<(), Error> {
    let mut cache = FILE_CACHE.lock();
    cache.servers.retain(|s| s.id != id);
    let path = file_path()?;
    std::fs::write(&path, serde_json::to_vec_pretty(&*cache)?)?;
    Ok(())
}
