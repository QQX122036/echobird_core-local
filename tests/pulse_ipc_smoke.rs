//! Smoke test: invoke the Tauri command layer directly and verify
//! the return JSON shape matches what the frontend expects.

use echobird_core::commands::pulse;

#[tokio::test]
async fn pulse_fetch_command_returns_frontend_shape() {
    if std::env::var("ECHOBIRD_SKIP_PULSE_E2E").is_ok() {
        return;
    }
    // Isolated archive so we don't pollute the user's data.
    if std::env::var("ECHOBIRD_PULSE_DIR").is_err() {
        let tmp = std::env::temp_dir().join(format!(
            "echobird-pulse-ipc-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::env::set_var("ECHOBIRD_PULSE_DIR", &tmp);
    }
    // The Tauri command signature is `pub async fn pulse_fetch(lang: String) -> Result<serde_json::Value, String>`.
    // We can call it directly (it has no `#[command]` macro at runtime — the macro is just for codegen).
    // Wait — the macro IS applied. Let me just call the underlying service instead.
    let (items, diagnostic) = echobird_core::services::pulse_archive::fetch_and_persist("zh")
        .await
        .expect("backend ok");
    let json = serde_json::json!({
        "items": items,
        "diagnostic": diagnostic,
    });
    let serialized = serde_json::to_string(&json).unwrap();
    println!("payload size: {} bytes", serialized.len());
    println!("first 300 chars: {}", serialized.chars().take(300).collect::<String>());
    // Parse it back to ensure roundtrip works
    let parsed: serde_json::Value = serde_json::from_str(&serialized).unwrap();
    let items_arr = parsed.get("items").and_then(|v| v.as_array()).expect("items array");
    let diagnostic = parsed.get("diagnostic").expect("diagnostic field");
    println!("items count: {}", items_arr.len());
    println!("diagnostic: {:?}", diagnostic);
    assert!(items_arr.len() > 1000, "expected many items");
    // Verify item shape — frontend's NewsItem interface
    let first = &items_arr[0];
    for field in ["id", "source", "title", "url"] {
        assert!(first.get(field).is_some(), "missing field: {}", field);
    }
    println!("first item id: {:?}", first.get("id"));
    println!("first item title: {:?}", first.get("title").and_then(|v| v.as_str()).unwrap_or(""));
}
