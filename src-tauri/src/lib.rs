//! Tauri-слой десктоп-клиента: системные вызовы Windows, жизненный цикл xray,
//! команды для фронтенда. Вся платформонезависимая логика — в крейте
//! `zexor-vpn-core`.

pub mod adblock;
pub mod auth;
pub mod state;
pub mod xray;

use tauri::Manager as _;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "zexor_vpn_lib=info,warn".into()),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state::AppState::default())
        .invoke_handler(tauri::generate_handler![
            auth::commands::login,
            auth::commands::logout,
            auth::commands::current_session,
            xray::commands::connect,
            xray::commands::disconnect,
            xray::commands::connection_status,
            xray::commands::list_nodes,
            adblock::commands::adblock_settings,
            adblock::commands::set_adblock_enabled,
        ])
        .setup(|app| {
            // Жёсткая гарантия: при любом выходе снимаем системный прокси и
            // гасим xray, иначе пользователь останется без интернета.
            let handle = app.handle().clone();
            app.manage(state::ShutdownGuard::new(handle));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let state = window.state::<state::AppState>();
                state.shutdown_blocking();
            }
        })
        .run(tauri::generate_context!())
        .expect("не удалось запустить приложение");
}
