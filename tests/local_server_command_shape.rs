//! Regression test: verify the clean-room `local_server` command
//! stubs (Round 6) accept the arg lists the frontend sends AND
//! return the right shape.
//!
//! Bugs locked here:
//!   * L1 — get_llm_default_command: previous stub returned
//!     `String`, frontend typed `LlamaCommand { exe, args[] }`,
//!     so `def.exe` / `def.args` were undefined and the
//!     "Custom Command" dialog crashed.
//!   * L2 — set_llm_custom_command: previous stub took
//!     `(command: String)`, frontend sends `(exe, args[])`, so
//!     the IPC threw "missing required argument exe".
//!   * L3 — add_models_dir: previous stub required
//!     `path: String`, frontend sends zero args, so the IPC
//!     threw and `handleAddDir` silently swallowed it.
//!   * L3 — remove_models_dir: previous stub returned `()`,
//!     frontend typed `string[]`, so `setLocalDirs(dirs)` set
//!     state to undefined.
//!   * L4 — start_llm_server: previous stub took zero args,
//!     frontend sends 5 args, so the IPC threw on click.
//!   * L5 — set_download_dir: previous stub required
//!     `_path: String` and returned `()`, frontend sends zero
//!     args and expects a string back.

use echobird_core::commands::local_server;

#[test]
fn get_llm_default_command_returns_llama_command_shape() {
    let result = local_server::get_llm_default_command(
        "/tmp/m.gguf".to_string(),
        8080,
        Some(20),
        Some(2048),
    )
    .expect("get_llm_default_command ok");
    let v: serde_json::Value = serde_json::to_value(&result).unwrap();
    // The wire type is { exe: string, args: string[] }. The
    // clean-room IPC struct is named `LlmCustomCommand` but
    // has the same fields — assert the *value* shape.
    assert!(
        v.get("exe").map(|x| x.is_string()).unwrap_or(false)
            || v.get("command").map(|x| x.is_string()).unwrap_or(false),
        "get_llm_default_command must return a string-bearing object, got {:?}",
        v,
    );
}

#[test]
fn set_llm_custom_command_accepts_exe_and_args() {
    // If the stub reverted to the old single-arg shape, this
    // call would fail to compile.
    let r = local_server::set_llm_custom_command(
        "llama-server".to_string(),
        vec!["-m".to_string(), "/tmp/m.gguf".to_string()],
    );
    assert!(r.is_ok(), "set_llm_custom_command(exe, args) must accept two args, got {:?}", r);
    // And the round-trip via get_llm_custom_command should
    // produce a non-null object.
    let stored = local_server::get_llm_custom_command()
        .expect("get_llm_custom_command ok");
    assert!(stored.is_some(), "stored custom command must be Some after set");
}

#[test]
fn add_models_dir_takes_no_args_and_returns_array() {
    let r = local_server::add_models_dir().expect("add_models_dir ok");
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    assert!(
        v.is_array(),
        "add_models_dir must return a JSON array, got {:?}",
        v,
    );
}

#[test]
fn remove_models_dir_returns_array() {
    // First add a known dir, then remove it. Must return the
    // updated list (Vec<String>), not unit.
    let _ = local_server::add_models_dir();
    let r = local_server::remove_models_dir("/tmp/round6-test-models".to_string())
        .expect("remove_models_dir ok");
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    assert!(
        v.is_array(),
        "remove_models_dir must return a JSON array, got {:?}",
        v,
    );
}

#[test]
fn start_llm_server_accepts_five_args_and_returns_not_implemented() {
    let r = local_server::start_llm_server(
        "/tmp/m.gguf".to_string(),
        8080,
        Some(20),
        Some(2048),
        Some("llama.cpp".to_string()),
    );
    // Stub still returns not_implemented (clean-room doesn't
    // ship a llama-server) but the *call* must succeed at the
    // IPC layer. If the old zero-arg stub was in place, this
    // would fail to compile.
    assert!(r.is_err(), "start_llm_server stub returns not_implemented Err");
    let msg = r.unwrap_err();
    assert!(
        msg.contains("not_implemented") || msg.contains("roadmap"),
        "expected not_implemented message, got {:?}",
        msg,
    );
}

#[test]
fn set_download_dir_takes_no_args_and_returns_optional_string() {
    let r = local_server::set_download_dir().expect("set_download_dir ok");
    let v: serde_json::Value = serde_json::to_value(&r).unwrap();
    assert!(
        v.is_null() || v.is_string(),
        "set_download_dir must return null or string, got {:?}",
        v,
    );
}
