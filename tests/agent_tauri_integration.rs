//! Tauri-runtime integration test for `services::agent::send_message`.
//!
//! This is the test the user asked for: "先测后端，再测前后端交互".
//! We spin up a real Tauri app via `tauri::test::mock_app()` and:
//!
//!   1. Subscribe to the `agent-event` channel via `app.listen()`,
//!      collecting every event payload into a `Vec<Value>`.
//!   2. Spawn a tokio task that calls
//!      `send_message(app, store, input)`.
//!   3. Assert that the events arrive in the expected order
//!      (State → TextDelta* → Done), no hang, total time < 15s.
//!
//! This validates the actual code path the production app uses —
//! `app.emit("agent-event", …)` is the same call regardless of
//! whether the AppHandle is `Wry` or `MockRuntime`. If anything in
//! the Rust event-emission pipeline is broken (event name
//! mismatch, async channel full, etc.), this test catches it
//! without needing the WebView.
//!
//! Skipped unless `ECHOBIRD_MINIMAX_KEY` is set.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use echobird_core::services::agent::{send_message, AgentEvent, AgentSendInput};
use echobird_core::storage::memory::InMemoryStore;
use echobird_core::storage::model::{ModelType, NewModel};
use echobird_core::storage::Store;
use serde_json::Value;
use tauri::Listener;

const ANTHROPIC_URL: &str = "https://api.minimaxi.com/anthropic";
const MODEL_ID: &str = "MiniMax-M3";
const MAX_CONTEXT: u64 = 1_000_000;
const MAX_INPUT: u64 = 1_000_000;
const MAX_OUTPUT: u64 = 64_000;

fn build_input(model_internal_id: &str, key: &str, message: &str) -> AgentSendInput {
    AgentSendInput {
        message: message.to_string(),
        model_id: model_internal_id.to_string(),
        base_url: "https://api.minimaxi.com/v1".to_string(),
        api_key: key.to_string(),
        model_name: MODEL_ID.to_string(),
        provider: "anthropic".to_string(),
        anthropic_url: Some(ANTHROPIC_URL.to_string()),
        server_ids: vec![],
        skills: vec![],
        locale: None,
        history: vec![],
    }
}

/// Build an in-memory store with a MiniMax model entry. Returns
/// `(store, internal_id)` — the input must use the same
/// `internal_id` so `send_message`'s `lookup_saved_model` finds
/// the model and emits the post-trim `contextUsage` state event.
fn build_store_with_model() -> (Arc<InMemoryStore>, String) {
    let store = InMemoryStore::new();
    let inserted = store
        .insert_model(NewModel {
            name: "MiniMax CN".to_string(),
            model_id: Some(MODEL_ID.to_string()),
            base_url: "https://api.minimaxi.com/v1".to_string(),
            api_key: String::new(), // api_key on the input, not the model
            anthropic_url: Some(ANTHROPIC_URL.to_string()),
            model_type: ModelType::Cloud,
            max_context_tokens: Some(MAX_CONTEXT),
            max_input_tokens: Some(MAX_INPUT),
            max_output_tokens: Some(MAX_OUTPUT),
        })
        .expect("insert seed model");
    (store, inserted.internal_id)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn send_message_emits_agent_event_through_mock_app() {
    let key = match std::env::var("ECHOBIRD_MINIMAX_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => {
            eprintln!("[skip] ECHOBIRD_MINIMAX_KEY not set");
            return;
        }
    };
    eprintln!("=== Tauri integration: agent-event emission ===");
    eprintln!("Key prefix: {}...", &key[..key.len().min(12)]);
    eprintln!("max_context={MAX_CONTEXT}, max_input={MAX_INPUT}, max_output={MAX_OUTPUT}");

    // 1. Build a Tauri app (mock runtime, no WebView).
    let app = tauri::test::mock_app();
    let app_handle = app.handle().clone();

    // 2. Subscribe to `agent-event` BEFORE we trigger send_message,
    //    so we don't miss early events. We capture the raw JSON
    //    payload (Tauri's event payload is a string) and inspect
    //    the `type` tag to reconstruct the event.
    let received: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    let received_for_handler = received.clone();
    let _unlisten = app_handle.listen("agent-event", move |event| {
        let payload_str = event.payload().to_string();
        eprintln!("[evt raw] {payload_str}");
        match serde_json::from_str::<Value>(&payload_str) {
            Ok(v) => {
                let mut buf = received_for_handler.lock().unwrap();
                buf.push(v);
            }
            Err(e) => eprintln!("[warn] payload not JSON: {e}"),
        }
    });

    // Give the listener a tick to register. The mock runtime
    // wires up listeners synchronously, but the async runtime
    // needs a moment to schedule the registration before we
    // start firing events.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 3. Build input + store. The model_id on the input must
    //    match the seeded store entry's internal_id so
    //    `lookup_saved_model` finds the token caps and emits
    //    the post-trim contextUsage state event.
    let (store, model_internal_id) = build_store_with_model();
    let input = build_input(&model_internal_id, &key, "1+1=");
    eprintln!("model_internal_id: {model_internal_id}");

    // 4. Fire the IPC handler the frontend would call. Bounded
    //    timeout: the production 90s safety net has a backend
    //    counterpart here.
    let started = Instant::now();
    let app_for_send = app_handle.clone();
    let store_for_send: Arc<dyn echobird_core::storage::Store> = store.clone();
    let send_fut = tokio::spawn(async move {
        send_message(&app_for_send, &store_for_send, input).await
    });

    // 5. Wait for the call to return OR for 15s — anything past
    //    that is a hang.
    let result = tokio::time::timeout(Duration::from_secs(15), send_fut).await;
    let elapsed = started.elapsed();
    eprintln!("send_message returned after {:.2?}", elapsed);

    let join_result = result.expect("send_message must not hang past 15s");
    let send_result = join_result.expect("send_message task did not panic");
    eprintln!("send_message result: ok={}", send_result.is_ok());
    if let Err(e) = &send_result {
        eprintln!("send_message error: {e}");
    }

    // Give the event listener a moment to drain queued events
    // that may have arrived after send_message resolved.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 6. Inspect the captured event stream.
    let events = received.lock().unwrap().clone();
    eprintln!("captured {} events", events.len());
    for (i, e) in events.iter().enumerate() {
        eprintln!("  [#{i}] {}", e);
    }

    // 7. Assertions: the order, the content, the terminal event.
    assert!(
        !events.is_empty(),
        "expected at least one agent-event, got none — listener probably not wired"
    );

    // First event MUST be a state event with kind=contextUsage
    // (the post-trim context footprint emit). This proves the
    // Rust code reached the emit path before contacting the
    // upstream AND that the saved-model lookup worked.
    let first = &events[0];
    assert_eq!(
        first["type"], "state",
        "expected first event to be state, got: {first}"
    );
    let state_str = first["state"]
        .as_str()
        .expect("state field is a string in the wire format");
    let parsed: Value = serde_json::from_str(state_str).expect("state JSON parses");
    assert_eq!(parsed["kind"], "contextUsage");
    let used = parsed["usedTokens"].as_u64().unwrap();
    let total = parsed["totalTokens"].as_u64().unwrap();
    assert!(used > 0, "usedTokens must be > 0");
    assert_eq!(total, MAX_CONTEXT);
    eprintln!("contextUsage: used={used} / total={total}");

    // Must have at least one TextDelta with the answer.
    let text_deltas: Vec<String> = events
        .iter()
        .filter_map(|e| {
            if e["type"] == "text_delta" {
                e["text"].as_str().map(String::from)
            } else {
                None
            }
        })
        .collect();
    assert!(
        !text_deltas.is_empty(),
        "expected at least one text_delta (MiniMax answered 1+1)"
    );
    let full_text: String = text_deltas.join("");
    eprintln!("assembled text: {full_text:?}");
    assert!(
        full_text.contains('2'),
        "expected answer to contain '2', got: {full_text:?}"
    );

    // Must have a terminal Done event.
    let has_done = events.iter().any(|e| e["type"] == "done");
    assert!(has_done, "expected a terminal done event so JS spinner clears");

    // No Error events.
    let errors: Vec<&str> = events
        .iter()
        .filter_map(|e| {
            if e["type"] == "error" {
                e["message"].as_str()
            } else {
                None
            }
        })
        .collect();
    assert!(errors.is_empty(), "expected no error events, got: {errors:?}");

    eprintln!(
        "PASS: send_message emitted {} events through mock_app in {:.2?}",
        events.len(),
        elapsed
    );

    // Suppress unused import warning
    let _ = std::marker::PhantomData::<AgentEvent>;
}
