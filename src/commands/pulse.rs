//! AI Pulse disk archive IPC handlers.
//!
//! The frontend (`src/pages/AiPulse/AiPulse.tsx`) calls two
//! commands against this module so the AI 资讯 / 明星项目 pages
//! can render their multi-day history even when the user is
//! offline. See `services::pulse_archive` for the storage
//! implementation and the file layout.

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
