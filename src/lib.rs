//! Clean-room reimplementation of the private `echobird_core`
//! crate. See the top of `src/error/mod.rs` for the derivation
//! contract; everything in this crate is derived from the public
//! thin shell's IPC surface and the TypeScript types in
//! `src/api/types.ts` of the public EchoBird repo.
//!
//! The only items the public thin shell needs from this crate
//! are [`services::bundled_assets::BundledAssets`] +
//! [`services::bundled_assets::register`] and the
//! [`run`] entry point. Everything else is reachable from the
//! IPC command layer.

pub mod error;
pub mod services;
pub mod storage;
pub mod commands;

use std::sync::Arc;

use tauri::{Context, Manager};

use storage::Store;

/// Public entry point. Mirrors the upstream signature. Called
/// from the thin shell's `run` after `tauri::generate_context!()`
/// and the tray-icon `include_bytes!` have expanded.
pub fn run(context: Context<tauri::Wry>, _tray_icon_bytes: &'static [u8]) {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .invoke_handler(tauri::generate_handler![
            // ─── Models ──────────────────────────────────────
            commands::models::get_models,
            commands::models::add_model,
            commands::models::update_model,
            commands::models::delete_model,
            commands::models::test_model,
            commands::models::ping_model,
            commands::models::is_key_destroyed,
            commands::models::get_model_directory,
            // ─── Agent ───────────────────────────────────────
            commands::agent::agent_send_message,
            commands::agent::agent_reset,
            commands::agent::agent_abort,
            // ─── Tools ───────────────────────────────────────
            commands::tools::scan_tools,
            commands::tools::start_tool,
            commands::tools::apply_model_to_tool,
            commands::tools::restore_tool_to_official,
            // ─── Settings + lifecycle ────────────────────────
            commands::app::get_settings,
            commands::app::save_settings,
            commands::app::app_ready,
            commands::app::open_folder,
            commands::app::read_log_tail,
            commands::app::get_system_info,
            commands::app::download_and_install_update,
            commands::app::get_avatar,
            commands::app::set_avatar,
            commands::app::seed_builtin_to_user_dir,
            commands::app::apply_user_project_model,
            commands::app::launch_user_project,
            commands::app::launch_game,
            // ─── Local LLM (llama-server) ───────────────────
            commands::local_server::start_llm_server,
            commands::local_server::stop_llm_server,
            commands::local_server::get_llm_server_info,
            commands::local_server::get_llm_server_logs,
            commands::local_server::get_llm_default_command,
            commands::local_server::get_llm_custom_command,
            commands::local_server::set_llm_custom_command,
            commands::local_server::clear_llm_custom_command,
            commands::local_server::add_models_dir,
            commands::local_server::remove_models_dir,
            commands::local_server::get_models_dirs,
            commands::local_server::get_download_dir,
            commands::local_server::set_download_dir,
            commands::local_server::scan_gguf_files,
            commands::local_server::scan_hf_models,
            commands::local_server::download_model,
            commands::local_server::pause_download,
            commands::local_server::cancel_download,
            commands::local_server::detect_gpu,
            commands::local_server::get_gpu_info,
            commands::local_server::install_local_engine,
            commands::local_server::get_local_engine_status,
            commands::local_server::list_engine_release_options,
            // ─── Store / bundled assets ─────────────────────
            commands::bundled::get_store_models,
            commands::bundled::get_install_index,
            // ─── SSH ────────────────────────────────────────
            commands::ssh::load_ssh_servers,
            commands::ssh::save_ssh_server,
            commands::ssh::remove_ssh_server,
            commands::ssh::ssh_test_connection,
            // ─── Secret (encrypted API keys) ────────────────
            commands::secret::encrypt_secret,
            commands::secret::decrypt_secret,
            // ─── Parasite (Mother Agent Connect mode) ───────
            commands::parasite::parasite_send_message,
            commands::parasite::parasite_reset,
            commands::parasite::parasite_abort,
            commands::parasite::parasite_list_installed,
            // ─── AI Career / Mother hints ───────────────────
            commands::ai_career::ai_career_heatmap,
            commands::ai_career::ai_career_family_history,
            commands::ai_career::ai_career_token_bytes,
            commands::ai_career::get_mother_hints,
        ])
        .setup(|app| {
            // Open the SQLite store. We resolve the app's data
            // dir from Tauri's `path()` resolver so the same
            // path is used across Windows / macOS / Linux.
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("app data dir resolvable on supported platforms");
            std::fs::create_dir_all(&data_dir).expect("create app data dir");
            let db_path = data_dir.join("echobird.sqlite");
            let store: Arc<dyn Store> =
                storage::sqlite::SqliteStore::open(&db_path).expect("open sqlite store");
            storage::global::install(store);

            // 1-second safety timer: if the frontend never
            // calls appReady() (boot crash, blank screen, etc.)
            // we show the main window anyway so the user is
            // never staring at a missing dock icon. Mirrors
            // the proprietary build's [Safety] behavior.
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(1));
                if let Some(w) = app_handle.get_webview_window("main") {
                    if !w.is_visible().unwrap_or(false) {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
            });

            Ok(())
        })
        .build(context)
        .expect("tauri app build");

    app.run(|_app_handle, _event| {
        // All work happens in command handlers. The event loop
        // here is a no-op.
    });
}
