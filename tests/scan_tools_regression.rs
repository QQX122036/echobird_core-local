//! Regression test: verify that `scan_tools` IPC returns the full set
//! of bundled tools with correct `name`, `displayName`, and `category`.
//!
//! This test catches two related bugs that previously broke the App
//! Manager (the frontend's `toolsStore.scanTools` swallowed the error,
//! leaving the user staring at an empty panel):
//!
//!   1. All 23 upstream install JSONs ship without a `name` field —
//!      only `displayName`. `InstallEntry.name` was `String` (required),
//!      so every JSON failed to deserialize and the IPC returned
//!      zero tools. Fix: `name` is now `Option<String>` with
//!      `#[serde(default)]`, and `detect_one` falls back to
//!      `display_name`.
//!   2. None of the 23 JSONs ship a `category` field. The App Manager
//!      sidebar tabs filter by `tool.category === activeToolCategory`,
//!      so every tool was in the empty-string category, hidden under
//!      every non-ALL tab. Fix: a Rust `category_for` map derives
//!      category from the tool id (single source of truth — adding a
//!      new tool only requires one row in that map, not edits to the
//!      JSONs).
//!
//! If a future refactor removes either fallback, this test will fail
//! with a clear assertion.

use std::sync::OnceLock;
use echobird_core::services::bundled_assets::{self, BundledAssets};
use echobird_core::services::tool_installer;

const INSTALL_DIR: &str = "/Users/ayden/Documents/EchoBird/docs/api/tools/install";

/// The category buckets the App Manager sidebar tabs accept. The
/// frontend declares these in `src/pages/AppManager/context.ts`.
const VALID_CATEGORIES: &[&str] = &[
    "Desktop", "IDE", "CLI Code", "AutoTrading", "Game", "Utility",
];

fn load_bundled() -> &'static BundledAssets {
    static LOADED: OnceLock<BundledAssets> = OnceLock::new();
    LOADED.get_or_init(|| {
        let index_path = format!("{}/index.json", INSTALL_DIR);
        let index_json = std::fs::read_to_string(&index_path)
            .unwrap_or_else(|e| panic!("read {}: {}", index_path, e));
        let index: serde_json::Value = serde_json::from_str(&index_json)
            .expect("index.json valid JSON");
        let ids: Vec<String> = index["ids"].as_array().unwrap()
            .iter().map(|v| v.as_str().unwrap().to_string()).collect();
        let mut refs: Vec<(&'static str, &'static str)> = Vec::new();
        for id in &ids {
            let path = format!("{}/{}.json", INSTALL_DIR, id);
            let content = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {}", path, e));
            let leaked_id: &'static str = Box::leak(id.clone().into_boxed_str());
            let leaked_content: &'static str = Box::leak(content.into_boxed_str());
            refs.push((leaked_id, leaked_content));
        }
        let leaked_refs: &'static [(&'static str, &'static str)] =
            Box::leak(refs.into_boxed_slice());
        BundledAssets {
            install_index_json: Box::leak(index_json.into_boxed_str()),
            install_refs: leaked_refs,
        }
    })
}

#[test]
fn scan_tools_returns_all_bundled_entries() {
    let bundled = load_bundled();
    bundled_assets::register(bundled);

    // The path the App Manager IPC actually takes.
    let detected = tool_installer::scan_tools()
        .expect("scan_tools must succeed when BUNDLED is registered");

    // index.json ships 23 tool ids. We must return all of them.
    let expected_count = 23;
    assert!(
        detected.len() >= expected_count,
        "expected at least {} tools from bundled_assets, got {} \
         (regression: a JSON without `name`/displayName is being \
         rejected at deserialization)",
        expected_count,
        detected.len(),
    );

    // Every tool must have:
    //   * a non-empty name (the UI renders this)
    //   * a category that the App Manager tabs recognize
    //     (so it shows up under at least one non-ALL tab)
    //   * a non-empty displayName (so the right panel has a label)
    for t in &detected {
        assert!(
            !t.name.trim().is_empty(),
            "tool {} has empty name — `detect_one` fallback to display_name broken",
            t.id,
        );
        assert!(
            VALID_CATEGORIES.contains(&t.category.as_str()),
            "tool {} has category {:?} which is not in the App Manager tab list {:?} \
             — `category_for` map missing an entry for this id",
            t.id, t.category, VALID_CATEGORIES,
        );
        assert!(
            t.display_name.as_deref().map_or(false, |s| !s.is_empty()),
            "tool {} has empty displayName — UI right panel will show blank",
            t.id,
        );
    }
}
