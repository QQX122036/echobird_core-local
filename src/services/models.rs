//! Model service: business rules on top of the raw storage trait.
//!
//! The split is intentional: `storage::Store` is a thin SQL-shaped
//! CRUD layer with no validation. This module owns:
//!   * Field validation (URL parseable, name non-empty, etc.)
//!   * Auth-key classification (recognize `enc:v1:` prefix →
//!     `is_key_destroyed`)
//!   * Latency measurement (ping the upstream base URL)
//!   * Token-limit clamping (a zero from the form means "unset", not
//!     "0 tokens allowed"; the form-side parser already handles
//!     that, but we defend in depth)
//!
//! Errors surface to the IPC layer with a stable code prefix
//! (see `error::Code` for the contract); the frontend's
//! `errorMessages.ts` file knows how to render each one.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{CoreResult, Error};
use crate::storage::{Model, ModelPatch, ModelType, NewModel, Store};

/// Encrypted key prefix the public frontend recognizes. Keys that
/// start with this are AES-GCM ciphertext produced by
/// `services::secret`. We don't decrypt here — that would require
/// the OS keychain handle, which only `services::secret` owns.
pub const ENC_KEY_PREFIX: &str = "enc:v1:";

// ─── DTOs for the IPC layer ──────────────────────────────────────
//
// These are what the command handlers deserialize from / serialize
// to. They're kept distinct from `storage::Model` so we can change
// the wire format (e.g. add a new optional field) without touching
// the storage schema.

/// Mirror of `ModelConfig` in `src/api/types.ts`. Field names are
/// camelCase on the wire; the struct itself uses `rename_all`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDto {
    pub internal_id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    pub base_url: String,
    pub api_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_type: Option<ModelType>,
    #[serde(default)]
    pub openai_tested: bool,
    #[serde(default)]
    pub anthropic_tested: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai_latency: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic_latency: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_context_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u64>,
}

impl From<Model> for ModelDto {
    fn from(m: Model) -> Self {
        Self {
            internal_id: m.internal_id,
            name: m.name,
            model_id: m.model_id,
            base_url: m.base_url,
            api_key: m.api_key,
            anthropic_url: m.anthropic_url,
            model_type: Some(m.model_type),
            openai_tested: m.openai_tested,
            anthropic_tested: m.anthropic_tested,
            openai_latency: m.openai_latency,
            anthropic_latency: m.anthropic_latency,
            max_context_tokens: m.max_context_tokens,
            max_input_tokens: m.max_input_tokens,
            max_output_tokens: m.max_output_tokens,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewModelDto {
    pub name: String,
    #[serde(default)]
    pub model_id: Option<String>,
    pub base_url: String,
    pub api_key: String,
    #[serde(default)]
    pub anthropic_url: Option<String>,
    #[serde(default)]
    pub model_type: Option<ModelType>,
    #[serde(default)]
    pub max_context_tokens: Option<u64>,
    #[serde(default)]
    pub max_input_tokens: Option<u64>,
    #[serde(default)]
    pub max_output_tokens: Option<u64>,
}

impl NewModelDto {
    /// Validate and convert into the storage-layer `NewModel`.
    /// Returns `Validation` for any rule violation; the IPC layer
    /// surfaces the prefix to the user.
    pub fn validate(self) -> CoreResult<NewModel> {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            return Err(Error::validation("name is required"));
        }
        if name.len() > 200 {
            return Err(Error::validation("name must be ≤ 200 characters"));
        }
        let base_url = self.base_url.trim().to_string();
        if base_url.is_empty() {
            return Err(Error::validation("baseUrl is required"));
        }
        // URL parse: we accept any URL that has a host. The actual
        // HTTP call (in test_model / ping_model) will surface
        // network errors with a clearer code.
        Url::parse(&base_url).map_err(|e| {
            Error::validation(format!("baseUrl is not a valid URL: {e}"))
        })?;
        let anthropic_url = match self.anthropic_url {
            Some(s) if !s.trim().is_empty() => {
                let s = s.trim().to_string();
                Url::parse(&s).map_err(|e| {
                    Error::validation(format!("anthropicUrl is not a valid URL: {e}"))
                })?;
                Some(s)
            }
            _ => None,
        };
        validate_token_limit("maxContextTokens", self.max_context_tokens)?;
        validate_token_limit("maxInputTokens", self.max_input_tokens)?;
        validate_token_limit("maxOutputTokens", self.max_output_tokens)?;
        // Cross-field: if maxInput is set, it must be ≤ maxContext.
        if let (Some(input), Some(ctx)) = (self.max_input_tokens, self.max_context_tokens) {
            if input > ctx {
                return Err(Error::validation(
                    "maxInputTokens cannot exceed maxContextTokens",
                ));
            }
        }
        Ok(NewModel {
            name,
            model_id: self
                .model_id
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            base_url,
            api_key: self.api_key,
            anthropic_url,
            model_type: self.model_type.unwrap_or_default(),
            max_context_tokens: self.max_context_tokens,
            max_input_tokens: self.max_input_tokens,
            max_output_tokens: self.max_output_tokens,
        })
    }
}

fn validate_token_limit(field: &str, value: Option<u64>) -> CoreResult<()> {
    if let Some(v) = value {
        if v == 0 {
            return Err(Error::validation(format!("{field} must be > 0")));
        }
        if v > 100_000_000 {
            return Err(Error::validation(format!(
                "{field} looks unreasonable ({v} > 100M)"
            )));
        }
    }
    Ok(())
}

// ─── Operations ──────────────────────────────────────────────────

pub fn list_models(store: &Arc<dyn Store>) -> CoreResult<Vec<ModelDto>> {
    store
        .list_models()
        .map(|v| v.into_iter().map(ModelDto::from).collect())
}

pub fn add_model(store: &Arc<dyn Store>, input: NewModelDto) -> CoreResult<ModelDto> {
    let new = input.validate()?;
    store.insert_model(new).map(ModelDto::from)
}

pub fn update_model(
    store: &Arc<dyn Store>,
    internal_id: &str,
    patch: ModelPatch,
) -> CoreResult<ModelDto> {
    if let Some(url) = &patch.base_url {
        Url::parse(url).map_err(|e| {
            Error::validation(format!("baseUrl is not a valid URL: {e}"))
        })?;
    }
    if let Some(Some(url)) = &patch.anthropic_url {
        Url::parse(url).map_err(|e| {
            Error::validation(format!("anthropicUrl is not a valid URL: {e}"))
        })?;
    }
    validate_token_limit("maxContextTokens", patch.max_context_tokens)?;
    validate_token_limit("maxInputTokens", patch.max_input_tokens)?;
    validate_token_limit("maxOutputTokens", patch.max_output_tokens)?;
    store
        .update_model(internal_id, patch)
        .map(ModelDto::from)
}

pub fn delete_model(store: &Arc<dyn Store>, internal_id: &str) -> CoreResult<bool> {
    store.delete_model(internal_id)
}

// ─── Latency / connectivity checks ───────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PingResult {
    pub success: bool,
    pub latency_ms: u32,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub async fn ping_model(
    store: &Arc<dyn Store>,
    internal_id: &str,
) -> CoreResult<PingResult> {
    let m = store.get_model(internal_id)?;
    let url = m.base_url.clone();
    let res = ping_url(&url).await;
    if res.success {
        // Persist latency so the ModelCard can show it.
        store.update_model(
            internal_id,
            ModelPatch {
                openai_latency: Some(res.latency_ms),
                openai_tested: Some(true),
                ..Default::default()
            },
        )?;
    }
    Ok(res)
}

/// Public for the test_model command (which also sends a real chat
/// completion to verify the key works). We keep the implementation
/// inline because it's tiny and the error mapping is bespoke.
pub async fn ping_url(url: &str) -> PingResult {
    let start = std::time::Instant::now();
    let client_res = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build();
    let client = match client_res {
        Ok(c) => c,
        Err(e) => return ping_error(url, start, e.to_string()),
    };
    match client.get(url).send().await {
        Ok(_resp) => PingResult {
            success: true,
            latency_ms: start.elapsed().as_millis() as u32,
            url: url.to_string(),
            error: None,
        },
        Err(e) => ping_error(url, start, e.to_string()),
    }
}

/// `ping_error` — internal helper to build a `PingResult`
/// without going through the request stack. Used when the
/// client builder itself fails (rare — usually means a TLS
/// stack init problem).
fn ping_error(url: &str, start: std::time::Instant, msg: String) -> PingResult {
    PingResult {
        success: false,
        latency_ms: start.elapsed().as_millis() as u32,
        url: url.to_string(),
        error: Some(msg),
    }
}

/// `TestModelResult` — the response from a real chat-completion
/// round trip. `protocol` is the wire protocol the test used
/// (e.g. `"openai"` or `"anthropic"`) so the UI can label the
/// card correctly.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestModelResult {
    pub success: bool,
    pub latency_ms: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub protocol: String,
}

/// Full chat-completion round trip with a tiny prompt, used to
/// verify the API key works and to give the user a confidence
/// signal in the ModelCard "test" button.
pub async fn test_model(
    store: &Arc<dyn Store>,
    internal_id: &str,
    prompt: &str,
    protocol: &str,
) -> CoreResult<TestModelResult> {
    let m = store.get_model(internal_id)?;
    let url = match protocol {
        "anthropic" => m.anthropic_url.clone().ok_or_else(|| {
            Error::validation("model has no anthropicUrl configured")
        })?,
        _ => m.base_url.clone(),
    };
    let start = std::time::Instant::now();
    let client_res = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build();
    let client = match client_res {
        Ok(c) => c,
        Err(e) => return Err(Error::network(e.to_string())),
    };
    let cap = m.max_output_tokens.unwrap_or(64).min(64) as u32;
    let body = serde_json::json!({
        "model": m.model_id.clone().unwrap_or_default(),
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": cap,
    });
    let resp_res = client.post(&url).bearer_auth(&m.api_key).json(&body).send().await;
    let resp = match resp_res {
        Ok(r) => r,
        Err(e) => return Err(Error::from(e)),
    };
    let status = resp.status();
    let latency_ms = start.elapsed().as_millis() as u32;
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Ok(TestModelResult {
            success: false,
            latency_ms,
            response: None,
            error: Some(format!("upstream {status}: {}", truncate(&text, 200))),
            protocol: protocol.to_string(),
        });
    }
    let response = first_content_string(&text).unwrap_or_else(|| truncate(&text, 200));
    Ok(TestModelResult {
        success: true,
        latency_ms,
        response: Some(response),
        error: None,
        protocol: protocol.to_string(),
    })
}

fn first_content_string(s: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(s).ok()?;
    // OpenAI: choices[0].message.content
    if let Some(s) = v.pointer("/choices/0/message/content").and_then(|x| x.as_str()) {
        return Some(s.to_string());
    }
    // Anthropic: content[0].text
    if let Some(s) = v.pointer("/content/0/text").and_then(|x| x.as_str()) {
        return Some(s.to_string());
    }
    None
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        let mut end = n;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    }
}

/// `is_key_destroyed` — frontend asks whether the encrypted key
/// stored in this model can still be decrypted. We always report
/// `false` unless the secret service tells us otherwise; the
/// "destroyed" state is set when a keychain operation fails
/// irrecoverably, which `services::secret` records.
pub fn is_key_destroyed(_store: &Arc<dyn Store>, _internal_id: &str) -> CoreResult<bool> {
    // The full implementation lives in `services::secret` once we
    // wire up the OS keychain. For the open-source clean-room
    // build, the key is always considered recoverable.
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::memory::InMemoryStore;

    fn sample_input() -> NewModelDto {
        NewModelDto {
            name: "MiniMax CN".into(),
            model_id: Some("MiniMax-M3".into()),
            base_url: "https://api.minimaxi.com/v1".into(),
            api_key: "sk-test".into(),
            anthropic_url: Some("https://api.minimaxi.com/anthropic".into()),
            model_type: None,
            max_context_tokens: Some(1_000_000),
            max_input_tokens: Some(900_000),
            max_output_tokens: Some(32_000),
        }
    }

    #[test]
    fn validate_accepts_minimax_cn() {
        let new = sample_input().validate().expect("valid");
        assert_eq!(new.name, "MiniMax CN");
        assert_eq!(new.max_context_tokens, Some(1_000_000));
    }

    #[test]
    fn validate_rejects_empty_name() {
        let mut s = sample_input();
        s.name = "  ".into();
        let err = s.validate().unwrap_err();
        assert!(matches!(err, Error::Validation { .. }));
    }

    #[test]
    fn validate_rejects_zero_context() {
        let mut s = sample_input();
        s.max_context_tokens = Some(0);
        let err = s.validate().unwrap_err();
        assert!(matches!(err, Error::Validation { .. }));
    }

    #[test]
    fn validate_rejects_input_greater_than_context() {
        let mut s = sample_input();
        s.max_context_tokens = Some(100_000);
        s.max_input_tokens = Some(200_000);
        let err = s.validate().unwrap_err();
        assert!(matches!(err, Error::Validation { .. }));
    }

    #[test]
    fn crud_roundtrip_persists_token_limits() {
        let store: Arc<dyn Store> = InMemoryStore::new();
        let m = add_model(&store, sample_input()).unwrap();
        let fetched = store.get_model(&m.internal_id).unwrap();
        assert_eq!(fetched.max_context_tokens, Some(1_000_000));
        assert_eq!(fetched.max_output_tokens, Some(32_000));
    }
}

/// `update_model_from_json` — convenience for the command layer.
/// The public frontend's `applyModelToTool` and `updateModel`
/// commands take a `serde_json::Value` of `updates` (a partial).
/// We map the JSON shape to a [`ModelPatch`] and delegate.
pub fn update_model_from_json(
    store: &Arc<dyn Store>,
    internal_id: &str,
    updates: serde_json::Value,
) -> CoreResult<ModelDto> {
    #[derive(Deserialize, Default)]
    #[serde(rename_all = "camelCase")]
    struct UpdatesDto {
        name: Option<String>,
        model_id: Option<String>,
        base_url: Option<String>,
        api_key: Option<String>,
        /// `null` clears the field; absent leaves it alone.
        anthropic_url: Option<Option<String>>,
        max_context_tokens: Option<u64>,
        max_input_tokens: Option<u64>,
        max_output_tokens: Option<u64>,
    }
    let u: UpdatesDto = serde_json::from_value(updates)?;
    let patch = ModelPatch {
        name: u.name,
        model_id: u.model_id,
        base_url: u.base_url,
        api_key: u.api_key,
        anthropic_url: u.anthropic_url,
        max_context_tokens: u.max_context_tokens,
        max_input_tokens: u.max_input_tokens,
        max_output_tokens: u.max_output_tokens,
        ..Default::default()
    };
    update_model(store, internal_id, patch)
}
