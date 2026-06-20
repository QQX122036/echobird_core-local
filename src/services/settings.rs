//! App-level settings (locale, theme, close-to-tray). Tiny on
//! purpose — the IPC contract is small and the storage side is
//! one row in the `settings` table.

use std::sync::Arc;

use crate::error::CoreResult;
use crate::storage::{AppSettings, SettingsPatch, Store};

pub fn get_settings(store: &Arc<dyn Store>) -> CoreResult<AppSettings> {
    store.get_settings()
}

pub fn save_settings(store: &Arc<dyn Store>, patch: SettingsPatch) -> CoreResult<AppSettings> {
    store.save_settings(patch)
}
