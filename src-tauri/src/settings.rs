//! Настройки подключения для интерфейса: авто-выбор сервера, режим туннеля, маршрутизация по приложениям.

use serde::Serialize;
use tauri::{AppHandle, State};
use zexor_vpn_core::settings::{self, AppSettings, RoutingMode};
use zexor_vpn_core::TunnelMode;

use crate::state::AppState;
use crate::xray::commands::CommandError;
use crate::xray::reconnect_if_connected;

#[derive(Debug, Serialize)]
pub struct SettingsView {
    pub auto: bool,
    /// `proxy` или `tun`.
    pub tunnel_mode: TunnelMode,
    /// `exclude` или `only`.
    pub routing_mode: RoutingMode,
    pub routing_apps: Vec<String>,
    /// Приложение запущено с правами администратора (нужны для режима TUN).
    pub elevated: bool,
    /// Отправка анонимных замеров и отчётов об ошибках разрешена пользователем.
    pub telemetry: bool,
    /// Пользователь уже ответил на вопрос о согласии.
    pub telemetry_decided: bool,
}

fn view(settings: &AppSettings) -> SettingsView {
    SettingsView {
        auto: settings.auto,
        tunnel_mode: settings.tunnel_mode,
        routing_mode: settings.routing.mode,
        routing_apps: settings.routing.apps.clone(),
        elevated: zexor_vpn_core::elevation::is_elevated(),
        telemetry: settings.telemetry_allowed(),
        telemetry_decided: settings.telemetry_decided,
    }
}

fn other(message: impl ToString) -> CommandError {
    CommandError::Other(message.to_string())
}

/// Применяет изменение к настройкам, сохраняет и возвращает новое состояние.
fn update(
    state: &AppState,
    change: impl FnOnce(&mut AppSettings) -> Result<(), String>,
) -> Result<SettingsView, CommandError> {
    let mut guard = state.settings.lock().unwrap();
    let mut next = guard.clone();
    change(&mut next).map_err(other)?;
    settings::save(&next).map_err(other)?;
    *guard = next;
    Ok(view(&guard))
}

#[tauri::command]
pub fn app_settings(state: State<'_, AppState>) -> SettingsView {
    view(&state.settings.lock().unwrap())
}

/// Включает/выключает авто-выбор сервера. Если сейчас идёт ручное подключение, а авто включили —
/// сразу переключаемся на авто; выключение уже идущее авто-подключение не обрывает.
#[tauri::command]
pub async fn set_auto(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<SettingsView, CommandError> {
    let result = update(&state, |s| {
        s.auto = enabled;
        Ok(())
    })?;
    let (connected, auto_running) = {
        let connection = state.connection.lock().unwrap();
        (connection.connected_node.is_some(), connection.auto)
    };
    if enabled && connected && !auto_running {
        let source_id = state.connection.lock().unwrap().connected_source.clone();
        let nodes = crate::xray::fetch_nodes(&state, source_id.as_deref()).await?;
        let source_id =
            source_id.unwrap_or_else(|| zexor_vpn_core::sources::ACCOUNT_SOURCE.to_string());
        crate::xray::disconnect_now(&state)?;
        crate::xray::connect_auto(&app, &state, &nodes, &source_id).await?;
    }
    Ok(result)
}

/// Переключает режим `proxy`/`tun`. Для TUN нужны права администратора: без них возвращается
/// `NeedsElevation`, и интерфейс предлагает перезапустить приложение (`restart_as_admin`).
#[tauri::command]
pub async fn set_tunnel_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    mode: TunnelMode,
) -> Result<SettingsView, CommandError> {
    if mode == TunnelMode::Tun && !zexor_vpn_core::elevation::is_elevated() {
        return Err(CommandError::NeedsElevation(
            "режиму TUN нужны права администратора".to_string(),
        ));
    }
    let result = update(&state, |s| {
        s.tunnel_mode = mode;
        Ok(())
    })?;
    reconnect_if_connected(&app, &state).await?;
    Ok(result)
}

/// Сохраняет TUN как режим и перезапускает приложение от имени администратора (окно UAC).
#[tauri::command]
pub fn restart_as_admin(app: AppHandle, state: State<'_, AppState>) -> Result<(), CommandError> {
    let exe = std::env::current_exe().map_err(other)?;
    zexor_vpn_core::elevation::relaunch_elevated(&exe).map_err(other)?;
    update(&state, |s| {
        s.tunnel_mode = TunnelMode::Tun;
        Ok(())
    })?;
    // Снимаем туннель и системный прокси — новый экземпляр поднимется сам, когда пользователь нажмёт «Подключиться».
    state.shutdown_blocking();
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub fn set_telemetry(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<SettingsView, CommandError> {
    update(&state, |s| {
        s.telemetry_enabled = enabled;
        s.telemetry_decided = true;
        Ok(())
    })
}

/// Тексты оболочки на языке интерфейса: меню трея и заголовок системного уведомления о новом ответе поддержки.
#[derive(Debug, serde::Deserialize)]
pub struct NativeLabelsInput {
    pub support_reply: String,
    pub tray_open: String,
    pub tray_disconnect: String,
    pub tray_quit: String,
}

#[tauri::command]
pub fn set_native_labels(state: State<'_, AppState>, labels: NativeLabelsInput) {
    {
        let mut current = state.labels.lock().unwrap();
        current.support_reply = labels.support_reply;
        current.tray_open = labels.tray_open.clone();
        current.tray_disconnect = labels.tray_disconnect.clone();
        current.tray_quit = labels.tray_quit.clone();
    }
    #[cfg(desktop)]
    if let Some(items) = state.tray_items.lock().unwrap().as_ref() {
        let _ = items.open.set_text(&labels.tray_open);
        let _ = items.disconnect.set_text(&labels.tray_disconnect);
        let _ = items.quit.set_text(&labels.tray_quit);
    }
}

#[tauri::command]
pub async fn set_routing_mode(
    app: AppHandle,
    state: State<'_, AppState>,
    mode: RoutingMode,
) -> Result<SettingsView, CommandError> {
    let result = update(&state, |s| {
        s.routing.mode = mode;
        Ok(())
    })?;
    reconnect_if_connected(&app, &state).await?;
    Ok(result)
}

#[tauri::command]
pub async fn add_routing_app(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<SettingsView, CommandError> {
    let result = update(&state, |s| s.routing.add_app(&name).map(|_| ()))?;
    reconnect_if_connected(&app, &state).await?;
    Ok(result)
}

#[tauri::command]
pub async fn remove_routing_app(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<SettingsView, CommandError> {
    let result = update(&state, |s| {
        s.routing.remove_app(&name);
        Ok(())
    })?;
    reconnect_if_connected(&app, &state).await?;
    Ok(result)
}

/// Запущенные приложения пользователя — чтобы выбрать из списка, а не вписывать имя файла руками.
#[tauri::command]
pub async fn list_running_apps() -> Vec<String> {
    tauri::async_runtime::spawn_blocking(zexor_vpn_core::apps::running_apps)
        .await
        .unwrap_or_default()
}
