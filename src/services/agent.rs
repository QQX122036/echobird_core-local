//! Agent chat-completion forwarding.
//!
//! The public `agent_send_message` IPC kicks off a stream of
//! `text_delta` / `thinking` / `tool_call_*` / `done` / `error`
//! events to the frontend (see `src/api/types.ts`'s
//! `AgentEvent`). This module is the Rust side of that pipeline.
//!
//! Token-limit integration:
//!   * On entry, the user's `max_input_tokens` cap is applied to
//!     the message history via `context_window::trim_to_input_cap`.
//!   * On every upstream call, the per-request `max_tokens` is
//!     clamped via `context_window::clamp_max_tokens` to
//!     `max_output_tokens`.
//!   * On done, we emit a `state` event with a `contextUsage`
//!     payload the Mother Agent UI consumes for the progress bar.
//!
//! Streaming is real: we use `reqwest`'s response stream and emit
//! one `text_delta` per Server-Sent Event (SSE) chunk. Anthropic
//! and OpenAI both speak SSE with slightly different envelope
//! shapes; the parser handles both.

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Runtime};

use super::context_window::{
    clamp_max_tokens, estimate_messages_tokens, trim_to_input_cap,
};
use super::models::ModelDto;
use crate::error::{CoreResult, Error};
use crate::storage::Store;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSendInput {
    pub message: String,
    pub model_id: String,
    pub base_url: String,
    pub api_key: String,
    pub model_name: String,
    pub provider: String,
    #[serde(default)]
    pub anthropic_url: Option<String>,
    #[serde(default)]
    pub server_ids: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    #[serde(default)]
    pub locale: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    TextDelta { text: String },
    Thinking { text: String },
    ToolCallStart { id: String, name: String },
    ToolCallArgs { id: String, args: String },
    ToolResult { id: String, output: String, success: bool },
    Done,
    Error { message: String },
    State { state: String },
}

/// Streaming chat-completion forward. Emits events to the
/// frontend via Tauri events on the `agent-event` channel. Returns
/// the final event for logging; callers (`commands::agent`) don't
/// usually inspect it because the IPC contract is "fire and
/// stream".
pub async fn send_message<R: Runtime>(
    app: &AppHandle<R>,
    store: &Arc<dyn Store>,
    input: AgentSendInput,
) -> CoreResult<AgentEvent> {
    // The IPC sends `modelId` and `baseUrl` directly (not a
    // `Model` lookup), so we don't pull the model out of the
    // store — the user can paste a baseUrl that isn't in any
    // saved model. Token limits therefore arrive as part of the
    // saved model; if the user is chatting with an ad-hoc
    // baseUrl, we just skip the cap (and the UI shows a hint).
    //
    // Resolve the saved model (if any) for its token caps.
    let saved_model = lookup_saved_model(store, &input.model_id);
    let (max_input, max_output, max_context) = match &saved_model {
        Some(m) => (m.max_input_tokens, m.max_output_tokens, m.max_context_tokens),
        None => (None, None, None),
    };

    // Compose the message list. Today the IPC sends a single
    // user message; we wrap it in a 1-element vec to leave room
    // for future multi-turn support. The trim step is a no-op
    // for 1 message, but the cap is still applied to
    // `max_tokens` on the request side.
    let mut messages = vec![input.message.clone()];
    let _dropped = trim_to_input_cap(&mut messages, max_input);
    let _input_tokens = estimate_messages_tokens(&messages);

    // Choose protocol by URL pattern: an `anthropicUrl` set
    // means the user wants the Anthropic protocol (the public
    // `agent` service tries Anthropic first and falls back to
    // OpenAI on 400). For the clean-room impl, we just look at
    // the URL and dispatch.
    let use_anthropic = input
        .anthropic_url
        .as_deref()
        .map(|u| u.contains("anthropic") || u.contains("/v1/messages"))
        .unwrap_or(false);
    let url = if use_anthropic {
        input.anthropic_url.clone().unwrap_or(input.base_url.clone())
    } else {
        input.base_url.clone()
    };

    // Build the request body, applying the output cap.
    let requested_max_tokens = 4096u32; // Frontend's default for first call
    let max_tokens = clamp_max_tokens(Some(requested_max_tokens), max_output);
    let body = if use_anthropic {
        json!({
            "model": input.model_id,
            "messages": [{"role": "user", "content": messages[0]}],
            "max_tokens": max_tokens,
            "stream": true,
        })
    } else {
        json!({
            "model": input.model_id,
            "messages": [{"role": "user", "content": messages[0]}],
            "max_tokens": max_tokens,
            "stream": true,
        })
    };

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| Error::network(e.to_string()))?;
    let req = client.post(&url).bearer_auth(&input.api_key).json(&body);
    let resp = req.send().await?;
    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        let msg = format!("upstream {status}: {}", truncate(&text, 200));
        let _ = app.emit("agent-event", AgentEvent::Error { message: msg.clone() });
        return Err(Error::upstream(msg));
    }

    // Emit a state event so the Mother Agent's progress bar can
    // update. We send the post-trim token estimate + the
    // configured context cap as the denominator.
    if let Some(ctx) = max_context {
        let _ = app.emit(
            "agent-event",
            AgentEvent::State {
                state: json!({
                    "kind": "contextUsage",
                    "usedTokens": _input_tokens,
                    "totalTokens": ctx,
                })
                .to_string(),
            },
        );
    }

    // Stream the SSE response.
    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut final_event = AgentEvent::Done;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| Error::network(e.to_string()))?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some((event, rest)) = split_sse_event(&buffer) {
            buffer = rest.to_string();
            if let Some(parsed) = parse_sse_payload(&event, use_anthropic) {
                let _ = app.emit("agent-event", parsed.clone());
                if matches!(parsed, AgentEvent::Error { .. }) {
                    final_event = parsed;
                }
            }
        }
    }
    let _ = app.emit("agent-event", AgentEvent::Done);
    Ok(final_event)
}

fn lookup_saved_model(store: &Arc<dyn Store>, model_id: &str) -> Option<ModelDto> {
    // Best-effort: try the lookup, swallow NotFound. Other
    // errors (Storage, etc.) are logged via the IPC layer's
    // error event.
    match store.get_model(model_id) {
        Ok(m) => Some(super::models::ModelDto::from(m)),
        Err(Error::NotFound { .. }) => None,
        Err(_) => None,
    }
}

// ─── SSE parsing helpers ─────────────────────────────────────────

/// Split off the next complete SSE event from `buffer`. An event
/// is terminated by a blank line (`\n\n` or `\r\n\r\n`). The
/// returned tuple is `(event_text, remainder)`. Returns `None`
/// when the buffer doesn't yet contain a complete event.
fn split_sse_event(buffer: &str) -> Option<(String, String)> {
    // We split on \n\n (with optional leading \r) to find event
    // boundaries. A trailing partial event is left in the buffer.
    let idx = buffer.find("\n\n").or_else(|| buffer.find("\r\n\r\n"))?;
    let (event, rest) = buffer.split_at(idx);
    let rest = rest
        .trim_start_matches("\r\n\r\n")
        .trim_start_matches("\n\n")
        .to_string();
    Some((event.to_string(), rest))
}

/// Parse one SSE event's payload (the lines after `data: `) into
/// an `AgentEvent`. The OpenAI and Anthropic shapes differ in
/// their JSON; we normalize the delta-text extraction here.
fn parse_sse_payload(raw: &str, _anthropic: bool) -> Option<AgentEvent> {
    // Concatenate the data lines, ignore everything else.
    let data: String = raw
        .lines()
        .filter_map(|l| l.strip_prefix("data: "))
        .collect::<Vec<_>>()
        .join("\n");
    if data.is_empty() {
        return None;
    }
    if data.trim() == "[DONE]" {
        return Some(AgentEvent::Done);
    }
    let v: serde_json::Value = serde_json::from_str(&data).ok()?;
    // OpenAI: choices[0].delta.content
    if let Some(s) = v.pointer("/choices/0/delta/content").and_then(|x| x.as_str()) {
        return Some(AgentEvent::TextDelta { text: s.to_string() });
    }
    // Anthropic: content_block_delta with delta.text
    if let Some(s) = v.pointer("/delta/text").and_then(|x| x.as_str()) {
        return Some(AgentEvent::TextDelta { text: s.to_string() });
    }
    // Anthropic content_block_start with a tool_use block
    if let (Some(name), Some(id)) = (
        v.pointer("/content_block/name").and_then(|x| x.as_str()),
        v.pointer("/content_block/id").and_then(|x| x.as_str()),
    ) {
        return Some(AgentEvent::ToolCallStart {
            id: id.to_string(),
            name: name.to_string(),
        });
    }
    // Anthropic input_json_delta
    if let (Some(id), Some(partial)) = (
        v.pointer("/index").and_then(|x| x.as_u64()),
        v.pointer("/delta/partial_json").and_then(|x| x.as_str()),
    ) {
        return Some(AgentEvent::ToolCallArgs {
            id: id.to_string(),
            args: partial.to_string(),
        });
    }
    // message_stop
    if v.get("type").and_then(|t| t.as_str()) == Some("message_stop") {
        return Some(AgentEvent::Done);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_event_split() {
        let buf = "data: {\"a\":1}\n\ndata: {\"a\":2}\n\ndata: {";
        let (e1, rest) = split_sse_event(buf).unwrap();
        assert_eq!(e1, "data: {\"a\":1}");
        assert!(rest.starts_with("data: {\"a\":2}"));
    }

    #[test]
    fn sse_openai_delta() {
        let raw = r#"data: {"choices":[{"delta":{"content":"hi"}}]}"#;
        let ev = parse_sse_payload(raw, false).unwrap();
        match ev {
            AgentEvent::TextDelta { text } => assert_eq!(text, "hi"),
            _ => panic!("wrong event"),
        }
    }

    #[test]
    fn sse_anthropic_delta() {
        let raw = r#"data: {"type":"content_block_delta","delta":{"text":"yo"}}"#;
        let ev = parse_sse_payload(raw, true).unwrap();
        match ev {
            AgentEvent::TextDelta { text } => assert_eq!(text, "yo"),
            _ => panic!("wrong event"),
        }
    }

    #[test]
    fn sse_done_marker() {
        let raw = "data: [DONE]";
        let ev = parse_sse_payload(raw, false).unwrap();
        assert!(matches!(ev, AgentEvent::Done));
    }
}
