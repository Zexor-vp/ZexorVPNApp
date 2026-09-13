//! Windows-специфичная часть: чтение/запись ветки Internet Settings и
//! уведомление системы о том, что настройки поменялись.

use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::RegKey;

use super::{ProxyError, ProxySnapshot};

const INTERNET_SETTINGS: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

fn open(access: u32) -> Result<RegKey, ProxyError> {
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(INTERNET_SETTINGS, access)
        .map_err(|e| ProxyError::Read(e.to_string()))
}

/// Читает текущее состояние прокси-настроек пользователя.
pub fn read_current() -> Result<ProxySnapshot, ProxyError> {
    let key = open(KEY_READ)?;
    Ok(ProxySnapshot {
        proxy_enable: key.get_value::<u32, _>("ProxyEnable").unwrap_or(0),
        proxy_server: key.get_value::<String, _>("ProxyServer").ok(),
        proxy_override: key.get_value::<String, _>("ProxyOverride").ok(),
        auto_config_url: key.get_value::<String, _>("AutoConfigURL").ok(),
    })
}

/// Включает ручной прокси на указанный `host:port`.
pub fn apply(server: &str) -> Result<(), ProxyError> {
    let key = open(KEY_READ | KEY_WRITE)?;

    key.set_value("ProxyEnable", &1u32)
        .map_err(|e| ProxyError::Write(e.to_string()))?;
    key.set_value("ProxyServer", &server)
        .map_err(|e| ProxyError::Write(e.to_string()))?;
    // Локальные адреса мимо прокси, иначе ломаются локальные сервисы и роутер.
    key.set_value("ProxyOverride", &"<local>")
        .map_err(|e| ProxyError::Write(e.to_string()))?;
    // Автоконфиг (PAC) конфликтует с ручным прокси — на время сеанса убираем.
    let _ = key.delete_value("AutoConfigURL");

    notify_settings_changed();
    Ok(())
}

/// Возвращает настройки к снятому ранее снапшоту.
pub fn write(snapshot: &ProxySnapshot) -> Result<(), ProxyError> {
    let key = open(KEY_READ | KEY_WRITE)?;

    key.set_value("ProxyEnable", &snapshot.proxy_enable)
        .map_err(|e| ProxyError::Write(e.to_string()))?;

    match &snapshot.proxy_server {
        Some(value) => key
            .set_value("ProxyServer", value)
            .map_err(|e| ProxyError::Write(e.to_string()))?,
        // Значения не было — удаляем, а не пишем пустую строку.
        None => {
            let _ = key.delete_value("ProxyServer");
        }
    }

    match &snapshot.proxy_override {
        Some(value) => key
            .set_value("ProxyOverride", value)
            .map_err(|e| ProxyError::Write(e.to_string()))?,
        None => {
            let _ = key.delete_value("ProxyOverride");
        }
    }

    // PAC-скрипт возвращаем, если он был до нас.
    match &snapshot.auto_config_url {
        Some(value) => key
            .set_value("AutoConfigURL", value)
            .map_err(|e| ProxyError::Write(e.to_string()))?,
        None => {
            let _ = key.delete_value("AutoConfigURL");
        }
    }

    notify_settings_changed();
    Ok(())
}

/// Без этого уведомления уже запущенные приложения продолжат ходить напрямую
/// (или через старый прокси) до перезапуска.
fn notify_settings_changed() {
    use windows::Win32::Networking::WinInet::{
        InternetSetOptionW, INTERNET_OPTION_REFRESH, INTERNET_OPTION_SETTINGS_CHANGED,
    };

    unsafe {
        let _ = InternetSetOptionW(None, INTERNET_OPTION_SETTINGS_CHANGED, None, 0);
        let _ = InternetSetOptionW(None, INTERNET_OPTION_REFRESH, None, 0);
    }
}
