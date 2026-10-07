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

/// Отключает системный прокси, не трогая остальные значения (если снапшота нет, а прокси остался нашим).
pub fn disable() -> Result<(), ProxyError> {
    let key = open(KEY_READ | KEY_WRITE)?;
    key.set_value("ProxyEnable", &0u32)
        .map_err(|e| ProxyError::Write(e.to_string()))?;
    let _ = key.delete_value("ProxyServer");
    notify_settings_changed();
    Ok(())
}

/// Скрытое окно верхнего уровня, которое получает `WM_ENDSESSION`: при выключении компьютера или выходе из системы
/// Windows убивает процессы без предупреждения, и без этого системный прокси остался бы указывать на мёртвый порт —
/// после следующего включения интернета не было бы, пока приложение не запустят снова.
pub fn install_session_end_guard() {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("zexor-session-end".to_string())
            .spawn(run_session_end_window);
    });
}

fn run_session_end_window() {
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
        TranslateMessage, MSG, WINDOW_EX_STYLE, WM_ENDSESSION, WM_QUERYENDSESSION, WNDCLASSW,
        WS_OVERLAPPED,
    };

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match message {
            // Нас не держим: сеанс завершать можно.
            WM_QUERYENDSESSION => LRESULT(1),
            WM_ENDSESSION => {
                // wparam != 0 — сеанс действительно завершается: возвращаем прокси, пока процесс ещё жив.
                if wparam.0 != 0 {
                    let _ = super::restore();
                }
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, message, wparam, lparam),
        }
    }

    unsafe {
        let Ok(instance) = GetModuleHandleW(None) else {
            return;
        };
        let class = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance.into(),
            lpszClassName: w!("ZexorVpnSessionGuard"),
            ..Default::default()
        };
        if RegisterClassW(&class) == 0 {
            return;
        }
        let Ok(_window) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("ZexorVpnSessionGuard"),
            w!("Zexor VPN session guard"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            instance,
            None,
        ) else {
            return;
        };
        // Окно не показываем (ShowWindow не вызываем): оно нужно только ради сообщений.
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}
