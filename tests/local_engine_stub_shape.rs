//! Regression test: verify the clean-room `local_server` stub commands
//! return values whose **shape** matches the frontend TypeScript
//! types in `src/api/localServer.ts`. The earlier stubs returned
//! objects with the wrong field names (`{"available":false,"name":null,
//! "vramGb":null}` / `{"available":false}` / `{"installed":false,
//! "version":null}`) that the frontend typed as
//! `{ gpuName: string, gpuVramGb: number } | null` and
//! `LocalEngineStatus { engines: LocalEngineEntry[] }`. The bugs
//! surfaced in production as:
//!   * GPU card stuck at "0 GB VRAM" — `info.gpuVramGb` was `undefined`,
//!     the page's `?? 0` default kicked in, and the auto-probe never
//!     re-fired because the page thought GPU was already known.
//!   * Engine status row stuck on "checking" forever — `status.engines`
//!     was `undefined`, `status.engines.find(...)` threw `TypeError`,
//!     was swallowed by the page's `.catch`, and the page kept showing
//!     the loading spinner.
//!
//! The fix: stubs now return `serde_json::Value::Null` for the GPU
//! helpers and `{"engines": []}` for the engine status — both bare
//! shapes the frontend can handle cleanly.

use echobird_core::commands::local_server;

#[test]
fn detect_gpu_returns_null_or_correct_shape() {
    // The frontend types this as
    // `{ gpuName: string, gpuVramGb: number } | null`. The
    // "GPU not yet detected" sentinel is `null`, so the page can
    // fall through to its `detect_gpu` auto-probe.
    let result = local_server::detect_gpu().expect("detect_gpu ok");
    let v: serde_json::Value = serde_json::to_value(&result).unwrap();
    assert!(
        v.is_null()
            || (v.get("gpuName").map(|x| x.is_string()).unwrap_or(false)
                && v.get("gpuVramGb").map(|x| x.is_number()).unwrap_or(false)),
        "detect_gpu must be null or {{gpuName, gpuVramGb}}, got {:?}",
        v,
    );
}

#[test]
fn get_gpu_info_returns_null_or_correct_shape() {
    // Same contract as `detect_gpu` — the frontend uses the same
    // type and reads the same fields.
    let result = local_server::get_gpu_info().expect("get_gpu_info ok");
    let v: serde_json::Value = serde_json::to_value(&result).unwrap();
    assert!(
        v.is_null()
            || (v.get("gpuName").map(|x| x.is_string()).unwrap_or(false)
                && v.get("gpuVramGb").map(|x| x.is_number()).unwrap_or(false)),
        "get_gpu_info must be null or {{gpuName, gpuVramGb}}, got {:?}",
        v,
    );
}

#[test]
fn get_local_engine_status_has_engines_array() {
    // The frontend types this as
    // `LocalEngineStatus { engines: LocalEngineEntry[] }` and does
    // `status.engines.find(...)` on it. The old stub returned
    // `{"installed":false,"version":null}` — `.engines` was
    // `undefined`, the `.find` threw, the `.catch` swallowed it,
    // and the page kept showing the loading spinner forever.
    let result = local_server::get_local_engine_status()
        .expect("get_local_engine_status ok");
    let v: serde_json::Value = serde_json::to_value(&result).unwrap();
    assert!(
        v.get("engines").map(|x| x.is_array()).unwrap_or(false),
        "get_local_engine_status must have an `engines` JSON array, got {:?}",
        v,
    );
}
