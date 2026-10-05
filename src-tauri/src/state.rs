//! Общее состояние приложения между командами Tauri.
//!
//! Всё в `Mutex`, потому что Tauri вызывает команды из пула потоков. Реальная
//! системная логика (парсинг, конфиг, процесс, прокси) — в `zexor-vpn-core`;
//! здесь только оркестрация и то, что обязано пережить `Drop` при выходе.

use std::sync::Mutex;

use tauri::AppHandle;
use zexor_vpn_core::adblock::counter::BlockCounter;
use zexor_vpn_core::adblock::settings::AdblockConfig;
use zexor_vpn_core::settings::AppSettings;
use zexor_vpn_core::{ApiClient, TokenSet, XrayProcess};

/// Запущенный туннель: xray или AmneziaWG (Android и Windows). Живёт, пока жив процесс движка.
pub enum Tunnel {
    Xray(XrayProcess),
    #[cfg(any(unix, windows))]
    Awg(zexor_vpn_core::awg::AwgProcess),
}

impl Tunnel {
    pub fn is_running(&mut self) -> bool {
        match self {
            Tunnel::Xray(process) => process.is_running(),
            #[cfg(any(unix, windows))]
            Tunnel::Awg(process) => process.is_running(),
        }
    }

    pub fn stop(&mut self) {
        match self {
            Tunnel::Xray(process) => process.stop(),
            #[cfg(any(unix, windows))]
            Tunnel::Awg(process) => process.stop(),
        }
    }
}

/// Базовый URL API кабинета. Тот же бэкенд, что обслуживает веб-кабинет —
/// nginx на этом домене режет префикс `/api/`, поэтому клиент шлёт запросы с
/// ним же (см. `ApiClient::url`, пути вида `/api/cabinet/...`).
pub const API_BASE_URL: &str = "https://cabinet.zexorvpn.site";

/// Адрес веб-кабинета, на котором открывается страница входа в браузере.
/// Именно этот домен зарегистрирован как redirect URI у Google OAuth, поэтому
/// вход через Google возможен только здесь (а не на `API_BASE_URL`).
pub const WEB_LOGIN_BASE_URL: &str = "https://cabinet.zexor.site";

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
    pub process: Option<Tunnel>,
    /// Remark ноды, к которой подключены — для отображения в UI.
    pub connected_node: Option<String>,
    /// Из какой подписки взят узел (`account` или id добавленной пользователем) —
    /// нужно, чтобы переподключиться к тому же узлу после смены настроек.
    pub connected_source: Option<String>,
    /// Подключение идёт в авто-режиме (балансировщик или самый быстрый сервер).
    pub auto: bool,
}

/// Сколько рекламных соединений заблокировано с запуска приложения.
#[derive(Default)]
pub struct BlockStats {
    /// Счётчик по журналу доступа текущего туннеля (есть только пока подключены).
    pub counter: Option<BlockCounter>,
    /// Накоплено по прошлым подключениям этого запуска.
    pub accumulated: u64,
}

/// Тексты, которые показывает сама оболочка (трей, системные уведомления): язык интерфейса живёт во
/// фронтенде, поэтому он присылает их сюда при запуске и при смене языка.
#[derive(Debug, Clone)]
pub struct NativeLabels {
    pub support_reply: String,
    pub tray_open: String,
    pub tray_disconnect: String,
    pub tray_quit: String,
    /// Язык интерфейса (код): по нему сервер называет серверы AmneziaWG/WireGuard.
    pub language: String,
}

impl Default for NativeLabels {
    fn default() -> Self {
        Self {
            support_reply: "Ответ поддержки".to_string(),
            tray_open: "Открыть Zexor VPN".to_string(),
            tray_disconnect: "Отключить VPN".to_string(),
            tray_quit: "Выйти (VPN отключится)".to_string(),
            language: String::new(),
        }
    }
}

/// Пункты меню трея — чтобы переименовать их при смене языка (трей есть только на компьютерах).
#[cfg(desktop)]
pub struct TrayItems {
    pub open: tauri::menu::MenuItem<tauri::Wry>,
    pub disconnect: tauri::menu::MenuItem<tauri::Wry>,
    pub quit: tauri::menu::MenuItem<tauri::Wry>,
}

pub struct AppState {
    pub api: ApiClient,
    pub session: Mutex<SessionState>,
    pub connection: Mutex<ConnectionState>,
    pub adblock: Mutex<AdblockConfig>,
    pub block_stats: Mutex<BlockStats>,
    pub settings: Mutex<AppSettings>,
    pub labels: Mutex<NativeLabels>,
    #[cfg(desktop)]
    pub tray_items: Mutex<Option<TrayItems>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            api: ApiClient::new(API_BASE_URL),
            session: Mutex::new(SessionState::default()),
            connection: Mutex::new(ConnectionState::default()),
            adblock: Mutex::new(zexor_vpn_core::adblock::settings::load()),
            block_stats: Mutex::new(BlockStats::default()),
            settings: Mutex::new(zexor_vpn_core::settings::load()),
            labels: Mutex::new(NativeLabels::default()),
            #[cfg(desktop)]
            tray_items: Mutex::new(None),
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
            connection.connected_source = None;
            connection.auto = false;
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
