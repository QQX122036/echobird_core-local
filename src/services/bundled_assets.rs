//! Compile-time install-manifest table passed in from the public thin
//! shell. The shell holds one `static BUNDLED: BundledAssets` literal
//! (built from `include_str!(".../*.json")`) and calls
//! [`register`] during boot.
//!
//! Why the indirection: the `include_str!` calls must expand in the
//! binary crate so `CARGO_MANIFEST_DIR` resolves to `src-tauri/`. This
//! crate is a library and can't do those expansions. So the shell
//! owns the raw bytes and we own the parsing + access.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::error::{CoreResult, Error};

/// Raw payload passed from the thin shell. Every field is `&'static`
/// because the shell's `static BUNDLED` is constructed at compile
/// time. We never own these strings.
#[derive(Debug, Clone)]
pub struct BundledAssets {
    pub install_index_json: &'static str,
    pub install_refs: &'static [(&'static str, &'static str)],
}

/// One tool's install manifest — the JSON shipped in
/// `docs/api/tools/install/<id>.json`. The public frontend reads
/// these via the `get_install_index` and `scan_tools` IPC commands,
/// so the field set is dictated by what the UI consumes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallEntry {
    pub id: String,
    /// Tool display name. Many upstream install JSONs only ship
    /// `displayName` (without a bare `name`), so this field is
    /// `Option<String>` with `#[serde(default)]` — `detect_one`
    /// falls back to `display_name` when it's missing/empty.
    #[serde(default)]
    pub name: Option<String>,
    /// Optional human-readable alias. When the JSON ships only
    /// `displayName` (the common case in the public manifest),
    /// `detect_one` uses this as the canonical `name`.
    #[serde(default, rename = "displayName")]
    pub display_name: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub api_protocol: Option<Vec<String>>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub start_command: Option<String>,
    #[serde(default)]
    pub launch_file: Option<String>,
    #[serde(default)]
    pub icon_base64: Option<String>,
    #[serde(default)]
    pub names: Option<serde_json::Value>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub detected_path: Option<String>,
    #[serde(default)]
    pub config_path: Option<String>,
}

/// `index.json` — just the list of ids.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallIndex {
    pub ids: Vec<String>,
}

struct ParsedAssets {
    index: InstallIndex,
    entries: Vec<(String, InstallEntry)>,
}

static REGISTERED: OnceLock<BundledAssets> = OnceLock::new();
static PARSED: OnceLock<ParsedAssets> = OnceLock::new();

/// Called once from `run`. Calling twice is a programming error
/// because the shell only registers at boot; the panic surfaces a
/// double-registration during development.
pub fn register(bundled: &'static BundledAssets) {
    if REGISTERED.set(bundled.clone()).is_err() {
        // Don't panic in release — log and keep the first registration.
        log::warn!("bundled_assets::register called twice; keeping first registration");
    }
}

/// Returns the parsed `index.json` — every tool id shipped in the
/// public install manifest. Used by the `get_install_index` IPC.
pub fn install_index() -> CoreResult<&'static InstallIndex> {
    let parsed = ensure_parsed()?;
    Ok(&parsed.index)
}

/// Returns the parsed install manifest for a specific tool id, or
/// `NotFound` if the id isn't in the bundle.
pub fn install_entry(id: &str) -> CoreResult<&'static InstallEntry> {
    let parsed = ensure_parsed()?;
    parsed
        .entries
        .iter()
        .find(|(eid, _)| eid == id)
        .map(|(_, e)| e)
        .ok_or_else(|| Error::not_found(format!("install entry for {id}")))
}

/// Returns all parsed install entries, in the order they appear in
/// the index. Used by the `scan_tools` IPC to render the right
/// panel of the App Manager.
pub fn all_install_entries() -> CoreResult<&'static [InstallEntry]> {
    let parsed = ensure_parsed()?;
    // Leak the entries into a 'static slice so the caller doesn't
    // hold a borrow on the OnceLock. The set of entries is fixed
    // for the lifetime of the process, so leaking is fine.
    let owned: &'static [InstallEntry] = Box::leak(
        parsed
            .entries
            .iter()
            .map(|(_, e)| e.clone())
            .collect::<Vec<_>>()
            .into_boxed_slice(),
    );
    Ok(owned)
}

fn ensure_parsed() -> CoreResult<&'static ParsedAssets> {
    if let Some(p) = PARSED.get() {
        return Ok(p);
    }
    // We hold no lock here; if two callers race, both will
    // compute the same value and the slower one wins. The
    // owned data is cheap (a few hundred KB at most) so
    // accepting the duplicate work is simpler than mutexing
    // the init.
    let raw = REGISTERED
        .get()
        .ok_or_else(|| Error::internal("bundled_assets not registered before use"))?;
    let index: InstallIndex = serde_json::from_str(raw.install_index_json)
        .map_err(|e| Error::internal(format!("install_index_json invalid: {e}")))?;
    let entries = raw
        .install_refs
        .iter()
        .map(|(id, json)| {
            let entry: InstallEntry = serde_json::from_str(json).map_err(|e| {
                Error::internal(format!("install entry {id} invalid: {e}"))
            })?;
            Ok::<_, Error>(((*id).to_string(), entry))
        })
        .collect::<CoreResult<Vec<_>>>()?;
    let parsed = ParsedAssets { index, entries };
    // Best-effort cache. If a racer beat us to it, we keep
    // the first one.
    let _ = PARSED.set(parsed);
    Ok(PARSED.get().expect("PARSED just set"))
}
