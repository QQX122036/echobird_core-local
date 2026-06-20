//! Smoke test for `pulse_fetch` — call the real function, dump
//! the return value, prove the backend can deliver content.
//! Skipped if ECHOBIRD_SKIP_PULSE_E2E=1.

use echobird_core::services::pulse_archive;
use std::env;

fn ensure_isolated_archive() {
    if env::var("ECHOBIRD_PULSE_DIR").is_err() {
        let tmp = std::env::temp_dir().join(format!(
            "echobird-pulse-smoke-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        env::set_var("ECHOBIRD_PULSE_DIR", &tmp);
    }
}

#[tokio::test]
async fn smoke_zh() {
    if env::var("ECHOBIRD_SKIP_PULSE_E2E").is_ok() { return; }
    ensure_isolated_archive();
    println!("=== smoke: fetch_and_persist('zh') ===");
    let (items, diagnostic) = pulse_archive::fetch_and_persist("zh").await.expect("zh fetch");
    println!("items: {} | diagnostic: {:?}", items.len(), diagnostic);
    if let Some(it) = items.first() {
        println!("first item: id={} title={:?}", it.id, it.title.chars().take(30).collect::<String>());
        println!("             source={} url={}", it.source, it.url);
        println!("             published_at={:?} first_seen_at={:?}", it.published_at, it.first_seen_at);
    }
    // Group by local date (CST) to see the bucketing shape.
    use std::collections::BTreeMap;
    let mut by_date: BTreeMap<String, usize> = BTreeMap::new();
    for it in &items {
        let ts = it.published_at.clone().unwrap_or_default();
        if ts.is_empty() { continue; }
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&ts) {
            let local: chrono::DateTime<chrono::Local> = dt.with_timezone(&chrono::Local);
            *by_date.entry(local.format("%Y-%m-%d").to_string()).or_insert(0) += 1;
        }
    }
    println!("items per local date (top 10 most recent):");
    for (d, c) in by_date.iter().rev().take(10) {
        println!("  {}: {}", d, c);
    }
    assert!(items.len() > 1000, "zh smoke: expected > 1000 items, got {}", items.len());
}

#[tokio::test]
async fn smoke_en() {
    if env::var("ECHOBIRD_SKIP_PULSE_E2E").is_ok() { return; }
    ensure_isolated_archive();
    println!("=== smoke: fetch_and_persist('en') ===");
    let (items, diagnostic) = pulse_archive::fetch_and_persist("en").await.expect("en fetch");
    println!("items: {} | diagnostic: {:?}", items.len(), diagnostic);
    if let Some(it) = items.first() {
        println!("first item: id={} title={:?}", it.id, &it.title[..it.title.len().min(60)]);
    }
    use std::collections::BTreeMap;
    let mut by_date: BTreeMap<String, usize> = BTreeMap::new();
    for it in &items {
        let ts = it.published_at.clone().unwrap_or_default();
        if ts.is_empty() { continue; }
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(&ts) {
            let local: chrono::DateTime<chrono::Local> = dt.with_timezone(&chrono::Local);
            *by_date.entry(local.format("%Y-%m-%d").to_string()).or_insert(0) += 1;
        }
    }
    println!("items per local date (top 10 most recent):");
    for (d, c) in by_date.iter().rev().take(10) {
        println!("  {}: {}", d, c);
    }
    assert!(items.len() > 100, "en smoke: expected > 100 items, got {}", items.len());
}
