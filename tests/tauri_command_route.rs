//! Verify that the pulse Tauri command functions have the right signatures
//! for Tauri 2's `tauri::generate_handler!` to register them correctly.
//!
//! The signatures must match what `lib.rs` registers in `invoke_handler`.
//! This is a compile-time-only check: if the actual function signature
//! drifts from the registered one, this test fails to build.

use echobird_core::commands::pulse;

#[test]
fn pulse_fetch_signature_matches_handler() {
    // `lib.rs` registers `commands::pulse::pulse_fetch` as a no-arg
    // handler accepting `{ lang: String }` from the frontend.
    let _: fn(String) -> _ = pulse::pulse_fetch;
}

#[test]
fn pulse_save_signature_matches_handler() {
    // `lib.rs` registers `commands::pulse::pulse_save` accepting
    // `{ lang: String, items: Vec<PulseItem> }`.
    let _: fn(String, Vec<echobird_core::services::pulse_archive::PulseItem>) -> _ = pulse::pulse_save;
}

#[test]
fn pulse_load_all_signature_matches_handler() {
    // `lib.rs` registers `commands::pulse::pulse_load_all` accepting
    // `{ lang: String }`.
    let _: fn(String) -> _ = pulse::pulse_load_all;
}
