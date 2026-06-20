//! Regression test: verify the clean-room `ai_career` stub commands
//! return values whose **shape** matches the frontend TypeScript
//! types in `src/api/aiCareer.ts`. The earlier stubs returned wrapper
//! objects (`{"days":[],"byFamily":{}}`, `{"items":[]}`, `{"bytes":0}`)
//! that the frontend iterates as plain arrays / treats as a plain
//! number. The bug surfaced in production as:
//!   * Heatmap + 4 of 5 stat cards stuck at 0
//!     (`for of` on an object throws `TypeError`, swallowed by
//!     `catch(() => {})`, `buckets` keeps its EMPTY initial state)
//!   * "约累计 Token" rendering as `NaNB`
//!     (object * 12 = NaN, `formatCompact(NaN)` falls through to the
//!     `else` branch with `n / 1_000_000_000` and emits "NaN" + "B")
//!
//! The fix: stubs now return the bare values the frontend expects
//! (`Vec<serde_json::Value>`, `Vec<serde_json::Value>`, `u64`). If a
//! future refactor reintroduces a wrapper object, this test will fail
//! with a clear "shape mismatch" assertion.

use echobird_core::commands::ai_career;

#[test]
fn ai_career_heatmap_returns_array() {
    // The frontend does `for (const e of entries)` on the result, so
    // it must be a JSON array. A bare object (the old stub shape)
    // would crash `for of` and leave the heatmap empty.
    let result = ai_career::ai_career_heatmap().expect("ai_career_heatmap ok");
    let v: serde_json::Value = serde_json::to_value(&result).unwrap();
    assert!(
        v.is_array(),
        "ai_career_heatmap must return a JSON array, got {:?}",
        v,
    );
}

#[test]
fn ai_career_family_history_returns_array() {
    // The frontend does `setRows(toRows(list, now))` and `list.length`
    // on the result. An object (the old stub shape) breaks both.
    let result = ai_career::ai_career_family_history("claude".to_string(), 0, 30)
        .expect("ai_career_family_history ok");
    let v: serde_json::Value = serde_json::to_value(&result).unwrap();
    assert!(
        v.is_array(),
        "ai_career_family_history must return a JSON array, got {:?}",
        v,
    );
}

#[test]
fn ai_career_token_bytes_returns_number() {
    // The frontend does `tokenBytes * 12`. If `tokenBytes` is an
    // object, the multiplication yields `NaN`, and
    // `formatCompact(NaN)` renders as "NaNB" — the user-visible bug
    // that prompted this fix. Must be a JSON number.
    let result = ai_career::ai_career_token_bytes().expect("ai_career_token_bytes ok");
    let v: serde_json::Value = serde_json::to_value(&result).unwrap();
    assert!(
        v.is_number(),
        "ai_career_token_bytes must return a JSON number, got {:?}",
        v,
    );
    // The value must be a non-negative integer (or 0). NaN/Infinity
    // would be invalid here too.
    assert!(v.as_u64().is_some(), "token_bytes must be a u64, got {:?}", v);
}
