//! Настройки подключения: режим туннеля, авто-выбор сервера и маршрутизация по приложениям.
//!
//! Лежат JSON-файлом рядом с остальными данными приложения. Сами правила xray собираются из них
//! при каждом подключении ([`AppSettings::apply_to`]).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::xray::config_builder::{ConfigOptions, TunnelMode};

/// Сколько приложений можно занести в список: каждое — строка в правиле xray.
const MAX_APPS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoutingMode {
    /// Всё через VPN, кроме приложений из списка — они ходят напрямую.
    #[default]
    Exclude,
    /// Через VPN идут только приложения из списка, остальное — напрямую.
    Only,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RoutingSettings {
    pub mode: RoutingMode,
    pub apps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    pub tunnel_mode: TunnelMode,
    /// Авто-выбор сервера включён: список серверов заблокирован, клиент выбирает сам.
    pub auto: bool,
    pub routing: RoutingSettings,
    /// Отправлять на сервер анонимные замеры доступности серверов Zexor из сети пользователя.
    pub telemetry_enabled: bool,
    /// Последняя «эпоха» команды «обновить подписку» от админа, которую приложение уже обработало.
    pub last_sync_epoch: Option<i64>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            tunnel_mode: TunnelMode::Proxy,
            auto: false,
            routing: RoutingSettings::default(),
            telemetry_enabled: true,
            last_sync_epoch: None,
        }
    }
}

/// Имя приложения для правила маршрутизации: из пути берём имя файла (`C:\Games\Steam\steam.exe` → `steam.exe`),
/// папки (`...\Steam\`) оставляем как есть — xray понимает и их.
pub fn normalize_app(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        return Err("введите имя приложения, например chrome.exe".to_string());
    }
    if trimmed
        .chars()
        .any(|c| c.is_control() || matches!(c, '<' | '>' | '|' | '*' | '?'))
    {
        return Err("в имени приложения есть недопустимые символы".to_string());
    }
    let is_folder = trimmed.ends_with(['\\', '/']);
    let name = if is_folder {
        trimmed.replace('\\', "/")
    } else {
        trimmed
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(trimmed)
            .to_string()
    };
    if name.is_empty() || name == "." || name == ".." {
        return Err("не удалось определить имя приложения".to_string());
    }
    Ok(name)
}

impl RoutingSettings {
    pub fn add_app(&mut self, raw: &str) -> Result<String, String> {
        let app = normalize_app(raw)?;
        if self.apps.iter().any(|a| a.eq_ignore_ascii_case(&app)) {
            return Err("это приложение уже в списке".to_string());
        }
        if self.apps.len() >= MAX_APPS {
            return Err(format!("слишком много приложений (максимум {MAX_APPS})"));
        }
        self.apps.push(app.clone());
        Ok(app)
    }

    pub fn remove_app(&mut self, app: &str) {
        self.apps.retain(|a| a != app);
    }
}

impl AppSettings {
    /// Переносит настройки в параметры сборки конфига.
    pub fn apply_to(&self, options: &mut ConfigOptions) {
        options.mode = self.tunnel_mode;
        if self.routing.apps.is_empty() {
            return;
        }
        match self.routing.mode {
            RoutingMode::Exclude => options.direct_processes = self.routing.apps.clone(),
            RoutingMode::Only => options.vpn_only_processes = Some(self.routing.apps.clone()),
        }
    }
}

pub fn config_path() -> PathBuf {
    crate::xray::process::app_data_dir().join("settings.json")
}

pub fn load() -> AppSettings {
    load_from(&config_path())
}

pub fn load_from(path: &Path) -> AppSettings {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(settings: &AppSettings) -> std::io::Result<()> {
    save_to(&config_path(), settings)
}

pub fn save_to(path: &Path, settings: &AppSettings) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(std::io::Error::other)?;
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_names_are_reduced_to_the_file_name() {
        assert_eq!(
            normalize_app(r#" "C:\Games\Steam\steam.exe" "#).unwrap(),
            "steam.exe"
        );
        assert_eq!(normalize_app("/usr/bin/curl").unwrap(), "curl");
        assert_eq!(normalize_app("chrome.exe").unwrap(), "chrome.exe");
        assert_eq!(
            normalize_app(r"C:\Games\Steam\").unwrap(),
            "C:/Games/Steam/"
        );
        assert!(normalize_app("   ").is_err());
        assert!(normalize_app("a|b").is_err());
    }

    #[test]
    fn duplicates_are_rejected_ignoring_case() {
        let mut routing = RoutingSettings::default();
        routing.add_app("Chrome.exe").unwrap();
        assert!(routing.add_app("chrome.EXE").is_err());
        routing.remove_app("Chrome.exe");
        assert!(routing.apps.is_empty());
    }

    #[test]
    fn settings_map_to_config_options() {
        let mut settings = AppSettings {
            tunnel_mode: TunnelMode::Tun,
            ..AppSettings::default()
        };
        settings.routing.apps = vec!["steam.exe".into()];

        let mut options = ConfigOptions::default();
        settings.apply_to(&mut options);
        assert_eq!(options.mode, TunnelMode::Tun);
        assert_eq!(options.direct_processes, ["steam.exe"]);
        assert!(options.vpn_only_processes.is_none());

        settings.routing.mode = RoutingMode::Only;
        let mut options = ConfigOptions::default();
        settings.apply_to(&mut options);
        assert!(options.direct_processes.is_empty());
        assert_eq!(
            options.vpn_only_processes.as_deref(),
            Some(&["steam.exe".to_string()][..])
        );
    }

    #[test]
    fn empty_list_in_only_mode_means_everything_goes_through_vpn() {
        let mut settings = AppSettings::default();
        settings.routing.mode = RoutingMode::Only;
        let mut options = ConfigOptions::default();
        settings.apply_to(&mut options);
        assert!(options.vpn_only_processes.is_none());
    }

    #[test]
    fn missing_or_broken_file_gives_defaults_and_roundtrips() {
        let dir = std::env::temp_dir().join(format!("zexor-settings-{}", std::process::id()));
        let path = dir.join("settings.json");
        assert_eq!(load_from(&path), AppSettings::default());

        let settings = AppSettings {
            auto: true,
            tunnel_mode: TunnelMode::Tun,
            ..AppSettings::default()
        };
        save_to(&path, &settings).unwrap();
        assert_eq!(load_from(&path), settings);

        std::fs::write(&path, "{ не json").unwrap();
        assert_eq!(load_from(&path), AppSettings::default());
        let _ = std::fs::remove_dir_all(dir);
    }
}
