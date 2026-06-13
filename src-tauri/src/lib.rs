mod commands;
mod connections;
mod error;
mod ftp;
mod localfs;
mod logging;
mod remote;
mod s3;
mod secret_store;
mod session;
mod sftp;
mod taxonomy;

use std::sync::Arc;
use tauri::Manager;

#[cfg(target_os = "macos")]
use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};

#[cfg(target_os = "windows")]
use window_vibrancy::apply_mica;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            // Install rustls' default CryptoProvider once at startup (FTPS, S3 TLS).
            let _ = rustls::crypto::ring::default_provider().install_default();

            // Forward protocol-library log records (russh, suppaftp, rustls)
            // into the Debug Log panel.
            logging::init_log_bridge(app.handle().clone());

            let store = connections::Store::load(&app.handle())?;
            app.manage(store);

            // Live session pool: SFTP/FTP connections stay open and are reused
            // across operations until the user disconnects or the server drops.
            app.manage(Arc::new(session::SessionPool::new()));

            // SFTP host keys persist next to connections.json (dir created by
            // Store::load above).
            sftp::init_known_hosts(&app.path().app_config_dir()?);

            let window = app.get_webview_window("main").expect("main window");

            #[cfg(target_os = "macos")]
            apply_vibrancy(
                &window,
                NSVisualEffectMaterial::HudWindow,
                Some(NSVisualEffectState::Active),
                Some(12.0),
            )
            .ok();

            #[cfg(target_os = "windows")]
            apply_mica(&window, None).ok();

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_connections,
            commands::upsert_connection,
            commands::delete_connection,
            commands::test_connection,
            commands::connect,
            commands::disconnect,
            commands::is_connected,
            commands::fs_home,
            commands::fs_list,
            commands::fs_parent,
            commands::remote_list,
            commands::remote_search,
            commands::remote_download,
            commands::remote_upload,
            commands::remote_delete,
            commands::window_close,
            commands::window_minimize,
            commands::window_toggle_maximize,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
