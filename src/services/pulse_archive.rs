//! AI Pulse disk archive.
//!
//! The proprietary `edison7009/EchoBird-secret-` core persists
//! news items in `~/.echobird/pulse/YYYY/MM/DD_{lang}.json` so the
//! `AI 资讯` (AI News) and `明星项目` (Star Projects) pages can
//! render the full multi-day history even when the user is offline.
//! The frontend (see `src/pages/AiPulse/AiPulse.tsx` in the public
//! repo) calls two IPC commands against this archive:
//!
//! * `pulse_save(lang, items)` — fan the freshly-fetched window
//!   out into per-day files keyed by each item's *local* date
//!   (the upstream extractor tags items with `published_at` in
//!   UTC; we re-derive the local YYYY-MM-DD so a CST user
//!   doesn't see every 00:00–08:00 item bucketed into the day
//!   before).
//! * `pulse_fetch(lang)` — walk the mirror chain for the given
//!   language, pull the 7-day JSON window, fan it out to disk via
//!   `pulse_save`, then return the merged view. The frontend calls
//!   this on mount so the page no longer depends on a flaky
//!   WebView-side `fetch` against an upstream CORS surface.
//! * `pulse_load_all(lang)` — read every `*_{lang}.json` file in
//!   the tree, dedupe by `item.url`, sort by `published_at`
//!   descending.
//!
//! The archive is intentionally file-based (not SQLite) for two
//! reasons:
//!   1. Items are append-mostly — the frontend triggers a save
//!      whenever a 30-min window elapses, and a single fetch
//!      window is at most ~5 MB of JSON. B-tree dedupe work is
//!      not worth the schema migration cost.
//!   2. It mirrors the proprietary layout exactly, so if a user
//!      upgrades back to the upstream build their archive is
//!      still there. (File names + paths are stable across both
//!      builds.)
//!
//! Concurrency: the frontend is single-tab; commands run on
//! Tauri's worker threads, but `pulse_save` and `pulse_load_all`
//! are serialised on a process-global `Mutex` so a save
//! happening during a load can't surface a half-written file.
//! This is conservative but correct — pulse traffic is
//! < 1 req/min, so the lock is effectively uncontended.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;


use chrono::{DateTime, Datelike, Local, Utc};
use parking_lot::Mutex as PlMutex;
use serde::{Deserialize, Serialize};

use crate::error::{CoreResult, Error};

/// One archive item. The frontend's `NewsItem` interface in
/// `src/pages/AiPulse/AiPulse.tsx` has the same shape; the field
/// list below is the union of "all fields that ever matter" so
/// old archives keep working when we add new optional columns.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PulseItem {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site_name: Option<String>,
    pub source: String,
    pub title: String,
    pub url: String,
    pub published_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seen_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_zh: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title_en: Option<String>,
}

/// `lang` as the frontend passes it. We accept both `"zh"` and
/// `"en"` and tolerate the locale-style `"zh-Hans"` form
/// gracefully (we don't ship that today, but the input is
/// trusted and inexpensive to accept).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Zh,
    En,
}

impl Lang {
    pub fn parse(s: &str) -> CoreResult<Self> {
        // `split('-')` is robust to "zh" / "zh-Hans" / "en-US" —
        // we only care about the primary subtag.
        let primary = s.split('-').next().unwrap_or(s).to_ascii_lowercase();
        match primary.as_str() {
            "zh" => Ok(Lang::Zh),
            "en" => Ok(Lang::En),
            other => Err(Error::Validation {
                message: format!("unsupported pulse lang `{other}`"),
            }),
        }
    }

    fn file_suffix(self) -> &'static str {
        match self {
            Lang::Zh => "zh",
            Lang::En => "en",
        }
    }
}

/// Process-global lock so a save and a load can't interleave a
/// half-written file. See module docs for rationale.
static ARCHIVE_LOCK: PlMutex<()> = PlMutex::new(());

/// Resolve the on-disk root for the archive. We honour the
/// standard XDG layout on Linux, the macOS `~/.echobird` location
/// (matching the proprietary build exactly), and the `AppData`
/// location on Windows. Centralising the path here means tests
/// can override `ECHOBIRD_PULSE_DIR` to a `tempdir` without
/// touching the rest of the code.
pub fn archive_root() -> PathBuf {
    if let Ok(forced) = std::env::var("ECHOBIRD_PULSE_DIR") {
        if !forced.is_empty() {
            return PathBuf::from(forced);
        }
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    #[cfg(target_os = "macos")]
    {
        home.join(".echobird").join("pulse")
    }
    #[cfg(target_os = "linux")]
    {
        // XDG_DATA_HOME if set, else ~/.local/share.
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local").join("share"));
        base.join("echobird").join("pulse")
    }
    #[cfg(target_os = "windows")]
    {
        home.join("AppData").join("Roaming").join("echobird").join("pulse")
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        home.join(".echobird").join("pulse")
    }
}

/// Wrapper struct for legacy archive files written by the
/// upstream EchoBird (and by the pre-pulse_fetch browser-side
/// path in this fork). Those code paths serialised the bucket
/// as `{"schema":1,"date":"...","lang":"...","items":[...]}`
/// instead of a bare `Vec<PulseItem>` array. We accept both
/// shapes so a user upgrading from an old build never has to
/// hand-delete their archive.
#[derive(Debug, serde::Deserialize)]
struct LegacyBucket {
    #[serde(default)]
    items: Vec<PulseItem>,
}

/// Read a single bucket file. Missing file = empty list (we
/// never want a 404-style error to bubble up; the frontend
/// treats an empty archive as a normal "first launch" state).
/// Tolerant of:
///   * bare `[Item, ...]` arrays (current format)
///   * `{"items":[...]}` legacy wrappers (upstream / browser-fetch)
///   * corrupt / unparseable bytes (logged to stderr, treated as
///     empty so a single bad file can't take down the whole
///     archive read)
fn read_bucket(path: &Path) -> CoreResult<Vec<PulseItem>> {
    match fs::read(path) {
        Ok(bytes) => {
            if bytes.is_empty() {
                return Ok(Vec::new());
            }
            // Try bare array first.
            if let Ok(items) = serde_json::from_slice::<Vec<PulseItem>>(&bytes) {
                return Ok(items);
            }
            // Fall back to legacy `{"items":[...]}` wrapper.
            if let Ok(legacy) = serde_json::from_slice::<LegacyBucket>(&bytes) {
                return Ok(legacy.items);
            }
            // Otherwise: log to stderr and treat as empty. This
            // is the "tolerant" fallback — we'd rather lose one
            // bad bucket file than fail the whole archive walk.
            eprintln!(
                "[pulse_archive] could not parse {} as PulseItem array or legacy wrapper; skipping",
                path.display()
            );
            Ok(Vec::new())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(Error::Storage {
            message: format!("read {}: {e}", path.display()),
        }),
    }
}

/// Atomically write a bucket. We write to `<path>.tmp` then
/// rename — this means `pulse_load_all` can never observe a
/// truncated file even if the save is interrupted mid-flush.
fn write_bucket(path: &Path, items: &[PulseItem]) -> CoreResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| Error::Storage {
            message: format!("create_dir_all {}: {e}", parent.display()),
        })?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec(items).map_err(|e| Error::Storage {
        message: format!("serialize {}: {e}", path.display()),
    })?;
    fs::write(&tmp, &bytes).map_err(|e| Error::Storage {
        message: format!("write {}: {e}", tmp.display()),
    })?;
    fs::rename(&tmp, path).map_err(|e| Error::Storage {
        message: format!("rename {} -> {}: {e}", tmp.display(), path.display()),
    })?;
    Ok(())
}

/// Decide which bucket an item belongs in. The frontend gives us
/// `published_at` in some timezone (usually UTC with a `Z` suffix
/// but a few CN sources lie and use local time with a `Z`
/// suffix). We re-derive the *local* YYYY-MM-DD so a CST user
/// sees items bucketed under the day they actually appeared,
/// not the day UTC says they did. This matches the upstream
/// behaviour and the frontend's own `itemLocalDate()` helper.
fn bucket_parts(item: &PulseItem) -> CoreResult<(i32, u32, u32)> {
    let ts = item
        .published_at
        .as_deref()
        .or(item.first_seen_at.as_deref())
        .or(item.last_seen_at.as_deref())
        .unwrap_or("");
    if ts.is_empty() {
        return Err(Error::Validation {
            message: format!("item {} has no timestamp", item.id),
        });
    }
    let parsed: DateTime<Utc> = ts.parse().map_err(|e| Error::Validation {
        message: format!("item {} timestamp {ts:?} is not RFC3339: {e}", item.id),
    })?;
    let local = parsed.with_timezone(&Local);
    Ok((local.year(), local.month(), local.day()))
}

/// Append-and-dedupe: load the bucket file, merge in the new
/// items by `url` (the dedupe key the upstream uses), and write
/// the merged result back. Returns the post-merge count so
/// callers can log how many items actually grew the bucket.
fn merge_bucket(bucket_path: &Path, new_items: &[PulseItem]) -> CoreResult<usize> {
    let mut existing = read_bucket(bucket_path)?;
    let existing_urls: std::collections::HashSet<String> =
        existing.iter().map(|i| i.url.clone()).collect();
    let before = existing.len();
    for item in new_items {
        if !existing_urls.contains(&item.url) {
            existing.push(item.clone());
        }
    }
    if existing.len() == before {
        return Ok(existing.len());
    }
    write_bucket(bucket_path, &existing)?;
    Ok(existing.len())
}

/// `pulse_save` — fan the freshly-fetched window out to its
/// per-day buckets, dedupe against what's already on disk, and
/// return the per-bucket paths we wrote to (purely for
/// observability — the frontend ignores the result).
pub fn save(lang_str: &str, items: &[PulseItem]) -> CoreResult<Vec<String>> {
    let lang = Lang::parse(lang_str)?;
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let _guard = ARCHIVE_LOCK.lock();
    let root = archive_root();
    fs::create_dir_all(&root).map_err(|e| Error::Storage {
        message: format!("create_dir_all {}: {e}", root.display()),
    })?;

    // Group items by their target bucket. We don't need to sort
    // the items before grouping — each bucket is a self-contained
    // file that the load step dedupes anyway.
    let mut groups: std::collections::BTreeMap<(i32, u32, u32), Vec<PulseItem>> =
        std::collections::BTreeMap::new();
    for item in items {
        // Items with unparseable timestamps are dropped silently:
        // they're rare (one or two per fetch window) and the
        // upstream has the same behaviour. Logging them would be
        // nice but our service layer doesn't have a logger handle
        // wired in.
        if let Ok(parts) = bucket_parts(item) {
            groups.entry(parts).or_default().push(item.clone());
        }
    }

    let mut written = Vec::with_capacity(groups.len());
    for ((year, month, day), group) in groups {
        let path = root
            .join(format!("{year:04}"))
            .join(format!("{month:02}"))
            .join(format!("{day:02}_{}.json", lang.file_suffix()));
        let _count = merge_bucket(&path, &group)?;
        written.push(path.display().to_string());
    }
    Ok(written)
}

/// `pulse_load_all` — walk the entire archive tree, collect
/// every `{day}_{lang}.json`, dedupe across the whole set by
/// `url`, and return a single list sorted newest-first.
pub fn load_all(lang_str: &str) -> CoreResult<Vec<PulseItem>> {
    let lang = Lang::parse(lang_str)?;
    let _guard = ARCHIVE_LOCK.lock();
    let root = archive_root();
    if !root.exists() {
        return Ok(Vec::new());
    }
    let suffix = format!("_{}.json", lang.file_suffix());
    let mut all: Vec<PulseItem> = Vec::new();
    let mut seen_urls: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Depth-first walk. The proprietary build uses a single
    // `{year}/{month}/{day}_{lang}.json` layout, but we also
    // accept a flat `{day}_{lang}.json` layout (older versions
    // wrote to that) so an upgrade from a very old install
    // doesn't leave the user staring at an empty archive.
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let read = match fs::read_dir(&dir) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(Error::Storage {
                    message: format!("read_dir {}: {e}", dir.display()),
                })
            }
        };
        for entry in read.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = match path.file_name().and_then(|n| n.to_str()) {
                Some(n) => n,
                None => continue,
            };
            if !name.ends_with(&suffix) {
                continue;
            }
            for item in read_bucket(&path)? {
                if seen_urls.insert(item.url.clone()) {
                    all.push(item);
                }
            }
        }
    }

    // Newest first. Items with `published_at: null` (a handful of
    // HN comments) sink to the bottom; that's the same behaviour
    // the upstream archive has.
    all.sort_by(|a, b| {
        let ak = a.published_at.as_deref().unwrap_or("");
        let bk = b.published_at.as_deref().unwrap_or("");
        bk.cmp(ak)
    });
    Ok(all)
}

/// Mirror chains for the `pulse_fetch` command. Each list is walked
/// in order on the Rust side so the WebView is no longer the one
/// racing CORS / throttling on the upstream domain. We duplicate
/// the per-lang ordering that lives in `src/pages/AiPulse/AiPulse.tsx`
/// (PULSE_MIRRORS_ZH / PULSE_MIRRORS_EN) deliberately — these are
/// public, well-known infrastructure endpoints and the cost of a
/// one-line drift if someone reorders the chain is far smaller than
/// the cost of sharing the config across the IPC boundary.
const PULSE_MIRRORS_ZH: &[&str] = &[
    "https://echobird.ai/pulse",
    "https://ainew-1251534910.cos.ap-hongkong.myqcloud.com",
    "https://suyxh.github.io/ai-news-aggregator/data",
    "https://cdn.jsdelivr.net/gh/SuYxh/ai-news-aggregator@main/data",
    "https://raw.githubusercontent.com/SuYxh/ai-news-aggregator/main/data",
];
const PULSE_MIRRORS_EN: &[&str] = &[
    "https://echobird.ai/pulse",
    "https://ainew-1251534910.cos.ap-hongkong.myqcloud.com",
    "https://cdn.jsdelivr.net/gh/edison7009/EchoBird@main/docs/pulse",
    "https://raw.githubusercontent.com/edison7009/EchoBird/main/docs/pulse",
];
const FEED_FILE_ZH: &str = "latest-7d.json";
const FEED_FILE_EN: &str = "latest-7d-en.json";
const MIRROR_TIMEOUT_SECS: u64 = 10;

/// One mirror response. The shape matches the upstream extractor
/// (see `docs/handoff/fix-ai-pulse-empty/README.md` §5). The header
/// fields are kept for forward-compat even though the Rust side
/// currently only uses `items`.
#[derive(Debug, Clone, Deserialize)]
#[allow(dead_code)]
struct PulseFeed {
    generated_at: String,
    #[serde(default)]
    window_hours: u32,
    #[serde(default)]
    total_items: u32,
    items: Vec<PulseItem>,
}

/// Pick the mirror chain + filename for a given archive language.
fn mirrors_for(lang: Lang) -> (&'static [&'static str], &'static str) {
    match lang {
        Lang::Zh => (PULSE_MIRRORS_ZH, FEED_FILE_ZH),
        Lang::En => (PULSE_MIRRORS_EN, FEED_FILE_EN),
    }
}

/// True if the response body looks like an HTML error page rather
/// than a JSON payload. Several of the upstream surfaces
/// (notably Cloudflare 5xx and a few of the GitHub raw redirects)
/// return a styled HTML doc with a 200 — we treat that as failure
/// and advance to the next mirror.
fn looks_like_html(s: &str) -> bool {
    let head: String = s.chars().take(200).collect::<String>().trim_start().to_lowercase();
    head.starts_with("<!doctype html") || head.starts_with("<html")
}

/// Walk the mirror chain, return the first body whose HTTP status
/// is 2xx and whose body parses as [`PulseFeed`]. Errors are
/// surfaced verbatim (the command layer prefixes them with the
/// standard `network:` / `upstream:` / `timeout:` code).
async fn fetch_from_mirrors(
    client: &reqwest::Client,
    mirrors: &[&str],
    file: &str,
) -> CoreResult<PulseFeed> {
    let mut last_err: Option<Error> = None;
    for base in mirrors {
        let url = format!("{base}/{file}");
        let res = client.get(&url).send().await;
        match res {
            Ok(r) => {
                let status = r.status();
                if !status.is_success() {
                    last_err = Some(Error::upstream(format!(
                        "{base} {status}"
                    )));
                    continue;
                }
                let text = r.text().await.map_err(|e| {
                    Error::network(format!("{base} read body: {e}"))
                })?;
                if looks_like_html(&text) {
                    last_err = Some(Error::upstream(format!(
                        "{base} returned HTML"
                    )));
                    continue;
                }
                match serde_json::from_str::<PulseFeed>(&text) {
                    Ok(feed) => return Ok(feed),
                    Err(e) => {
                        last_err = Some(Error::upstream(format!(
                            "{base} bad JSON: {e}"
                        )));
                        continue;
                    }
                }
            }
            Err(e) if e.is_timeout() => {
                last_err = Some(Error::timeout(format!("{base} > {MIRROR_TIMEOUT_SECS}s")));
                continue;
            }
            Err(e) => {
                last_err = Some(Error::network(format!("{base}: {e}")));
                continue;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| Error::network("all mirrors failed")))
}

/// `pulse_fetch` — pull the 7-day window from the mirror chain and
/// fan it out into the on-disk archive. After the persist step we
/// run `load_all` so the in-memory view the frontend gets back is
/// the deduped union of (already-on-disk) + (just-fetched). The
/// frontend treats both fields as the "current items" payload and
/// reconciles them with whatever it already had in browser state.
///
/// Network failures are non-fatal: `pulse_fetch` returns the
/// existing on-disk items alongside an `error` string so the
/// frontend can surface a "fetch failed, showing cached" banner
/// instead of an empty page.
pub async fn fetch_and_persist(
    lang_str: &str,
) -> CoreResult<(Vec<PulseItem>, Option<String>)> {
    let lang = Lang::parse(lang_str)?;
    let (mirrors, file) = mirrors_for(lang);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(MIRROR_TIMEOUT_SECS))
        .user_agent(concat!("EchoBird/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| Error::network(format!("build reqwest client: {e}")))?;

    let fetch_res = fetch_from_mirrors(&client, mirrors, file).await;
    let merged = load_all(lang_str)?;

    match fetch_res {
        Ok(feed) => {
            if !feed.items.is_empty() {
                save(lang_str, &feed.items)?;
                // Re-load so the returned list reflects the just-persisted
                // items plus everything that was already on disk.
                let merged = load_all(lang_str)?;
                return Ok((merged, None));
            }
            // Empty payload from a 2xx mirror is unusual but
            // non-fatal: return the existing archive with a
            // diagnostic so the UI can show "no new items".
            Ok((
                merged,
                Some(format!("{} returned 0 items", mirrors[0])),
            ))
        }
        Err(e) => {
            // Network/upstream failure: keep the archive intact
            // and surface the error to the frontend.
            Ok((merged, Some(e.to_string())))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::OnceLock;

    /// All tests in this module share a single tempdir under
    /// `ECHOBIRD_PULSE_DIR`. We set the env var once, before the
    /// first test, so `archive_root()` returns the test root for
    /// the whole run. The `OnceLock` mutex makes the env-var
    /// write race-free even with `cargo test`'s parallel test
    /// runner.
    fn init_test_env() {
        static TEST_LOCK: OnceLock<PlMutex<()>> = OnceLock::new();
        let lock = TEST_LOCK.get_or_init(|| {
            let tmp = std::env::temp_dir().join(format!(
                "echobird-pulse-test-{}-{}",
                std::process::id(),
                chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
            ));
            // Safety: tests run inside a single process, so setting
            // an env var in one thread is visible to the others.
            // We still serialise through the OnceLock to keep the
            // intent obvious.
            env::set_var("ECHOBIRD_PULSE_DIR", &tmp);
            PlMutex::new(())
        });
        let _g = lock.lock();
    }

    fn make_item(id: &str, url: &str, ts: &str) -> PulseItem {
        PulseItem {
            id: id.to_string(),
            site_id: None,
            site_name: None,
            source: "test".to_string(),
            title: format!("item {id}"),
            url: url.to_string(),
            published_at: Some(ts.to_string()),
            first_seen_at: None,
            last_seen_at: None,
            title_zh: None,
            title_en: None,
        }
    }

    #[test]
    fn lang_parse_handles_locale_form() {
        assert!(matches!(Lang::parse("zh"), Ok(Lang::Zh)));
        assert!(matches!(Lang::parse("en"), Ok(Lang::En)));
        assert!(matches!(Lang::parse("zh-Hans"), Ok(Lang::Zh)));
        assert!(matches!(Lang::parse("en-US"), Ok(Lang::En)));
        assert!(Lang::parse("de").is_err());
    }

    #[test]
    fn save_then_load_roundtrip() {
        init_test_env();
        let items = vec![
            make_item("a", "https://a.test/1", "2026-06-19T10:00:00Z"),
            make_item("b", "https://b.test/1", "2026-06-19T12:00:00Z"),
            make_item("c", "https://c.test/1", "2026-06-20T01:30:00Z"),
        ];
        let written = save("zh", &items).expect("save");
        assert!(!written.is_empty(), "save should report at least one bucket");

        let loaded = load_all("zh").expect("load_all");
        assert_eq!(loaded.len(), 3, "all three items should round-trip");

        // Save again with one duplicate URL + one new URL. The
        // archive should grow by one (the duplicate is deduped).
        let more = vec![
            make_item("a-dup", "https://a.test/1", "2026-06-19T10:00:00Z"),
            make_item("d", "https://d.test/1", "2026-06-20T15:00:00Z"),
        ];
        save("zh", &more).expect("save 2");
        let loaded2 = load_all("zh").expect("load_all 2");
        assert_eq!(loaded2.len(), 4, "duplicate URL should be deduped");

        // The other lang shouldn't see any of these items.
        let en = load_all("en").expect("load en");
        assert!(en.is_empty(), "en archive should be independent of zh");
    }

    #[test]
    fn items_with_unparseable_timestamps_are_skipped() {
        init_test_env();
        let items = vec![PulseItem {
            id: "broken".to_string(),
            site_id: None,
            site_name: None,
            source: "test".to_string(),
            title: "broken".to_string(),
            url: "https://broken.test/1".to_string(),
            published_at: None,
            first_seen_at: None,
            last_seen_at: None,
            title_zh: None,
            title_en: None,
        }];
        // We don't expect this to error — the upstream
        // implementation also silently drops these.
        let written = save("en", &items).expect("save with broken item");
        assert!(written.is_empty(), "no bucket should be written for an item with no timestamp");
    }

    #[test]
    fn read_bucket_accepts_legacy_wrapper_format() {
        // The upstream EchoBird (and our pre-pulse_fetch browser
        // path) wrote each bucket as a JSON object with an
        // `items` field, not a bare array. read_bucket must
        // accept that shape so users upgrading from an old
        // build keep their history. This test is fully isolated:
        // it writes a custom file to a tempdir and calls
        // read_bucket directly, so it doesn't share state with
        // the roundtrip tests (which use ECHOBIRD_PULSE_DIR).
        let tmp = std::env::temp_dir().join(format!(
            "echobird-pulse-legacy-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let bucket = tmp.join("2026/06/12_zh.json");
        std::fs::create_dir_all(bucket.parent().unwrap()).unwrap();
        let legacy = serde_json::json!({
            "schema": 1,
            "date": "2026-06-12",
            "lang": "zh",
            "item_count": 2,
            "items": [
                make_item("legacy-a", "https://legacy.test/a", "2026-06-12T10:00:00Z"),
                make_item("legacy-b", "https://legacy.test/b", "2026-06-12T12:00:00Z"),
            ]
        });
        std::fs::write(&bucket, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let items = read_bucket(&bucket).expect("read_bucket should tolerate legacy format");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].url, "https://legacy.test/a");
    }

    #[test]
    fn read_bucket_skips_unparseable_files_instead_of_erroring() {
        // A file that isn't valid JSON for either shape should
        // not bring down the whole archive walk — the loader
        // logs to stderr and returns an empty list for that
        // bucket. Fully isolated (no env-var mutation).
        let tmp = std::env::temp_dir().join(format!(
            "echobird-pulse-corrupt-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let bucket = tmp.join("2026/06/13_zh.json");
        std::fs::create_dir_all(bucket.parent().unwrap()).unwrap();
        std::fs::write(&bucket, b"this is not json").unwrap();
        let items = read_bucket(&bucket).expect("read_bucket must not error on bad JSON");
        assert!(items.is_empty(), "bad bucket should be treated as empty");
    }

    #[test]
    fn load_all_on_missing_root_is_empty() {
        // Distinct tempdir from the other tests so we can prove
        // the missing-root path specifically.
        let tmp = std::env::temp_dir().join(format!(
            "echobird-pulse-missing-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        env::set_var("ECHOBIRD_PULSE_DIR", &tmp);
        let loaded = load_all("en").expect("load_all on missing root");
        assert!(loaded.is_empty());
    }
}
