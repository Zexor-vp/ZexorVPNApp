//! Управление системным прокси Windows.
//!
//! Модель работы: перед включением снимаем снапшот текущих значений реестра и
//! сохраняем его на диск. Восстанавливаем ровно то, что было — а не «выключаем
//! прокси», потому что у пользователя мог стоять корпоративный прокси, который
//! мы обязаны вернуть. Снапшот лежит в файле, чтобы пережить аварийное падение
//! приложения: при следующем запуске мы увидим его и откатим настройки.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(windows)]
mod windows_proxy;

#[derive(Debug, Error)]
pub enum ProxyError {
    #[error("не удалось прочитать настройки прокси: {0}")]
    Read(String),
    #[error("не удалось изменить настройки прокси: {0}")]
    Write(String),
    #[error("не удалось сохранить снапшот настроек: {0}")]
    Snapshot(String),
    #[error("управление системным прокси поддерживается только в Windows")]
    Unsupported,
}

/// Снимок пользовательских настроек прокси до нашего вмешательства.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProxySnapshot {
    pub proxy_enable: u32,
    pub proxy_server: Option<String>,
    pub proxy_override: Option<String>,
    pub auto_config_url: Option<String>,
}

/// Путь к файлу снапшота: `%LOCALAPPDATA%\ZexorVPN\proxy-backup.json`.
pub fn snapshot_path() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    base.join("ZexorVPN").join("proxy-backup.json")
}

pub fn save_snapshot(snapshot: &ProxySnapshot) -> Result<(), ProxyError> {
    let path = snapshot_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ProxyError::Snapshot(e.to_string()))?;
    }
    let json =
        serde_json::to_string_pretty(snapshot).map_err(|e| ProxyError::Snapshot(e.to_string()))?;
    std::fs::write(&path, json).map_err(|e| ProxyError::Snapshot(e.to_string()))
}

pub fn load_snapshot() -> Option<ProxySnapshot> {
    let data = std::fs::read_to_string(snapshot_path()).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn clear_snapshot() {
    let _ = std::fs::remove_file(snapshot_path());
}

/// Включает системный прокси на локальный порт xray, предварительно сохранив
/// снапшот прежних настроек.
pub fn enable(http_port: u16) -> Result<(), ProxyError> {
    #[cfg(windows)]
    {
        // Снапшот берём только если его ещё нет: иначе повторный вызов затрёт
        // исходные настройки нашими собственными.
        if load_snapshot().is_none() {
            let current = windows_proxy::read_current()?;
            save_snapshot(&current)?;
        }
        windows_proxy::apply(&format!("127.0.0.1:{http_port}"))
    }
    #[cfg(not(windows))]
    {
        let _ = http_port;
        Err(ProxyError::Unsupported)
    }
}

/// Возвращает настройки ровно в то состояние, в котором они были до `enable`.
pub fn restore() -> Result<(), ProxyError> {
    #[cfg(windows)]
    {
        match load_snapshot() {
            Some(snapshot) => {
                windows_proxy::write(&snapshot)?;
                clear_snapshot();
                Ok(())
            }
            // Снапшота нет — значит мы ничего и не меняли; трогать реестр нельзя.
            None => Ok(()),
        }
    }
    #[cfg(not(windows))]
    {
        Err(ProxyError::Unsupported)
    }
}

/// Вызывается на старте приложения: если остался снапшот, значит прошлый запуск
/// завершился аварийно, не откатив прокси — чиним.
pub fn restore_after_crash() -> bool {
    if load_snapshot().is_some() {
        let restored = restore().is_ok();
        if restored {
            tracing::warn!("обнаружен незавершённый сеанс: системный прокси восстановлен");
        }
        return restored;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_roundtrips_through_disk() {
        // Изолируем тест от реального LOCALAPPDATA.
        let tmp = std::env::temp_dir().join(format!("zexor-proxy-test-{}", std::process::id()));
        std::env::set_var("LOCALAPPDATA", &tmp);

        let snapshot = ProxySnapshot {
            proxy_enable: 1,
            proxy_server: Some("corp-proxy.local:3128".into()),
            proxy_override: Some("<local>".into()),
            auto_config_url: None,
        };

        save_snapshot(&snapshot).unwrap();
        assert_eq!(load_snapshot().as_ref(), Some(&snapshot));

        clear_snapshot();
        assert!(load_snapshot().is_none());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn restore_without_snapshot_is_a_noop_not_an_error() {
        let tmp = std::env::temp_dir().join(format!("zexor-proxy-empty-{}", std::process::id()));
        std::env::set_var("LOCALAPPDATA", &tmp);
        clear_snapshot();

        // Ключевая гарантия: без снапшота мы НЕ трогаем реестр пользователя.
        assert!(load_snapshot().is_none());
        assert!(!restore_after_crash());

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
