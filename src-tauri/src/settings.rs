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
    /// Отправка анонимных замеров доступности серверов Zexor.
    pub telemetry: bool,
}

fn view(settings: &AppSettings) -> SettingsView {
    SettingsView {
        auto: settings.auto,
        tunnel_mode: settings.tunnel_mode,
        routing_mode: settings.routing.mode,
        routing_apps: settings.routing.apps.clone(),
        elevated: zexor_vpn_core::elevation::is_elevated(),
        telemetry: settings.telemetry_enabled,
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
        Ok(())
    })
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
