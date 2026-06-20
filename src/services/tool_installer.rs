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
    /// `LocalTool.displayName` reads this on the frontend. We
    /// forward `InstallEntry.displayName` here so the right-hand
    /// panel of the App Manager can show the human-friendly
    /// label (e.g. "Claude Code (CLI)") even when the JSON's
    /// `name` is empty and we fell back to `displayName`.
    #[serde(skip_serializing_if = "Option::is_none", rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

pub fn scan_tools() -> CoreResult<Vec<DetectedTool>> {
    let entries = bundled_assets::all_install_entries()?;
    Ok(entries.iter().map(detect_one).collect())
}

/// `category_for` — map a tool id to its App Manager tab
/// category. The upstream install JSONs do not ship a `category`
/// field at all, but the frontend filter (`tool.category ===
/// activeToolCategory`) requires every tool to be in one of the
/// fixed buckets declared by `toolCategories` (Desktop / IDE /
/// CLI Code / AutoTrading / Game / Utility). The mapping below is
/// derived from each entry's `displayName` and the conventional
/// role of the tool — it lives in Rust (not in 23 separate JSON
/// edits) so the public manifest stays the single source of truth
/// and adding a new tool only requires adding one row here.
///
/// If a future tool id doesn't appear in this map, we fall back
/// to whatever `category` field the JSON carries (currently
/// always `None`, so the entry shows up only under the `ALL` tab).
fn category_for(id: &str) -> Option<&'static str> {
    match id {
        // Desktop — native GUI apps launched from the system shell
        "claudedesktop" | "codexdesktop" | "geminidesktop" | "opencodedesktop"
        | "coffeecli" => Some("Desktop"),
        // IDE — code editors with their own workspace UI
        "vscode" | "cursor" | "windsurf" | "trae" | "traecn" => Some("IDE"),
        // CLI Code — terminal-first developer tools
        "claudecode" | "codex" | "qwencode" | "aider" | "pi" | "openclaw"
        | "opencode" | "mimocode" => Some("CLI Code"),
        // AutoTrading — quant / trading agents
        "vibe-trading" => Some("AutoTrading"),
        // Utility — general-purpose assistants not specific to coding
        "grok" | "workbuddy" | "hermes" | "zcode" => Some("Utility"),
        _ => None,
    }
}

fn detect_one(entry: &InstallEntry) -> DetectedTool {
    let detected_path = entry
        .detected_path
        .as_deref()
        .and_then(|p| expand(p).ok())
        .filter(|p| probe(p));
    let installed = detected_path.is_some() || entry.detected_path.is_none();
    // `name` and `display_name` come from two different fields in the
    // upstream install JSONs. Some entries have only `name`; most
    // have only `displayName` (e.g. "Claude Code (CLI)"). Pick the
    // first one that's non-empty so the UI always has a label to
    // render, and copy whichever we used into `display_name` so the
    // frontend's `tool.displayName || tool.name` rendering reads
    // both branches identically.
    let name = entry
        .name
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or(entry.display_name.as_deref())
        .unwrap_or(entry.id.as_str())
        .to_string();
    let display_name = entry
        .display_name
        .clone()
        .or_else(|| Some(name.clone()));
    DetectedTool {
        id: entry.id.clone(),
        name,
        category: category_for(&entry.id)
            .map(|c| c.to_string())
            .or_else(|| entry.category.clone())
            .unwrap_or_default(),
        installed,
        detected_path: detected_path.as_ref().map(|p| p.display().to_string()),
        config_path: entry.config_path.clone(),
        website: entry.website.clone(),
        api_protocol: entry.api_protocol.clone(),
        command: entry.command.clone(),
        start_command: entry.start_command.clone(),
        launch_file: entry.launch_file.clone(),
        icon_base64: entry.icon_base64.clone(),
        display_name,
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
