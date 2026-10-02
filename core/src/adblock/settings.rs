//! Настройки блокировщика рекламы: тумблер, дополнительный список Zexor, свои домены.
//!
//! Хранятся JSON-файлом рядом с остальными данными приложения. Сами правила
//! маршрутизации собираются из этих настроек при каждом подключении
//! (`effective_block` + `custom_allow`).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::is_valid_domain;

/// Дополнительный список поверх встроенного: РСЯ/Yandex RTB и другие рекламные домены,
/// которых нет в общих списках. Отдаётся тем же сервером, что и подписки.
pub const REMOTE_LIST_URL: &str = "https://sub.zexorvpn.site/adblock-filter.txt";

/// Как часто перекачивать дополнительный список.
const REFRESH_INTERVAL_SECS: i64 = 24 * 60 * 60;

/// Верхняя граница на количество своих доменов: каждый превращается в правило xray,
/// а тысячи строк в текстовом поле — почти наверняка вставка чужого списка целиком.
const MAX_CUSTOM_DOMAINS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    Block,
    Allow,
}

impl ListKind {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "block" => Some(Self::Block),
            "allow" => Some(Self::Allow),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AdblockConfig {
    pub enabled: bool,
    /// Подключать ли дополнительный список Zexor.
    pub use_remote: bool,
    /// Свои домены «всегда блокировать».
    pub custom_block: Vec<String>,
    /// Свои домены «никогда не блокировать» — идут через туннель как обычно.
    pub custom_allow: Vec<String>,
    /// Последняя скачанная копия дополнительного списка.
    pub remote_domains: Vec<String>,
    pub remote_updated_at: Option<i64>,
}

impl Default for AdblockConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            use_remote: true,
            custom_block: Vec::new(),
            custom_allow: Vec::new(),
            remote_domains: Vec::new(),
            remote_updated_at: None,
        }
    }
}

/// Приводит то, что пользователь вставил в поле, к чистому домену: `https://www.Example.com/path`
/// → `www.example.com`. Ошибка — человекочитаемая, для показа в интерфейсе.
pub fn normalize_user_domain(raw: &str) -> Result<String, String> {
    let mut value = raw.trim().to_ascii_lowercase();
    for prefix in ["https://", "http://"] {
        if let Some(rest) = value.strip_prefix(prefix) {
            value = rest.to_string();
        }
    }
    let value = value
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .split(':')
        .next()
        .unwrap_or_default()
        .trim_start_matches("*.")
        .trim_start_matches('.')
        .trim_end_matches('.')
        .to_string();
    if is_valid_domain(&value) {
        Ok(value)
    } else {
        Err("введите домен, например ads.example.com".to_string())
    }
}

impl AdblockConfig {
    /// Итоговый список блокировки: встроенный + дополнительный (если включён) + свой.
    /// При выключенном блокировщике — пустой, и конфиг xray строится без правил рекламы.
    pub fn effective_block(&self, bundled: &[String]) -> Vec<String> {
        if !self.enabled {
            return Vec::new();
        }
        let mut all: BTreeSet<String> = bundled.iter().cloned().collect();
        if self.use_remote {
            all.extend(self.remote_domains.iter().cloned());
        }
        all.extend(self.custom_block.iter().cloned());
        all.into_iter().collect()
    }

    fn list_mut(&mut self, kind: ListKind) -> &mut Vec<String> {
        match kind {
            ListKind::Block => &mut self.custom_block,
            ListKind::Allow => &mut self.custom_allow,
        }
    }

    /// Добавляет свой домен. Возвращает нормализованное значение.
    pub fn add_domain(&mut self, kind: ListKind, raw: &str) -> Result<String, String> {
        let domain = normalize_user_domain(raw)?;
        let total = self.custom_block.len() + self.custom_allow.len();
        let list = self.list_mut(kind);
        if list.contains(&domain) {
            return Err("этот домен уже в списке".to_string());
        }
        if total >= MAX_CUSTOM_DOMAINS {
            return Err(format!(
                "слишком много своих доменов (максимум {MAX_CUSTOM_DOMAINS})"
            ));
        }
        list.push(domain.clone());
        Ok(domain)
    }

    pub fn remove_domain(&mut self, kind: ListKind, domain: &str) {
        self.list_mut(kind).retain(|d| d != domain);
    }

    /// Пора ли перекачать дополнительный список.
    pub fn needs_refresh(&self, now_unix: i64) -> bool {
        self.use_remote
            && self
                .remote_updated_at
                .map(|at| now_unix - at >= REFRESH_INTERVAL_SECS)
                .unwrap_or(true)
    }

    pub fn set_remote(&mut self, domains: Vec<String>, now_unix: i64) {
        self.remote_domains = domains;
        self.remote_updated_at = Some(now_unix);
    }
}

pub fn config_path() -> PathBuf {
    crate::xray::process::app_data_dir().join("adblock.json")
}

pub fn load() -> AdblockConfig {
    load_from(&config_path())
}

pub fn load_from(path: &Path) -> AdblockConfig {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(config: &AdblockConfig) -> std::io::Result<()> {
    save_to(&config_path(), config)
}

pub fn save_to(path: &Path, config: &AdblockConfig) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(config).map_err(std::io::Error::other)?;
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_input_is_normalized_to_a_bare_domain() {
        assert_eq!(
            normalize_user_domain(" https://WWW.Example.com:8080/a?b#c ").unwrap(),
            "www.example.com"
        );
        assert_eq!(
            normalize_user_domain("*.ads.example.com").unwrap(),
            "ads.example.com"
        );
        assert!(normalize_user_domain("not a domain").is_err());
        assert!(normalize_user_domain("localhost").is_err());
        assert!(normalize_user_domain("").is_err());
    }

    #[test]
    fn effective_list_merges_sources_and_respects_toggles() {
        let mut cfg = AdblockConfig {
            remote_domains: vec!["rtb.example.net".into()],
            custom_block: vec!["mine.example.org".into()],
            ..AdblockConfig::default()
        };
        let bundled = vec![
            "ads.example.com".to_string(),
            "mine.example.org".to_string(),
        ];
        assert_eq!(
            cfg.effective_block(&bundled),
            ["ads.example.com", "mine.example.org", "rtb.example.net"]
        );

        cfg.use_remote = false;
        assert_eq!(
            cfg.effective_block(&bundled),
            ["ads.example.com", "mine.example.org"]
        );

        cfg.enabled = false;
        assert!(cfg.effective_block(&bundled).is_empty());
    }

    #[test]
    fn custom_lists_reject_duplicates_and_garbage_and_support_removal() {
        let mut cfg = AdblockConfig::default();
        assert_eq!(
            cfg.add_domain(ListKind::Block, "https://Ads.Example.com/x")
                .unwrap(),
            "ads.example.com"
        );
        assert!(cfg.add_domain(ListKind::Block, "ads.example.com").is_err());
        assert!(cfg.add_domain(ListKind::Allow, "???").is_err());
        assert!(cfg.add_domain(ListKind::Allow, "good.example.com").is_ok());
        assert_eq!(cfg.custom_allow, ["good.example.com"]);

        cfg.remove_domain(ListKind::Block, "ads.example.com");
        assert!(cfg.custom_block.is_empty());
    }

    #[test]
    fn custom_domain_count_is_capped() {
        let mut cfg = AdblockConfig::default();
        for i in 0..MAX_CUSTOM_DOMAINS {
            cfg.add_domain(ListKind::Block, &format!("d{i}.example.com"))
                .unwrap();
        }
        assert!(cfg
            .add_domain(ListKind::Allow, "one-more.example.com")
            .is_err());
    }

    #[test]
    fn refresh_is_due_when_never_fetched_or_stale_and_not_when_remote_is_off() {
        let mut cfg = AdblockConfig::default();
        assert!(cfg.needs_refresh(1_000));
        cfg.set_remote(vec![], 1_000);
        assert!(!cfg.needs_refresh(1_000 + 3_600));
        assert!(cfg.needs_refresh(1_000 + REFRESH_INTERVAL_SECS));
        cfg.use_remote = false;
        assert!(!cfg.needs_refresh(1_000 + 10 * REFRESH_INTERVAL_SECS));
    }

    #[test]
    fn roundtrip_and_tolerance_to_old_or_broken_files() {
        let dir = std::env::temp_dir().join(format!("zexor-adblock-{}", std::process::id()));
        let path = dir.join("adblock.json");
        assert_eq!(load_from(&path), AdblockConfig::default());

        let mut cfg = AdblockConfig::default();
        cfg.add_domain(ListKind::Block, "x.example.com").unwrap();
        save_to(&path, &cfg).unwrap();
        assert_eq!(load_from(&path), cfg);

        // Файл старой версии, где было только поле `enabled`, не должен сбрасывать остальное в мусор.
        std::fs::write(&path, r#"{"enabled": false}"#).unwrap();
        let old = load_from(&path);
        assert!(!old.enabled && old.use_remote);

        std::fs::write(&path, "{ broken").unwrap();
        assert_eq!(load_from(&path), AdblockConfig::default());
        let _ = std::fs::remove_dir_all(dir);
    }
}
