//! Domain types for the model store. Mirrors the TypeScript
//! `ModelConfig` interface in `src/api/types.ts` so the IPC layer
//! can `serde_json::to_value` straight into the response payload.
//!
//! Naming: we use Rust-native snake_case internally and rely on
//! `#[serde(rename_all = "camelCase")]` at the IPC boundary. The
//! TypeScript side already speaks camelCase.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// All fields the IPC contract exposes. `internal_id` is the
/// primary key on the wire and in the DB; everything else is the
/// model config the user typed in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub internal_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub base_url: String,
    pub api_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic_url: Option<String>,
    #[serde(default = "default_model_type")]
    pub model_type: ModelType,
    #[serde(default)]
    pub openai_tested: bool,
    #[serde(default)]
    pub anthropic_tested: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai_latency: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic_latency: Option<u32>,
    /// Total context window in tokens (input + output). Optional —
    /// when `None`, the agent falls back to its built-in default.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_context_tokens: Option<u64>,
    /// Maximum input tokens per request. Optional.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<u64>,
    /// Maximum output tokens per response. Optional.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
    #[serde(default = "Utc::now")]
    pub created_at: DateTime<Utc>,
    #[serde(default = "Utc::now")]
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum ModelType {
    #[default]
    Cloud,
    Local,
    Tunnel,
    Demo,
}

fn default_model_type() -> ModelType {
    ModelType::Cloud
}

/// Payload for `insert_model`. The storage layer fills in
/// `internal_id` (UUID v4) and `created_at` / `updated_at`. The
/// caller is responsible for validating the inputs (we surface
/// `Error::Validation` for missing required fields, but the service
/// layer is where business validation lives).
#[derive(Debug, Clone)]
pub struct NewModel {
    pub name: String,
    pub model_id: Option<String>,
    pub base_url: String,
    pub api_key: String,
    pub anthropic_url: Option<String>,
    pub model_type: ModelType,
    pub max_context_tokens: Option<u64>,
    pub max_input_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
}

/// All-optional patch for `update_model`. `None` means "don't
/// change"; `Some(None)` on `anthropic_url` means "clear the field".
/// The other `Option<u64>` fields use `None` to mean "don't change"
/// — clearing them is not a supported use case from the frontend.
#[derive(Debug, Clone, Default)]
pub struct ModelPatch {
    pub name: Option<String>,
    pub model_id: Option<String>,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    /// `None` = don't change, `Some(None)` = clear, `Some(Some(url))` = set.
    pub anthropic_url: Option<Option<String>>,
    pub model_type: Option<ModelType>,
    pub max_context_tokens: Option<u64>,
    pub max_input_tokens: Option<u64>,
    pub max_output_tokens: Option<u64>,
    /// When `true`, the patch's per-field test latency / tested
    /// flags are updated. Today no caller flips this, but the
    /// schema reserves the columns for the future.
    pub openai_tested: Option<bool>,
    pub anthropic_tested: Option<bool>,
    pub openai_latency: Option<u32>,
    pub anthropic_latency: Option<u32>,
}

impl Model {
    /// `true` if the user has set at least one token-limit field.
    /// Used by the agent forwarding layer to decide whether to
    /// apply per-request clamps (if false, we keep upstream's
    /// default behavior, which is what the user wants when they
    /// haven't given us a number).
    pub fn has_token_limits(&self) -> bool {
        self.max_context_tokens.is_some()
            || self.max_input_tokens.is_some()
            || self.max_output_tokens.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_token_limits_detects_any_field() {
        let mut m = sample_model();
        // sample_model already has max_context_tokens and max_output_tokens set, so has_token_limits is true at this point
        let mut m = Model { max_context_tokens: Some(128_000), max_input_tokens: None, max_output_tokens: None, ..sample_model() };
        

        m.max_context_tokens = None;
        m.max_input_tokens = Some(8_000);
        

        m.max_input_tokens = None;
        m.max_output_tokens = Some(4_000);
        
    }

    #[test]
    fn serde_roundtrip_uses_camel_case() {
        let m = sample_model();
        let v = serde_json::to_value(&m).unwrap();
        assert!(v.get("internalId").is_some());
        assert!(v.get("baseUrl").is_some());
        assert!(v.get("apiKey").is_some());
        assert!(v.get("maxContextTokens").is_some());
    }

    fn sample_model() -> Model {
        Model {
            internal_id: "abc".into(),
            name: "MiniMax CN".into(),
            model_id: Some("MiniMax-M3".into()),
            base_url: "https://api.minimaxi.com/v1".into(),
            api_key: "sk-test".into(),
            anthropic_url: Some("https://api.minimaxi.com/anthropic".into()),
            model_type: ModelType::Cloud,
            openai_tested: false,
            anthropic_tested: false,
            openai_latency: None,
            anthropic_latency: None,
            max_context_tokens: Some(1_000_000),
            max_input_tokens: None,
            max_output_tokens: Some(32_000),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }
}
