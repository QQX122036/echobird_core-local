//! Agent chat-completion forwarding.
//!
//! The public `agent_send_message` IPC kicks off a stream of
//! `text_delta` / `thinking` / `tool_call_*` / `done` / `error`
//! events to the frontend (see `src/api/types.ts`'s
//! `AgentEvent`). This module is the Rust side of that pipeline.
//!
//! Token-limit integration (v5.3.4):
//!   * Per-request `max_tokens` is clamped via
//!     [`context_window::clamp_max_tokens`] to the user-configured
//!     `max_output_tokens`. This is the headline behavior the
//!     user sees when they save 32K output on a 1M-context model:
//!     the upstream never gets asked to generate more than 32K
//!     tokens per turn, no matter what the caller asked for.
//!   * If the IPC sends a `history` array, the message list is
//!     trimmed to fit `max_input_tokens` via
//!     [`context_window::trim_to_input_cap`]. Older non-system
//!     messages are dropped first, with a 5% safety margin so
//!     the upstream doesn't 400 on a tight estimate.
//!   * A `state` event with `{ kind: "contextUsage", usedTokens,
//!     totalTokens }` is emitted before the upstream call, so
//!     the Mother Agent UI can show a real percentage against
//!     `max_context_tokens` instead of a hardcoded 128K/200K
//!     denominator.
//!
//! Streaming is real: we use `reqwest`'s response stream and
//! emit one `text_delta` per Server-Sent Event (SSE) chunk.
//! Anthropic and OpenAI both speak SSE with slightly different
//! envelope shapes; the parser handles both.

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryMessage {
    pub role: String,
    pub content: String,
}

impl AsRef<str> for HistoryMessage {
    fn as_ref(&self) -> &str {
        self.content.as_str()
    }
}

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
    /// Optional conversation history. When present, `trim_to_input_cap`
    /// walks the full list (oldest non-system message first) until
    /// the running estimate fits under the model’s
    /// `max_input_tokens`. The first entry is treated as the
    /// system prompt and is never evicted.
    #[serde(default)]
    pub history: Vec<HistoryMessage>,
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
/// frontend via Tauri events on the `agent-event` channel.
/// Returns the final event for logging; the IPC contract is
/// "fire and stream" so callers (`commands::agent`) don't
/// usually inspect the return value.
pub async fn send_message<R: Runtime>(
    app: &AppHandle<R>,
    store: &Arc<dyn Store>,
    input: AgentSendInput,
) -> CoreResult<AgentEvent> {
    // Resolve the saved model (if any) for its token caps. If
    // the caller passed an ad-hoc baseUrl that doesn't match a
    // saved model, we skip the cap and the UI shows a hint.
    let saved_model = lookup_saved_model(store, &input.model_id);
    let (max_input, max_output, max_context) = match &saved_model {
        Some(m) => (m.max_input_tokens, m.max_output_tokens, m.max_context_tokens),
        None => (None, None, None),
    };

    // Build the message list we'll actually send upstream. The
    // history (if any) comes first; the current user message
    // is appended at the end. The first history entry is
    // treated as a system prompt and is not evictable — the
    // trim helper preserves it by contract.
    let mut messages: Vec<HistoryMessage> = input.history.clone();
    messages.push(HistoryMessage {
        role: "user".to_string(),
        content: input.message.clone(),
    });

    // Apply the input cap. The trim keeps index 0 (system) and
    // drops oldest non-system messages until the running
    // estimate fits under the cap with 5% safety.
    let dropped = trim_to_input_cap(&mut messages, max_input);
    if dropped > 0 {
        log::info!(
            "agent: trimmed {dropped} oldest non-system message(s) to fit \
             max_input_tokens={}",
            max_input.unwrap_or(0)
        );
    }
    let used_tokens = estimate_messages_tokens(&messages);

    // Choose protocol by URL pattern. The public agent tries
    // Anthropic first and falls back to OpenAI on 400; the
    // clean-room build dispatches on the URL alone and never
    // mid-flight switches (matches v5.2.0+ behavior).
    let use_anthropic = input
        .anthropic_url
        .as_deref()
        .map(|u| u.contains("anthropic") || u.contains("/v1/messages"))
        .unwrap_or(false);
    let url = if use_anthropic {
        input
            .anthropic_url
            .clone()
            .unwrap_or_else(|| input.base_url.clone())
    } else {
        input.base_url.clone()
    };

    // Clamp the per-request `max_tokens` to the configured
    // `max_output_tokens`. The default 4096 is what the
    // upstream's own SDK sends when the caller omits the field;
    // we always send an explicit value so the cap is
    // unambiguous.
    let requested_max_tokens = 4096u32;
    let max_tokens = clamp_max_tokens(Some(requested_max_tokens), max_output);

    // Build the OpenAI-shaped messages array from the
    // *trimmed* message list so the upstream actually sees
    // the result of `trim_to_input_cap`. This is the array the
    // upstream will be billed against and the array that
    // counts toward `max_input_tokens`.
    let openai_messages: Vec<serde_json::Value> = messages
        .iter()
        .map(|m| json!({"role": m.role, "content": m.content}))
        .collect();

    let body = json!({
        "model": input.model_id,
        "messages": openai_messages,
        "max_tokens": max_tokens,
        "stream": true,
    });

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

    // Emit the post-trim context-usage state event so the
    // Mother Agent progress bar can update against the real cap.
    if let Some(ctx) = max_context {
        let _ = app.emit(
            "agent-event",
            AgentEvent::State {
                state: json!({
                    "kind": "contextUsage",
                    "usedTokens": used_tokens,
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
    let idx = buffer.find("\n\n").or_else(|| buffer.find("\r\n\r\n"))?;
    let (event, rest) = buffer.split_at(idx);
    let rest = rest
        .trim_start_matches("\r\n\r\n")
        .trim_start_matches("\n\n")
        .to_string();
    Some((event.to_string(), rest))
}

/// Parse one SSE event's payload (the lines after `data: `)
/// into an `AgentEvent`. The `anthropic` flag dispatches to
/// the right shape matcher. Both matchers share the
/// `[DONE]`/`message_stop`/`response.done` terminal check.
fn parse_sse_payload(raw: &str, anthropic: bool) -> Option<AgentEvent> {
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
    if anthropic {
        parse_anthropic_sse(&v)
    } else {
        parse_openai_sse(&v)
    }
}

/// OpenAI-shape parser. Handles both Chat Completions
/// (`choices[0].delta.content`) and the Responses API
/// (top-level `delta: "..."` string + `response.done`).
fn parse_openai_sse(v: &serde_json::Value) -> Option<AgentEvent> {
    // Chat Completions: choices[0].delta.content (string or null)
    if let Some(s) = v.pointer("/choices/0/delta/content").and_then(|x| x.as_str()) {
        return Some(AgentEvent::TextDelta { text: s.to_string() });
    }
    // Responses API: top-level `delta` is a string when it's a
    // text delta. (For tool-call deltas it's an object, which
    // `as_str()` rejects and we fall through to `None` — we
    // don't yet surface Responses-API tool calls, but we also
    // don't misread them as text.)
    if let Some(s) = v.get("delta").and_then(|x| x.as_str()) {
        return Some(AgentEvent::TextDelta { text: s.to_string() });
    }
    // Responses API done marker
    if v.get("type").and_then(|t| t.as_str()) == Some("response.done") {
        return Some(AgentEvent::Done);
    }
    // Chat Completions done marker
    if v.get("type").and_then(|t| t.as_str()) == Some("message_stop") {
        return Some(AgentEvent::Done);
    }
    None
}

/// Anthropic Messages API parser. Handles content_block_delta
/// (text + tool-use input_json_delta), content_block_start
/// (tool_use), and the message_stop terminal.
fn parse_anthropic_sse(v: &serde_json::Value) -> Option<AgentEvent> {
    // content_block_delta with delta.text
    if let Some(s) = v.pointer("/delta/text").and_then(|x| x.as_str()) {
        return Some(AgentEvent::TextDelta { text: s.to_string() });
    }
    // content_block_start with a tool_use block
    if let (Some(name), Some(id)) = (
        v.pointer("/content_block/name").and_then(|x| x.as_str()),
        v.pointer("/content_block/id").and_then(|x| x.as_str()),
    ) {
        return Some(AgentEvent::ToolCallStart {
            id: id.to_string(),
            name: name.to_string(),
        });
    }
    // input_json_delta: streams the tool-call args. `index`
    // identifies which tool-use block is being streamed.
    if let (Some(index), Some(partial)) = (
        v.get("index").and_then(|x| x.as_u64()),
        v.pointer("/delta/partial_json").and_then(|x| x.as_str()),
    ) {
        return Some(AgentEvent::ToolCallArgs {
            id: index.to_string(),
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

    #[test]
    fn sse_responses_delta() {
        // OpenAI Responses API: {type: "response.output_text.delta", delta: "..."}
        let raw =
            r#"data: {"type":"response.output_text.delta","delta":"world"}"#;
        let ev = parse_sse_payload(raw, false).unwrap();
        match ev {
            AgentEvent::TextDelta { text } => assert_eq!(text, "world"),
            _ => panic!("wrong event"),
        }
    }

    #[test]
    fn trim_keeps_system_and_user_message() {
        // Reproduce the message-list construction path that
        // `send_message` uses: history + current user message,
        // trimmed to `max_input_tokens`. The system prompt
        // (index 0) must survive; the current user message
        // (last) must survive; oldest middle messages are
        // dropped.
        let mut messages: Vec<HistoryMessage> = vec![
            HistoryMessage { role: "system".into(), content: "you are helpful".into() },
            HistoryMessage { role: "user".into(), content: "old-1".into() },
            HistoryMessage { role: "assistant".into(), content: "old-1-reply".into() },
            HistoryMessage { role: "user".into(), content: "old-2".into() },
            HistoryMessage { role: "assistant".into(), content: "old-2-reply".into() },
            HistoryMessage { role: "user".into(), content: "current question".into() },
        ];
        let dropped = trim_to_input_cap(&mut messages, Some(20));
        assert!(dropped > 0);
        // System + current question both survive.
        assert_eq!(messages[0].role, "system");
        assert_eq!(messages.last().unwrap().content, "current question");
        assert_eq!(messages.last().unwrap().role, "user");
    }

    #[test]
    fn trim_skipped_when_cap_none_or_under() {
        // No cap: no trim.
        let mut messages: Vec<HistoryMessage> = vec![
            HistoryMessage { role: "system".into(), content: "sys".into() },
            HistoryMessage { role: "user".into(), content: "hi".into() },
        ];
        assert_eq!(trim_to_input_cap(&mut messages, None), 0);
        assert_eq!(messages.len(), 2);

        // Big cap: no trim.
        let mut messages: Vec<HistoryMessage> = vec![
            HistoryMessage { role: "system".into(), content: "sys".into() },
            HistoryMessage { role: "user".into(), content: "hi".into() },
        ];
        assert_eq!(trim_to_input_cap(&mut messages, Some(10_000)), 0);
        assert_eq!(messages.len(), 2);
    }

    #[test]
    fn sse_openai_chat_done() {
        // Chat Completions: type=message_stop is the terminal.
        let raw = r#"data: {"type":"message_stop"}"#;
        let ev = parse_sse_payload(raw, false).unwrap();
        assert!(matches!(ev, AgentEvent::Done));
    }

    #[test]
    fn sse_openai_empty_delta_falls_through() {
        // Chat Completions heartbeat: choices[0].delta is an
        // object with no content key. The parser must NOT
        // emit an empty text_delta and must NOT crash.
        let raw = r#"data: {"choices":[{"delta":{}}]}"#;
        assert!(parse_sse_payload(raw, false).is_none());
    }

    #[test]
    fn sse_anthropic_tool_use_start() {
        let raw = r#"data: {"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_abc","name":"get_weather"}}"#;
        let ev = parse_sse_payload(raw, true).unwrap();
        match ev {
            AgentEvent::ToolCallStart { id, name } => {
                assert_eq!(id, "toolu_abc");
                assert_eq!(name, "get_weather");
            }
            _ => panic!("wrong event"),
        }
    }

    #[test]
    fn sse_anthropic_input_json_delta() {
        let raw = r#"data: {"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"city\":\"SF\"}"}}"#;
        let ev = parse_sse_payload(raw, true).unwrap();
        match ev {
            AgentEvent::ToolCallArgs { id, args } => {
                assert_eq!(id, "1");
                assert!(args.contains("SF"));
            }
            _ => panic!("wrong event"),
        }
    }

    #[test]
    fn sse_anthropic_message_stop() {
        let raw = r#"data: {"type":"message_stop"}"#;
        let ev = parse_sse_payload(raw, true).unwrap();
        assert!(matches!(ev, AgentEvent::Done));
    }

    #[test]
    fn sse_dispatch_isolates_anthropic_from_openai() {
        // An Anthropic event must NOT be misread by the OpenAI
        // parser (and vice versa) now that parse_sse_payload
        // dispatches on the protocol flag.
        let anthropic_event = r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#;
        // If we lie and call the Anthropic-shaped event with
        // anthropic=false, we should get None — the OpenAI
        // parser doesn't know about /delta/text.
        assert!(parse_sse_payload(anthropic_event, false).is_none());

        let openai_event = r#"data: {"type":"response.output_text.delta","delta":"hi"}"#;
        // And the OpenAI Responses delta must NOT be misread
        // by the Anthropic parser.
        assert!(parse_sse_payload(openai_event, true).is_none());
    }
}
