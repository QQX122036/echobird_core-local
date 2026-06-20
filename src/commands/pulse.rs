//! AI Pulse disk archive IPC handlers.
//!
//! The frontend (`src/pages/AiPulse/AiPulse.tsx`) calls three
//! commands against this module so the AI 资讯 / 明星项目 pages
//! can render their multi-day history even when the user is
//! offline. See `services::pulse_archive` for the storage
//! implementation and the file layout.
//!
//! * `pulse_save` — fan the freshly-fetched window out to disk
//!   (called by the legacy browser-side `fetch` path and the
//!   legacy-migration helper).
//! * `pulse_load_all` — read every archived item for the given
//!   language, deduped by URL and sorted newest-first.
//! * `pulse_fetch` — walk the mirror chain on the Rust side
//!   (10s per mirror, sticky-success doesn't apply on the Rust
//!   side because the next-mirror retry logic is server-driven),
//!   persist the items via `pulse_save`, and return the merged
//!   view. Network failures are non-fatal: the returned payload
//!   still contains whatever was already on disk, with a
//!   `diagnostic` string the frontend can surface as a "fetch
//!   failed, showing cached" banner.

use tauri::command;

use crate::commands::ipc;
use crate::services::pulse_archive::{self, PulseItem};

/// `pulse_save` — persist the freshly-fetched pulse window into
/// the on-disk archive. The frontend passes the language key
/// (`"zh"` / `"en"`) and the items it just received from the
/// upstream mirror; we fan them out into per-day bucket files
/// and dedupe against anything already on disk. Errors are
/// surfaced as `storage:`-prefixed strings so the frontend can
/// log them and continue.
#[command]
pub fn pulse_save(lang: String, items: Vec<PulseItem>) -> Result<serde_json::Value, String> {
    ipc(pulse_archive::save(&lang, &items).map(|written| {
        serde_json::json!({ "written": written })
    }))
}

/// `pulse_load_all` — load every archived item for the given
/// language, deduped by URL and sorted newest-first. Returns
/// `[]` (not an error) on a first launch where no archive
/// exists yet — the frontend treats both as "no cached history
/// yet, will fetch from the mirror".
#[command]
pub fn pulse_load_all(lang: String) -> Result<Vec<PulseItem>, String> {
    ipc(pulse_archive::load_all(&lang))
}

/// `pulse_fetch` — the on-Rust equivalent of the browser-side
/// `fetchOneFeed` walk. We do the network round-trip from a
/// reqwest client (no CORS surface, no WebView quirks), persist
/// the result via the same `pulse_save` codepath the frontend
/// uses, and return the merged archive so the caller doesn't
/// have to follow up with a separate `pulse_load_all`.
///
/// Response shape on success:
///   `{ items: PulseItem[], diagnostic: null }`
/// Response shape on network failure:
///   `{ items: <archived>, diagnostic: "<error code>: <message>" }`
/// The frontend reconciles the returned `items` with its own
/// in-memory list (URL-deduped) and surfaces `diagnostic` as a
/// non-blocking banner.
#[command]
pub async fn pulse_fetch(lang: String) -> Result<serde_json::Value, String> {
    let (items, diagnostic) =
        ipc(pulse_archive::fetch_and_persist(&lang).await)?;
    Ok(serde_json::json!({
        "items": items,
        "diagnostic": diagnostic,
    }))
}
