//! Round 6 follow-up regression tests: command signatures
//! (arg lists) that the frontend uses but the clean-room stubs
//! got wrong.
//!
//! Bugs locked here:
//!   * L6 — ssh_test_connection: previous stub took
//!     (_id: String), frontend passes 4 args (host, port,
//!     username, password).
//!   * L7 — parasite_abort / parasite_reset: previous stubs
//!     took zero args; frontend passes agentId.
//!     parasite_abort also returns Promise<boolean>.
//!   * L8 — get_system_info / sysinfo(): previous stub
//!     returned only {os, arch}; frontend typed the result as
//!     SystemInfo with hasNvidiaGpu / hasAmdGpu / gpuName /
//!     gpuVramGb fields and used them to gate the runtime-
//!     options panel.

use echobird_core::commands::app;
use echobird_core::commands::parasite;
use echobird_core::commands::ssh;

#[test]
fn ssh_test_connection_accepts_four_args_and_returns_object() {
    // If the old single-arg signature was in place this would
    // fail to compile (or fail at the IPC layer with
    // "function takes 1 argument, got 4").
    let r = ssh::ssh_test_connection(
        "127.0.0.1".to_string(),
        22,
        "ayden".to_string(),
        "test-pwd".to_string(),
    )
    .expect("ssh_test_connection ok");
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    assert!(
        v.is_object() && v.get("success").is_some(),
        "ssh_test_connection must return {{ success, ... }}, got {:?}",
        v,
    );
}

#[test]
fn parasite_abort_accepts_agent_id_and_returns_bool() {
    let r = parasite::parasite_abort("claude-code".to_string())
        .expect("parasite_abort ok");
    // Frontend types as Promise<boolean>. The wire shape is
    // serialised as `true` or `false` (Rust `bool` → JSON
    // boolean).
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    assert!(
        v.is_boolean(),
        "parasite_abort must return a JSON boolean, got {:?}",
        v,
    );
}

#[test]
fn parasite_reset_accepts_agent_id() {
    // Just assert the call succeeds — the page only uses the
    // result to clear its "Reset" button loading state.
    let r = parasite::parasite_reset("claude-code".to_string());
    assert!(r.is_ok(), "parasite_reset must accept agent_id and return Ok, got {:?}", r);
}

#[test]
fn sysinfo_returns_full_system_info_shape() {
    // The page uses os, hasNvidiaGpu, and gpuVramGb; all three
    // must be present (even if the GPU values are null/false
    // in the clean-room build).
    let v = app::get_system_info().expect("get_system_info ok");
    let j: serde_json::Value = serde_json::to_value(&v).unwrap();
    assert!(j.get("os").map(|x| x.is_string()).unwrap_or(false),
        "os must be a string, got {:?}", j);
    assert!(j.get("arch").map(|x| x.is_string()).unwrap_or(false),
        "arch must be a string, got {:?}", j);
    assert!(j.get("hasNvidiaGpu").map(|x| x.is_boolean()).unwrap_or(false),
        "hasNvidiaGpu must be a boolean, got {:?}", j);
    assert!(j.get("hasAmdGpu").map(|x| x.is_boolean()).unwrap_or(false),
        "hasAmdGpu must be a boolean, got {:?}", j);
    // gpuName / gpuVramGb may be null or string/number — they
    // must be present, not undefined.
    assert!(j.get("gpuName").is_some(),
        "gpuName must be present (null or string), got {:?}", j);
    assert!(j.get("gpuVramGb").is_some(),
        "gpuVramGb must be present (null or number), got {:?}", j);
}
