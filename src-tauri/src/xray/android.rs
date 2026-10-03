//! Android: системный VPN-сервис живёт в Kotlin-плагине (`tauri-plugin-zexor-vpn`), здесь — обвязка вокруг него.
//!
//! Схема подключения: плагин просит у пользователя разрешение на VPN и создаёт TUN-интерфейс; приложение
//! получает его дескриптор и запускает xray (он упакован в APK как `libxray.so`) с `XRAY_TUN_FD`.

use std::path::PathBuf;
use std::sync::OnceLock;

use tauri::{AppHandle, Emitter, Manager, Wry};
use tauri_plugin_zexor_vpn::Vpn;

use super::ConnectError;

#[derive(Debug, Clone)]
pub struct Paths {
    /// Папка нативных библиотек (там `libxray.so`).
    pub lib_dir: PathBuf,
    /// Закрытая папка приложения.
    pub files_dir: PathBuf,
}

static PATHS: OnceLock<Paths> = OnceLock::new();
static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn set_app(app: AppHandle) {
    let _ = APP.set(app);
}

/// Узнаёт у плагина пути приложения и включает `XRAY_LOCATION_ASSET` для баз geoip/geosite.
pub async fn init_paths(app: &AppHandle) {
    let Some(vpn) = app.try_state::<Vpn<Wry>>() else {
        tracing::error!("плагин VPN не найден");
        return;
    };
    match vpn.info().await {
        Ok(info) => {
            crate::error_report::set_device(&info.os_version, &info.model);
            zexor_vpn_core::hwid::set_override(&info.device_id);
            let paths = Paths {
                lib_dir: PathBuf::from(info.lib_dir),
                files_dir: PathBuf::from(info.files_dir),
            };
            std::env::set_var("XRAY_LOCATION_ASSET", paths.files_dir.join("geo"));
            let _ = PATHS.set(paths);
        }
        Err(err) => tracing::error!(?err, "не удалось получить пути приложения от плагина"),
    }
}

/// Ждёт, пока плагин сообщит пути и идентификатор устройства (обычно это миллисекунды после запуска): подписку
/// нельзя скачивать раньше, иначе панель увидит временный идентификатор и займёт лишнее место в лимите устройств.
pub async fn ready() {
    for _ in 0..40 {
        if PATHS.get().is_some() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

pub fn paths() -> Option<&'static Paths> {
    PATHS.get()
}

pub fn xray_binary() -> Option<PathBuf> {
    paths().map(|p| p.lib_dir.join("libxray.so"))
}

/// Переменные окружения для xray: дескриптор TUN и папка с базами.
pub fn xray_env(fd: i32) -> Vec<(String, String)> {
    let mut env = vec![("XRAY_TUN_FD".to_string(), fd.to_string())];
    if let Some(p) = paths() {
        env.push((
            "XRAY_LOCATION_ASSET".to_string(),
            p.files_dir.join("geo").display().to_string(),
        ));
    }
    env
}

/// Разрешение на VPN (системное окно) и создание TUN. Возвращает дескриптор — закрыть его нужно после запуска xray.
pub async fn establish(app: &AppHandle) -> Result<i32, ConnectError> {
    let vpn = app
        .try_state::<Vpn<Wry>>()
        .ok_or_else(|| ConnectError::VpnSetup("плагин VPN не найден".to_string()))?;
    // После быстрого «отключить — включить» прежний сервис ещё может закрывать свой интерфейс: дожидаемся, пока
    // он остановится, иначе старый экземпляр и новый мешают друг другу.
    if vpn.info().await.map(|info| info.running).unwrap_or(false) {
        let _ = vpn.stop();
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if !vpn.info().await.map(|info| info.running).unwrap_or(false) {
                break;
            }
        }
    }
    let granted = vpn
        .prepare()
        .await
        .map_err(|e| ConnectError::VpnSetup(e.to_string()))?;
    if !granted {
        return Err(ConnectError::VpnPermissionDenied);
    }
    let established = vpn
        .establish()
        .await
        .map_err(|e| ConnectError::VpnSetup(e.to_string()))?;
    Ok(established.fd)
}

/// Закрывает свою копию дескриптора (xray уже получил собственную).
pub fn close_fd(fd: i32) {
    // SAFETY: дескриптор получен от плагина и принадлежит только нам.
    unsafe {
        libc::close(fd);
    }
}

/// Останавливает VPN-сервис (закрывает TUN и убирает уведомление).
pub fn stop() {
    if let Some(app) = APP.get() {
        if let Some(vpn) = app.try_state::<Vpn<Wry>>() {
            let _ = vpn.stop();
        }
    }
}

/// «Быстрое включение»: нажатие на плитку шторки или виджет запускает приложение с пометкой, плагин её запоминает,
/// а здесь она подхватывается: VPN переключается (подписка аккаунта, авто-режим), и окно уходит обратно в фон.
/// Если не вышло (нет входа, нет разрешения на VPN и т. п.) — окно остаётся, а причина показывается на главной.
pub fn spawn_quick_actions(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
            // Пока состояние приложения не готово, действие не забираем — иначе оно потеряется.
            if app.try_state::<crate::state::AppState>().is_none() {
                continue;
            }
            let Some(vpn) = app.try_state::<Vpn<Wry>>() else {
                continue;
            };
            let Ok(action) = vpn.quick_action().await else {
                continue;
            };
            if action != "toggle" {
                continue;
            }
            match toggle(&app).await {
                Ok(()) => {
                    let _ = vpn.background().await;
                }
                Err(message) => {
                    let _ = app.emit("quick-error", message);
                }
            }
        }
    });
}

async fn toggle(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<crate::state::AppState>();
    let connected = state.connection.lock().unwrap().process.is_some();
    if connected {
        return super::disconnect_now(&state).map_err(|e| e.to_string());
    }
    let nodes = super::fetch_nodes(&state, Some(zexor_vpn_core::sources::ACCOUNT_SOURCE))
        .await
        .map_err(|e| e.to_string())?;
    super::disconnect_now(&state).map_err(|e| e.to_string())?;
    super::connect_auto(app, &state, &nodes, zexor_vpn_core::sources::ACCOUNT_SOURCE)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}
