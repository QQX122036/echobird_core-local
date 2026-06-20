//! AI Career / Mother Agent hints IPC. All clean-room stubs that
//! return shape-compatible empty data so the UI's heatmap and
//! hints panels don't crash. A future build can re-implement
//! these against the same SQLite store.

use tauri::command;

use crate::commands::ipc;


#[command]
pub fn ai_career_heatmap() -> Result<Vec<serde_json::Value>, String> {
    // Shape-compatible stub: the frontend types this as
    // `HeatmapEntry[]` (see `src/api/aiCareer.ts`). Returning the
    // wrapper object `{"days":[],"byFamily":{}}` was a real bug —
    // `entriesToBuckets()` does `for of` on the value, which throws
    // `TypeError` on an object, gets swallowed by the page-level
    // `.catch(() => {})`, and leaves the heatmap + 4 of the 5
    // stat cards stuck at 0 forever. The clean-room stub contract
    // is "return shape-compatible empty data so the UI doesn't
    // crash" — returning the bare array honours that contract.
    ipc(Ok::<Vec<serde_json::Value>, _>(Vec::new()))
}

#[command]
pub fn ai_career_family_history(
    _family: String,
    _offset: u32,
    _limit: u32,
) -> Result<Vec<serde_json::Value>, String> {
    // Shape-compatible stub: the frontend types this as
    // `SavedSession[]`. Returning `{"items":[]}` was a bug — the
    // page does `setRows(toRows(list, now))` and the row map
    // chokes on an object, so the right panel stayed on the
    // skeleton forever and the page fell through to "暂无会话记录"
    // only by accident. The clean-room stub contract is the bare
    // empty array.
    ipc(Ok::<Vec<serde_json::Value>, _>(Vec::new()))
}

#[command]
pub fn ai_career_token_bytes() -> Result<u64, String> {
    // Shape-compatible stub: the frontend types this as `number`
    // and uses it to estimate cumulative tokens. Returning
    // `{"bytes": 0}` was a real bug — `tokenBytes * 12` evaluates
    // to `NaN` (object * number), `Math.round(NaN) = NaN`, and
    // `formatCompact(NaN) = "NaNB"` (the trailing B comes from the
    // `n < 1_000_000_000 → else → divide by 1B → toFixed(1) → "NaN" + "B"`
    // branch in heatmapData.ts::formatCompact).
    ipc(Ok::<u64, _>(0))
}

#[command]
pub fn get_mother_hints() -> Result<Vec<String>, String> {
    ipc(Ok::<Vec<String>, _>(Vec::new()))
}
