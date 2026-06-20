//! Verify the `category_for` map in `tool_installer` assigns every
//! bundled tool to a valid App Manager tab category, and that the
//! distribution is non-empty (no tab should be entirely empty after
//! the fix).

use std::collections::HashMap;
use std::sync::OnceLock;
use echobird_core::services::bundled_assets::{self, BundledAssets};
use echobird_core::services::tool_installer;

const INSTALL_DIR: &str = "/Users/ayden/Documents/EchoBird/docs/api/tools/install";

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
fn every_category_tab_has_at_least_one_tool() {
    let bundled = load_bundled();
    bundled_assets::register(bundled);
    let detected = tool_installer::scan_tools().expect("scan_tools ok");

    // Group tools by category so we can prove no tab is empty.
    let mut by_category: HashMap<String, Vec<String>> = HashMap::new();
    for t in &detected {
        by_category
            .entry(t.category.clone())
            .or_default()
            .push(t.id.clone());
    }

    eprintln!("\n=== Category distribution ===");
    for (cat, ids) in &by_category {
        eprintln!("  {:<12} ({}): {}", cat, ids.len(), ids.join(", "));
    }

    // Every non-Game tab should have at least one tool.
    // (Game legitimately has no tools today; that's a
    //  "no game integrations shipped yet" state, not a bug.)
    let must_have_tools = ["Desktop", "IDE", "CLI Code", "AutoTrading", "Utility"];
    for cat in must_have_tools {
        let count = by_category.get(cat).map(|v| v.len()).unwrap_or(0);
        assert!(
            count > 0,
            "tab '{}' is empty after category_for fix — the Rust-side category map is missing an entry for the bundled tool(s).",
            cat,
        );
    }
}
