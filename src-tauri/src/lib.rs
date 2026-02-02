// CLI Proxy API - Tauri Desktop App

pub mod api;
pub mod auth;
pub mod commands;
pub mod config;
pub mod db;
pub mod proxy;

use tauri::{
    Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let app_handle = app.handle().clone();

            // Initialize config and start server on startup
            let config_handle = app_handle.clone();
            let server_handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                // First init config
                if let Err(e) = config::init_config(&config_handle).await {
                    tracing::error!("Failed to initialize config: {}", e);
                }

                // Initialize SQLite database
                if let Ok(data_dir) = config_handle.path().app_data_dir() {
                    if let Err(e) = db::init_db(data_dir) {
                        tracing::error!("Failed to initialize database: {}", e);
                    }
                }

                // Then start the API server
                tracing::info!("Starting API server...");
                if let Err(e) = crate::api::start_server(server_handle).await {
                    tracing::error!("Failed to start server: {}", e);
                }
            });

            // Setup system tray
            setup_tray(app)?;

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_config,
            commands::save_config,
            commands::get_auth_accounts,
            commands::get_auth_summary,
            commands::start_server,
            commands::stop_server,
            commands::get_server_status,
            commands::start_oauth_login,
            commands::delete_account,
            commands::set_account_enabled,
            commands::set_gemini_project_id,
            commands::fetch_antigravity_quota,
            commands::fetch_codex_quota,
            commands::fetch_gemini_quota,
            commands::fetch_kiro_quota,
            commands::export_all_accounts,
            commands::import_accounts,
            commands::export_accounts_to_file,
            commands::import_accounts_from_file,
            commands::get_cached_quotas,
            commands::get_settings,
            commands::save_settings,
            commands::get_request_logs,
            commands::get_request_logs_count,
            commands::clear_request_logs,
            commands::get_claude_code_config,
            commands::save_claude_code_config,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Create menu items
    let show_item = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
    let hide_item = MenuItem::with_id(app, "hide", "Hide Window", true, None::<&str>)?;
    let separator1 = MenuItem::with_id(app, "sep1", "─────────", false, None::<&str>)?;
    let start_item = MenuItem::with_id(app, "start", "Start Server", true, None::<&str>)?;
    let stop_item = MenuItem::with_id(app, "stop", "Stop Server", true, None::<&str>)?;
    let separator2 = MenuItem::with_id(app, "sep2", "─────────", false, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    // Create menu
    let menu = Menu::with_items(
        app,
        &[
            &show_item,
            &hide_item,
            &separator1,
            &start_item,
            &stop_item,
            &separator2,
            &quit_item,
        ],
    )?;

    // Build tray icon
    let _tray = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("CLI Proxy API")
        .on_menu_event(|app, event| {
            match event.id.as_ref() {
                "show" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "hide" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.hide();
                    }
                }
                "start" => {
                    let handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = crate::api::start_server(handle).await {
                            tracing::error!("Failed to start server: {}", e);
                        }
                    });
                }
                "stop" => {
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) = crate::api::stop_server().await {
                            tracing::error!("Failed to stop server: {}", e);
                        }
                    });
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            // Show window on left click
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;

    tracing::info!("System tray initialized");
    Ok(())
}
