//! Tauri-слой десктоп-клиента: системные вызовы Windows, жизненный цикл xray,
//! команды для фронтенда. Вся платформонезависимая логика — в крейте
//! `zexor-vpn-core`.

pub mod adblock;
pub mod apk_update;
pub mod auth;
pub mod cabinet;
pub mod error_report;
pub mod settings;
pub mod state;
pub mod sync;
pub mod xray;

#[cfg(desktop)]
use tauri::menu::{Menu, MenuItem};
#[cfg(desktop)]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager as _};

/// Показывает главное окно (из трея или при повторном запуске приложения).
#[cfg(desktop)]
fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Настоящий выход: гасим туннель, возвращаем системный прокси и закрываемся.
/// Закрытие окна крестиком сюда НЕ ведёт — оно только прячет окно в трей, чтобы
/// VPN продолжал работать.
#[cfg(desktop)]
fn quit_app(app: &AppHandle) {
    if let Some(state) = app.try_state::<state::AppState>() {
        state.shutdown_blocking();
    }
    app.exit(0);
}

#[cfg(desktop)]
fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Открыть Zexor VPN", true, None::<&str>)?;
    let disconnect = MenuItem::with_id(app, "disconnect", "Отключить VPN", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Выйти (VPN отключится)", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &disconnect, &quit])?;
    if let Some(state) = app.try_state::<state::AppState>() {
        *state.tray_items.lock().unwrap() = Some(state::TrayItems {
            open: open.clone(),
            disconnect: disconnect.clone(),
            quit: quit.clone(),
        });
    }

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Zexor VPN")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_main_window(app),
            "disconnect" => {
                if let Some(state) = app.try_state::<state::AppState>() {
                    let _ = xray::disconnect_now(&state);
                }
            }
            "quit" => quit_app(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "zexor_vpn_lib=info,warn".into()),
        )
        .init();

    let builder = tauri::Builder::default();

    // Только на компьютерах. Должен идти первым: второй процесс сообщает первому и сразу завершается.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
        show_main_window(app);
    }));
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    #[cfg(target_os = "android")]
    let builder = builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_zexor_vpn::init());

    builder
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            auth::commands::login,
            auth::commands::logout,
            auth::commands::current_session,
            auth::commands::start_telegram_login,
            auth::commands::poll_telegram_login,
            auth::commands::start_browser_login,
            auth::commands::poll_browser_login,
            xray::commands::connect,
            xray::commands::disconnect,
            xray::commands::connection_status,
            xray::commands::list_nodes,
            xray::commands::list_sources,
            xray::commands::add_source,
            xray::commands::remove_source,
            xray::commands::ping_nodes,
            xray::commands::test_node,
            xray::commands::auto_connect,
            xray::commands::crash_report,
            error_report::report_app_error,
            settings::app_settings,
            settings::set_auto,
            settings::set_tunnel_mode,
            settings::restart_as_admin,
            settings::set_routing_mode,
            settings::add_routing_app,
            settings::remove_routing_app,
            settings::list_running_apps,
            settings::set_telemetry,
            settings::set_protocol,
            settings::set_native_labels,
            apk_update::check_apk_update,
            cabinet::cabinet_request,
            cabinet::upload_support_photo,
            cabinet::open_external,
            cabinet::open_payment_url,
            adblock::commands::adblock_settings,
            adblock::commands::set_adblock_enabled,
            adblock::commands::set_adblock_remote,
            adblock::commands::add_adblock_domain,
            adblock::commands::remove_adblock_domain,
            adblock::commands::refresh_adblock_remote,
            adblock::commands::adblock_session_stats,
            adblock::commands::reset_adblock_session_stats,
        ])
        .setup(|app| {
            error_report::install_panic_hook();
            // Не-Windows (Android): корень данных приложения — закрытая папка приложения, а не %LOCALAPPDATA%.
            // Состояние создаём уже после этого, чтобы настройки читались из правильного места.
            #[cfg(not(windows))]
            {
                if let Ok(dir) = app.path().app_local_data_dir() {
                    std::env::set_var("LOCALAPPDATA", dir);
                }
            }
            #[cfg(target_os = "android")]
            {
                auth::set_android_app(app.handle().clone());
                xray::android::set_app(app.handle().clone());
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    xray::android::init_paths(&handle).await;
                });
                xray::android::spawn_quick_actions(app.handle().clone());
            }
            // Прошлый запуск мог завершиться, не вернув системный прокси (убит процесс, выключили компьютер) —
            // тогда интернет не работает, пока прокси смотрит на наш закрытый порт. Чиним на старте.
            #[cfg(windows)]
            {
                zexor_vpn_core::proxy::restore_after_crash();
                zexor_vpn_core::proxy::install_session_end_guard();
            }
            app.manage(state::AppState::default());

            // Жёсткая гарантия: при любом выходе снимаем системный прокси и
            // гасим xray, иначе пользователь останется без интернета.
            let handle = app.handle().clone();
            app.manage(state::ShutdownGuard::new(handle));
            #[cfg(desktop)]
            setup_tray(app)?;
            xray::export_geo_assets_dir(app.handle());
            sync::spawn(app.handle().clone());
            sync::spawn_support_poll(app.handle().clone());

            // Дополнительный список рекламы обновляется раз в сутки, в фоне и без шума.
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Some(state) = app_handle.try_state::<state::AppState>() {
                    adblock::commands::refresh_remote_if_stale(app_handle.clone(), &state).await;
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // Крестик не закрывает приложение: окно прячется в трей, а VPN
            // продолжает работать. Выйти по-настоящему можно из меню трея.
            #[cfg(desktop)]
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
            #[cfg(not(desktop))]
            let _ = (window, event);
        })
        .run(tauri::generate_context!())
        .expect("не удалось запустить приложение");
}
