//! Tool discovery: reads the bundled install manifest, then
//! checks the host filesystem for the binary / config file each
//! tool needs. Returns the same `DetectedTool` shape the
//! frontend's `App Manager` page renders.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::bundled_assets::{self, InstallEntry};
use crate::error::CoreResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedTool {
    pub id: String,
    pub name: String,
    pub category: String,
    pub installed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detected_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub website: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_protocol: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub launch_file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

pub fn scan_tools() -> CoreResult<Vec<DetectedTool>> {
    let entries = bundled_assets::all_install_entries()?;
    Ok(entries.iter().map(detect_one).collect())
}

fn detect_one(entry: &InstallEntry) -> DetectedTool {
    let detected_path = entry
        .detected_path
        .as_deref()
        .and_then(|p| expand(p).ok())
        .filter(|p| probe(p));
    let installed = detected_path.is_some() || entry.detected_path.is_none();
    DetectedTool {
        id: entry.id.clone(),
        name: entry.name.clone(),
        category: entry.category.clone().unwrap_or_default(),
        installed,
        detected_path: detected_path.as_ref().map(|p| p.display().to_string()),
        config_path: entry.config_path.clone(),
        website: entry.website.clone(),
        api_protocol: entry.api_protocol.clone(),
        command: entry.command.clone(),
        start_command: entry.start_command.clone(),
        launch_file: entry.launch_file.clone(),
        icon_base64: entry.icon_base64.clone(),
        model: entry.model.clone(),
    }
}

fn expand(raw: &str) -> CoreResult<PathBuf> {
    if let Some(rest) = raw.strip_prefix("$HOME/") {
        let home = std::env::var_os("HOME")
            .ok_or_else(|| crate::error::Error::internal("$HOME not set"))?;
        Ok(PathBuf::from(home).join(rest))
    } else {
        Ok(PathBuf::from(raw))
    }
}

fn probe(p: &std::path::Path) -> bool {
    p.exists()
}
