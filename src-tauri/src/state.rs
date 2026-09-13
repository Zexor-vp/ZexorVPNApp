//! Общее состояние приложения между командами Tauri.
//!
//! Всё в `Mutex`, потому что Tauri вызывает команды из пула потоков. Реальная
//! системная логика (парсинг, конфиг, процесс, прокси) — в `zexor-vpn-core`;
//! здесь только оркестрация и то, что обязано пережить `Drop` при выходе.

use std::sync::Mutex;

use tauri::AppHandle;
use zexor_vpn_core::{ApiClient, TokenSet, XrayProcess};

/// Базовый URL API кабинета. Тот же бэкенд, что обслуживает веб-кабинет —
/// nginx на этом домене режет префикс `/api/`, поэтому клиент шлёт запросы с
/// ним же (см. `ApiClient::url`, пути вида `/api/cabinet/...`).
pub const API_BASE_URL: &str = "https://cabinet.zexorvpn.site";

/// Имя сервиса в системном хранилище учётных данных (Windows Credential
/// Manager через крейт `keyring`) — под этим именем ищем refresh-токен.
pub const CREDENTIAL_SERVICE: &str = "ZexorVPN";
pub const CREDENTIAL_USER: &str = "refresh_token";

#[derive(Default)]
pub struct SessionState {
    pub tokens: Option<TokenSet>,
}

#[derive(Default)]
pub struct ConnectionState {
    pub process: Option<XrayProcess>,
    /// Remark ноды, к которой подключены — для отображения в UI.
    pub connected_node: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AdblockSettings {
    pub enabled: bool,
}

impl Default for AdblockSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

pub struct AppState {
    pub api: ApiClient,
    pub session: Mutex<SessionState>,
    pub connection: Mutex<ConnectionState>,
    pub adblock: Mutex<AdblockSettings>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            api: ApiClient::new(API_BASE_URL),
            session: Mutex::new(SessionState::default()),
            connection: Mutex::new(ConnectionState::default()),
            adblock: Mutex::new(AdblockSettings::default()),
        }
    }
}

impl AppState {
    /// Гасит xray и возвращает системный прокси в исходное состояние.
    /// Вызывается и при обычном выходе, и при аварийном (см. `ShutdownGuard`).
    /// Идемпотентна: повторный вызов ничего не ломает.
    pub fn shutdown_blocking(&self) {
        if let Ok(mut connection) = self.connection.lock() {
            if let Some(mut process) = connection.process.take() {
                process.stop();
            }
            connection.connected_node = None;
        }

        if let Err(err) = zexor_vpn_core::proxy::restore() {
            tracing::error!(?err, "не удалось восстановить системный прокси при выходе");
        }
    }
}

/// Гарантирует откат прокси/остановку xray даже если структура роняется не
/// через штатный путь (паника в другом потоке и т.п.) — `Drop` вызывается в
/// любом случае, пока процесс жив.
pub struct ShutdownGuard {
    handle: AppHandle,
}

impl ShutdownGuard {
    pub fn new(handle: AppHandle) -> Self {
        Self { handle }
    }
}

impl Drop for ShutdownGuard {
    fn drop(&mut self) {
        use tauri::Manager;
        if let Some(state) = self.handle.try_state::<AppState>() {
            state.shutdown_blocking();
        }
    }
}
