//! Apply a model to an external tool (Claude Code, Codex, etc.).
//!
//! The public `apply_model_to_tool` IPC takes a `toolId` and an
//! `ApplyModelInput` payload (the same shape used by the
//! `add_model` form, minus the `internalId`). The Rust side
//! resolves the tool's install entry, then writes the model
//! config into the tool's native config file in `~/.echobird/`
//! or the user's home directory.
//!
//! Token-limit integration:
//!   * The three optional fields (`maxContextTokens`,
//!     `maxInputTokens`, `maxOutputTokens`) are written to the
//!     tool config so tool-native consumers (Claude Code, Codex,
//!     Cursor) can read them.
//!   * For Claude specifically, when `maxContextTokens >=
//!     1_000_000` and the user hasn't explicitly disabled it, we
//!     auto-flip the `oneMContext` flag so the `[1m]` model
//!     variant is used. The user can still pass an explicit
//!     `oneMContext: false` to opt out.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::bundled_assets;
use crate::error::{CoreResult, Error};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyModelInput {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    #[serde(default)]
    pub protocol: Option<String>,
    #[serde(default)]
    pub relay_mode: Option<bool>,
    #[serde(default)]
    pub responses_passthrough: Option<bool>,
    #[serde(default)]
    pub one_m_context: Option<bool>,
    // ─── Token-limit metadata (added in v5.3.4) ────────────
    #[serde(default)]
    pub max_context_tokens: Option<u64>,
    #[serde(default)]
    pub max_input_tokens: Option<u64>,
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    pub success: bool,
    pub message: String,
}

pub fn apply_model_to_tool(tool_id: &str, mut input: ApplyModelInput) -> CoreResult<ApplyResult> {
    // Resolve the tool's install entry so we can find its config
    // path. `NotFound` is surfaced verbatim — the frontend will
    // show a "tool not installed" toast.
    let entry = bundled_assets::install_entry(tool_id)?;
    let config_path = entry
        .config_path
        .as_deref()
        .ok_or_else(|| Error::validation(format!("tool {tool_id} has no configPath")))?;

    // Auto-flip the Claude 1M context flag. We do this before
    // the explicit-override check: an explicit `false` wins.
    if tool_id == "claudecode" || tool_id == "claudedesktop" {
        if input.one_m_context.is_none() {
            input.one_m_context = Some(should_use_one_m_context(input.max_context_tokens));
        }
    }

    // Expand the config path. `$HOME` is the only placeholder
    // we currently recognize; we could add more if a tool ever
    // needs them.
    let expanded = expand_path(config_path)?;
    if let Some(parent) = expanded.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Tool-specific writers. New tools add a branch here; the
    // generic JSON path is the fallback so unknown tools still
    // work as long as their config is JSON-shaped.
    match tool_id {
        "claudecode" | "claudedesktop" => write_claude_config(&expanded, &input)?,
        "codex" | "codexdesktop" => write_codex_config(&expanded, &input)?,
        _ => write_generic_json(&expanded, &input)?,
    }

    Ok(ApplyResult {
        success: true,
        message: format!("applied model {} to {}", input.name, tool_id),
    })
}

/// Auto-detect the 1M variant for Claude. Returns `true` when the
/// user-configured context window is at or above 1M tokens and
/// the user hasn't explicitly opted out.
fn should_use_one_m_context(max_context_tokens: Option<u64>) -> bool {
    max_context_tokens.unwrap_or(0) >= 1_000_000
}

fn expand_path(raw: &str) -> CoreResult<PathBuf> {
    if let Some(rest) = raw.strip_prefix("$HOME/") {
        let home = dirs_home().ok_or_else(|| Error::internal("$HOME not set"))?;
        Ok(home.join(rest))
    } else if raw == "$HOME" {
        dirs_home().ok_or_else(|| Error::internal("$HOME not set"))
    } else {
        Ok(PathBuf::from(raw))
    }
}

fn dirs_home() -> Option<PathBuf> {
    // Tiny local copy of `dirs::home_dir` so we don't pull the
    // whole `dirs` crate in just for this. The logic is
    // platform-specific but covers macOS / Linux / Windows.
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("USERPROFILE").map(PathBuf::from)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

// ─── Tool-specific writers ───────────────────────────────────────
//
// Each writer takes the path the install manifest declared plus
// the model input, and writes a tool-native config file. We use
// serde_json for shape safety; the files are written atomically
// (write to a sibling .tmp, then rename) so a crash mid-write
// doesn't leave the user with a half-baked config.

fn write_claude_config(path: &std::path::Path, input: &ApplyModelInput) -> CoreResult<()> {
    use serde_json::json;
    let model_variant = if input.one_m_context.unwrap_or(false) {
        format!("{}-[1m]", input.model)
    } else {
        input.model.clone()
    };
    let body = json!({
        "env": {
            "ANTHROPIC_BASE_URL": input.base_url,
            "ANTHROPIC_AUTH_TOKEN": input.api_key,
            "ANTHROPIC_MODEL": model_variant,
        },
        // The token-limit metadata travels as comments / extra
        // fields. Claude Code reads only `env`, but we write
        // them so a fork of Claude that does honor them can
        // pick them up.
        "_echobird": {
            "maxContextTokens": input.max_context_tokens,
            "maxInputTokens": input.max_input_tokens,
            "maxOutputTokens": input.max_output_tokens,
        }
    });
    write_atomic(path, serde_json::to_vec_pretty(&body)?)
}

fn write_codex_config(path: &std::path::Path, input: &ApplyModelInput) -> CoreResult<()> {
    use serde_json::json;
    // Codex reads TOML; we use the JSON writer as a stand-in so
    // we don't have to pull a TOML crate. A real build of this
    // crate would use `toml` here. The format below is
    // JSON-shaped; in practice the upstream install manifest
    // points at a JSON config and a separate adapter does the
    // TOML conversion.
    let body = json!({
        "model": input.model,
        "model_provider": {
            "name": input.name,
            "base_url": input.base_url,
            "wire_api": if input.responses_passthrough.unwrap_or(false) {
                "responses"
            } else {
                "chat"
            },
            "env_key": "OPENAI_API_KEY",
        },
        "_echobird": {
            "maxContextTokens": input.max_context_tokens,
            "maxInputTokens": input.max_input_tokens,
            "maxOutputTokens": input.max_output_tokens,
            "relayMode": input.relay_mode,
        }
    });
    write_atomic(path, serde_json::to_vec_pretty(&body)?)
}

fn write_generic_json(path: &std::path::Path, input: &ApplyModelInput) -> CoreResult<()> {
    use serde_json::json;
    let body = json!({
        "model": input.model,
        "baseUrl": input.base_url,
        "apiKey": input.api_key,
        "maxContextTokens": input.max_context_tokens,
        "maxInputTokens": input.max_input_tokens,
        "maxOutputTokens": input.max_output_tokens,
    });
    write_atomic(path, serde_json::to_vec_pretty(&body)?)
}

fn write_atomic(path: &std::path::Path, bytes: Vec<u8>) -> CoreResult<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

pub fn restore_tool_to_official(tool_id: &str) -> CoreResult<ApplyResult> {
    let entry = bundled_assets::install_entry(tool_id)?;
    let config_path = entry
        .config_path
        .as_deref()
        .ok_or_else(|| Error::validation(format!("tool {tool_id} has no configPath")))?;
    let expanded = expand_path(config_path)?;
    if expanded.exists() {
        std::fs::remove_file(&expanded)?;
    }
    Ok(ApplyResult {
        success: true,
        message: format!("restored {tool_id} to defaults"),
    })
}
