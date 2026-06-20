//! Application services — the layer between the IPC commands and the
//! storage / network layer. Each service is a focused module that
//! owns one slice of business logic and exposes a typed surface to
//! the command handlers.
//!
//! Service design rules:
//!   * No service ever touches the Tauri runtime directly. Commands
//!     extract any state they need (app data dir, current time, etc.)
//!     and pass it in. This keeps services testable from plain
//!     `#[test]` functions without a Tauri harness.
//!   * Services never return `serde_json::Value` — every public
//!     function has a concrete return type. The IPC layer does the
//!     `serde` work.
//!   * Errors are `Error` (not `anyhow`) so callers can pattern-match
//!     when they need to (e.g. turning a 401 into a different
//!     frontend message).

pub mod agent;
pub mod models;
pub mod bundled_assets;
pub mod context_window;
pub mod model_proxy;
pub mod settings;
pub mod tool_installer;
