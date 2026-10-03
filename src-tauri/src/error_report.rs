//! Отправка ошибок и вылетов приложения на сервер (раздел «Ошибки приложений» в админ-панели кабинета).
//!
//! Без идентификатора пользователя и IP: версия приложения, система, модель устройства и текст ошибки.
//! Уважает тот же переключатель «Статистика», что и замеры серверов, не больше 20 отчётов за запуск и не шлёт
//! одно и то же сообщение дважды.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use serde_json::json;
use zexor_vpn_core::ApiClient;

use crate::state::API_BASE_URL;

const MAX_PER_SESSION: usize = 20;
const MAX_MESSAGE_CHARS: usize = 3900;

static CLIENT: OnceLock<ApiClient> = OnceLock::new();
static SENT: Mutex<Option<HashSet<String>>> = Mutex::new(None);
static DEVICE: Mutex<(String, String)> = Mutex::new((String::new(), String::new()));

/// Запоминает описание устройства (Android узнаёт его у плагина).
pub fn set_device(os_version: &str, model: &str) {
    if let Ok(mut device) = DEVICE.lock() {
        *device = (os_version.to_string(), model.to_string());
    }
}

fn platform() -> &'static str {
    if cfg!(target_os = "android") {
        "android"
    } else if cfg!(windows) {
        "windows"
    } else {
        "other"
    }
}

/// Отправляет отчёт в фоне. `kind`: `crash`, `panic`, `connect`, `js`, `other`.
pub fn report(kind: &str, message: impl Into<String>) {
    let message: String = message.into();
    let message = message.trim();
    if message.is_empty() || !zexor_vpn_core::settings::load().telemetry_enabled {
        return;
    }
    let message: String = message.chars().take(MAX_MESSAGE_CHARS).collect();
    {
        let Ok(mut sent) = SENT.lock() else { return };
        let sent = sent.get_or_insert_with(HashSet::new);
        if sent.len() >= MAX_PER_SESSION || !sent.insert(format!("{kind}:{message}")) {
            return;
        }
    }
    let (os_version, device_model) = DEVICE.lock().map(|d| d.clone()).unwrap_or_default();
    let body = json!({
        "kind": kind,
        "platform": platform(),
        "app_version": env!("CARGO_PKG_VERSION"),
        "os_version": (!os_version.is_empty()).then_some(os_version),
        "device_model": (!device_model.is_empty()).then_some(device_model),
        "message": message,
    });
    tauri::async_runtime::spawn(async move {
        let client = CLIENT.get_or_init(|| ApiClient::new(API_BASE_URL));
        let _ = client.report_app_error(&body).await;
    });
}

/// Паника в любом потоке тоже уходит на сервер (затем отрабатывает обычный обработчик).
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        report("panic", info.to_string());
        previous(info);
    }));
}

/// Ошибки интерфейса (необработанные исключения JS) отправляет фронтенд.
#[tauri::command]
pub fn report_app_error(kind: String, message: String) {
    let kind = if kind == "js" { "js" } else { "other" };
    report(kind, message);
}
