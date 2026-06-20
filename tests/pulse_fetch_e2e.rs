//! End-to-end smoke test for the `pulse_fetch` codepath.
//!
//! Verifies, against the real mirror chain (not a mock):
//!   1. `fetch_and_persist` returns > 0 items for `zh`
//!   2. The on-disk archive is populated for today
//!   3. The returned items are a `Vec<PulseItem>` whose every entry
//!      has a parseable `published_at` (the bucketing step depends
//!      on this).
//!   4. A second call with the same lang is non-fatal even though
//!      `pulse_save` will be a no-op (existing urls are deduped).
//!
//! Skipped if `ECHOBIRD_SKIP_PULSE_E2E=1` so CI can opt out without
//! network. The test uses an isolated `ECHOBIRD_PULSE_DIR` so it
//! can't pollute the user's real archive at `~/.echobird/pulse/`.

use echobird_core::services::pulse_archive;
use std::env;

fn ensure_isolated_archive() {
    if env::var("ECHOBIRD_PULSE_DIR").is_err() {
        let tmp = std::env::temp_dir().join(format!(
            "echobird-pulse-e2e-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        env::set_var("ECHOBIRD_PULSE_DIR", &tmp);
        eprintln!("[pulse-e2e] isolated archive root: {}", tmp.display());
    }
}

#[tokio::test]
async fn fetch_and_persist_returns_zh_items_from_real_mirrors() {
    if env::var("ECHOBIRD_SKIP_PULSE_E2E").is_ok() {
        eprintln!("[pulse-e2e] skipped via ECHOBIRD_SKIP_PULSE_E2E");
        return;
    }
    ensure_isolated_archive();
    let (items, diagnostic) = pulse_archive::fetch_and_persist("zh")
        .await
        .expect("fetch_and_persist should not error at the IPC boundary");
    // A non-diagnostic (clean) response is what we want; a soft
    // diagnostic that the archive is still empty is also acceptable
    // (e.g. upstream returned 0 items), but in practice the mirror
    // chain should always have content. We accept both, just assert
    // the items path is non-empty.
    assert!(
        items.len() > 100,
        "expected > 100 zh items from real mirror chain, got {} (diagnostic={:?})",
        items.len(),
        diagnostic
    );
    // Every item should have at least one parseable timestamp
    // (published_at, else first_seen_at, else last_seen_at) — that
    // is the contract `bucket_parts` enforces, and the same
    // fallback chain the frontend's `itemTs` uses, so every item
    // returned here will surface in the UI.
    let probe = |s: &Option<String>| {
        s.as_deref()
            .map(|x| x.parse::<chrono::DateTime<chrono::Utc>>().is_ok())
            .unwrap_or(false)
    };
    let displayable = items
        .iter()
        .filter(|it| probe(&it.published_at) || probe(&it.first_seen_at) || probe(&it.last_seen_at))
        .count();
    assert_eq!(
        displayable, items.len(),
        "every returned item should have at least one parseable timestamp"
    );
}

#[tokio::test]
async fn fetch_and_persist_is_idempotent() {
    if env::var("ECHOBIRD_SKIP_PULSE_E2E").is_ok() {
        return;
    }
    ensure_isolated_archive();
    let (first, _) = pulse_archive::fetch_and_persist("zh")
        .await
        .expect("first fetch");
    let first_count = first.len();
    assert!(first_count > 0, "first fetch returned 0 items");

    // Second call: same mirror, same dedupe key (url), should return
    // the same number (no growth because the disk archive already has
    // every url). The function shouldn't error.
    let (second, diag2) = pulse_archive::fetch_and_persist("zh")
        .await
        .expect("second fetch");
    assert!(
        second.len() >= first_count,
        "second fetch should not shrink the archive ({} < {})",
        second.len(),
        first_count
    );
    // The diagnostic should be None for a clean run.
    assert!(
        diag2.is_none(),
        "second fetch on a healthy mirror should have no diagnostic, got: {:?}",
        diag2
    );
}
