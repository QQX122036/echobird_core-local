//! End-to-end smoke tests for the v5.3.4 token-limit path:
//!
//! 1. SqliteStore round-trip of maxContextTokens / maxInputTokens /
//!    maxOutputTokens (the persistence layer the frontend's "Add
//!    Model" form hits).
//! 2. context_window::clamp_max_tokens enforces maxOutputTokens
//!    on the per-request `max_tokens` field.
//! 3. context_window::trim_to_input_cap evicts oldest non-system
//!    messages when the input would exceed maxInputTokens.

use echobird_core::services::context_window::{clamp_max_tokens, trim_to_input_cap};
use echobird_core::storage::model::ModelType;
use echobird_core::storage::sqlite::SqliteStore;
use echobird_core::storage::{NewModel, Store};
use std::path::PathBuf;

#[test]
fn minimax_cn_1m_round_trips_token_limits() {
    let path = PathBuf::from("/tmp/test_minimax_1m.sqlite");
    let _ = std::fs::remove_file(&path);
    let store = SqliteStore::open(&path).expect("open store");

    let m = store
        .insert_model(NewModel {
            name: "MiniMax M3 (1M Context)".into(),
            model_id: Some("MiniMax-M3".into()),
            base_url: "https://api.minimaxi.com/v1".into(),
            api_key: "test-key".into(),
            anthropic_url: None,
            model_type: ModelType::Cloud,
            max_context_tokens: Some(1_000_000),
            max_input_tokens: Some(900_000),
            max_output_tokens: Some(32_000),
        })
        .expect("insert model");

    let read = store.get_model(&m.internal_id).expect("get model");

    assert_eq!(read.max_context_tokens, Some(1_000_000), "context window");
    assert_eq!(read.max_input_tokens, Some(900_000), "max input");
    assert_eq!(read.max_output_tokens, Some(32_000), "max output");

    println!(
        "Persistence OK: {} -> ctx={:?}, in={:?}, out={:?}",
        read.name, read.max_context_tokens, read.max_input_tokens, read.max_output_tokens
    );
}

#[test]
fn clamp_uses_user_max_output_when_caller_omits() {
    // Frontend omits max_tokens → backend should default to the
    // user's configured max_output_tokens. This is the headline
    // behavior: a 1M-context model with 32K output config never
    // asks the upstream for more than 32K output tokens.
    let clamped = clamp_max_tokens(None, Some(32_000));
    assert_eq!(clamped, Some(32_000), "must use configured 32K cap");

    // Caller asks for more than the cap → backend clamps down.
    let clamped = clamp_max_tokens(Some(64_000), Some(32_000));
    assert_eq!(clamped, Some(32_000), "must clamp 64K down to 32K cap");

    // Caller asks for less than the cap → backend leaves it
    // alone (the caller is allowed to be more restrictive).
    let clamped = clamp_max_tokens(Some(8_000), Some(32_000));
    assert_eq!(clamped, Some(8_000), "must not over-clamp from below");

    println!("Clamp OK: 32K cap enforced on the upstream request");
}

#[test]
fn trim_drops_oldest_to_fit_user_max_input() {
    // System prompt + 4 history turns + current turn. With a
    // max_input_tokens of 50 the trim helper should drop enough
    // to fit. The system prompt (index 0) and the current
    // turn (last) must survive.
    let mut messages: Vec<String> = vec![
        "you are a helpful assistant".to_string(),
        "msg 1: this is a moderately long user turn about something".to_string(),
        "msg 1 reply: a fairly long assistant response covering the topic".to_string(),
        "msg 2: another user turn with more words to push the budget".to_string(),
        "msg 2 reply: assistant response also adds characters to the count".to_string(),
        "current question from the user that we want to definitely keep".to_string(),
    ];
    let dropped = trim_to_input_cap(&mut messages, Some(50));
    assert!(dropped > 0, "must drop something to fit");
    assert_eq!(messages[0], "you are a helpful assistant", "system prompt survives");
    assert_eq!(
        messages.last().unwrap(),
        "current question from the user that we want to definitely keep",
        "current turn survives"
    );

    println!("Trim OK: dropped {dropped} oldest non-system message(s) to fit 50-token cap");
}
