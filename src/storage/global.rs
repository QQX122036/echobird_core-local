//! Process-global store handle, populated by `lib::run` and read by
//! every command handler. The trait object keeps the storage
//! implementation swappable for tests.
//!
//! We use `OnceLock<Arc<dyn Store>>` so the read path is wait-free
//! after the first call. The first read in production happens
//! after `run` finishes `setup`, so the lock is always populated
//! by then.

use std::sync::{Arc, OnceLock};

use super::Store;

static STORE: OnceLock<Arc<dyn Store>> = OnceLock::new();

/// Set the process-global store. Called exactly once, from
/// `lib::run`'s `setup` hook. A second call is a programming
/// error and is logged-and-ignored (we never want to overwrite
/// a working store mid-process).
pub fn install(store: Arc<dyn Store>) {
    if STORE.set(store).is_err() {
        log::warn!("storage::global::install called twice; keeping first store");
    }
}

/// Read-only access to the global store. Panics if called before
/// `install` — which is a programming error because `run` always
/// installs during `setup`, before any command can fire.
pub fn get() -> &'static Arc<dyn Store> {
    STORE
        .get()
        .expect("storage::global::get called before storage::global::install")
}
