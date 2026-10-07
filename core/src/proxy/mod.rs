//! Управление системным прокси Windows.
//!
//! Модель работы: перед включением снимаем снапшот текущих значений реестра и
//! сохраняем его на диск. Восстанавливаем ровно то, что было — а не «выключаем
//! прокси», потому что у пользователя мог стоять корпоративный прокси, который
//! мы обязаны вернуть. Снапшот лежит в файле, чтобы пережить аварийное падение
//! приложения: при следующем запуске мы увидим его и откатим настройки.

use std::path::{Path, PathBuf};

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
    crate::xray::process::app_data_dir()
        .join("ZexorVPN")
        .join("proxy-backup.json")
}

/// Пути передаются явно: глобальный `LOCALAPPDATA` протекал бы между
/// параллельными тестами, а ошибиться здесь — значит не восстановить прокси.
pub fn save_snapshot_to(path: &Path, snapshot: &ProxySnapshot) -> Result<(), ProxyError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ProxyError::Snapshot(e.to_string()))?;
    }
    let json =
        serde_json::to_string_pretty(snapshot).map_err(|e| ProxyError::Snapshot(e.to_string()))?;
    std::fs::write(path, json).map_err(|e| ProxyError::Snapshot(e.to_string()))
}

pub fn load_snapshot_from(path: &Path) -> Option<ProxySnapshot> {
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

pub fn clear_snapshot_at(path: &Path) {
    let _ = std::fs::remove_file(path);
}

pub fn save_snapshot(snapshot: &ProxySnapshot) -> Result<(), ProxyError> {
    save_snapshot_to(&snapshot_path(), snapshot)
}

pub fn load_snapshot() -> Option<ProxySnapshot> {
    load_snapshot_from(&snapshot_path())
}

pub fn clear_snapshot() {
    clear_snapshot_at(&snapshot_path())
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

/// Порты нашего HTTP-прокси: если системный прокси остался на одном из них, а на нём никто не слушает, это наш
/// «осиротевший» прокси (приложение завершили без отката), и он отрезает интернет.
const OWN_HTTP_PORTS: &[u16] = &[10809];

/// «Осиротевший» ли системный прокси: указывает на локальный порт из [`OWN_HTTP_PORTS`], а на нём никто не слушает.
/// Чужие локальные прокси (на других портах или реально работающие) не трогаем.
pub fn is_orphaned_local_proxy(server: &str, port_is_listening: impl Fn(u16) -> bool) -> bool {
    let server = server
        .trim()
        .trim_start_matches("http=")
        .trim_start_matches("https=");
    let Some((host, port)) = server.rsplit_once(':') else {
        return false;
    };
    if host != "127.0.0.1" && !host.eq_ignore_ascii_case("localhost") {
        return false;
    }
    let Ok(port) = port.trim().parse::<u16>() else {
        return false;
    };
    OWN_HTTP_PORTS.contains(&port) && !port_is_listening(port)
}

/// Вызывается на старте приложения: если остался снапшот, значит прошлый запуск
/// завершился аварийно, не откатив прокси — чиним. Если снапшота нет (потерян), а системный прокси всё ещё
/// смотрит на наш порт, на котором никто не слушает, — просто отключаем его.
pub fn restore_after_crash() -> bool {
    if load_snapshot().is_some() {
        let restored = restore().is_ok();
        if restored {
            tracing::warn!("обнаружен незавершённый сеанс: системный прокси восстановлен");
        }
        return restored;
    }
    #[cfg(windows)]
    {
        if let Ok(current) = windows_proxy::read_current() {
            if current.proxy_enable == 1 {
                if let Some(server) = &current.proxy_server {
                    let listening = |port: u16| {
                        std::net::TcpStream::connect_timeout(
                            &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
                            std::time::Duration::from_millis(300),
                        )
                        .is_ok()
                    };
                    if is_orphaned_local_proxy(server, listening) {
                        let fixed = windows_proxy::disable().is_ok();
                        if fixed {
                            tracing::warn!(
                                "системный прокси указывал на наш закрытый порт: отключён"
                            );
                        }
                        return fixed;
                    }
                }
            }
        }
    }
    false
}

/// Следит за завершением сеанса Windows (выключение, выход из системы) и возвращает системный прокси до того, как
/// процесс убьют. На других системах ничего не делает.
pub fn install_session_end_guard() {
    #[cfg(windows)]
    windows_proxy::install_session_end_guard();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_snapshot_path(tag: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("zexor-proxy-{}-{tag}", std::process::id()))
            .join("proxy-backup.json")
    }

    #[test]
    fn orphaned_local_proxy_is_detected_only_for_our_dead_port() {
        // Наш порт, никто не слушает — осиротевший.
        assert!(is_orphaned_local_proxy("127.0.0.1:10809", |_| false));
        // Кто-то слушает (например, другой клиент на том же порту) — не трогаем.
        assert!(!is_orphaned_local_proxy("127.0.0.1:10809", |_| true));
        // Чужой порт или чужой хост — не трогаем.
        assert!(!is_orphaned_local_proxy("127.0.0.1:8080", |_| false));
        assert!(!is_orphaned_local_proxy("corp-proxy.local:3128", |_| false));
        // Формат «по протоколам» тоже понимаем.
        assert!(is_orphaned_local_proxy("http=127.0.0.1:10809", |_| false));
        assert!(!is_orphaned_local_proxy("", |_| false));
    }

    #[test]
    fn snapshot_roundtrips_through_disk() {
        let path = temp_snapshot_path("roundtrip");

        let snapshot = ProxySnapshot {
            proxy_enable: 1,
            proxy_server: Some("corp-proxy.local:3128".into()),
            proxy_override: Some("<local>".into()),
            auto_config_url: None,
        };

        save_snapshot_to(&path, &snapshot).unwrap();
        // Корпоративный прокси должен вернуться ровно таким, каким был.
        assert_eq!(load_snapshot_from(&path).as_ref(), Some(&snapshot));

        clear_snapshot_at(&path);
        assert!(load_snapshot_from(&path).is_none());
    }

    #[test]
    fn missing_snapshot_reads_as_none_so_registry_is_left_alone() {
        // Ключевая гарантия: без снапшота мы НЕ трогаем реестр пользователя.
        let path = temp_snapshot_path("absent");
        clear_snapshot_at(&path);
        assert!(load_snapshot_from(&path).is_none());
    }

    #[test]
    fn corrupted_snapshot_is_treated_as_absent() {
        // Битый файл не должен приводить к записи мусора в реестр.
        let path = temp_snapshot_path("corrupt");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ это не json").unwrap();
        assert!(load_snapshot_from(&path).is_none());
        clear_snapshot_at(&path);
    }
}
