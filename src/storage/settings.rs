//! User-level app settings. Tiny for now; we keep the type around
//! so the IPC layer can be strict about what's persisted.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    /// `'light' | 'dark'`. `None` means "follow the OS".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_mode: Option<ThemeMode>,
    /// `None` = always ask, `Some(true)` = minimize to tray,
    /// `Some(false)` = quit directly.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_to_tray: Option<bool>,
    /// Tracks whether the user has explicitly picked a close
    /// behavior. The frontend reads this to decide whether to
    /// show the "what should we do on close?" dialog on launch.
    #[serde(default)]
    pub close_window_behavior_set: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
}

#[derive(Debug, Clone, Default)]
pub struct SettingsPatch {
    pub locale: Option<String>,
    pub theme_mode: Option<Option<ThemeMode>>,
    pub close_to_tray: Option<Option<bool>>,
    pub close_window_behavior_set: Option<bool>,
}
