//! Подписки, которые пользователь добавил сам (кнопка «+» на главном экране).
//!
//! Подписка аккаунта Zexor приходит из кабинета и здесь не хранится; тут только
//! «чужие» ссылки — например, подписка с другого сервиса, которой человек хочет
//! пользоваться в том же приложении. Лежат обычным JSON рядом с остальными данными
//! приложения: это не секрет уровня пароля, но и не должно жить в localStorage окна.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Идентификатор «встроенного» источника — подписки аккаунта.
pub const ACCOUNT_SOURCE: &str = "account";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub url: String,
}

fn file_path() -> PathBuf {
    crate::xray::process::app_data_dir().join("sources.json")
}

pub fn load() -> Vec<Source> {
    load_from(&file_path())
}

pub fn load_from(path: &std::path::Path) -> Vec<Source> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save(sources: &[Source]) -> std::io::Result<()> {
    save_to(&file_path(), sources)
}

pub fn save_to(path: &std::path::Path, sources: &[Source]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(sources).map_err(std::io::Error::other)?;
    std::fs::write(path, text)
}

/// Подписки на наших собственных адресах (в том числе добавленные вручную по ссылке, например аккаунт близкого человека)
/// панель отдаёт только устройству, которое себя назвало (HWID). Чужим сервисам идентификатор устройства не отправляем,
/// а нашим — обязательно, иначе вместо серверов приходит заглушка «HWID не поддерживается».
pub fn is_zexor_service_url(url: &str) -> bool {
    const OWN_HOSTS: &[&str] = &[
        "sub.zexorvpn.site",
        "sub.zexor.site",
        "cabinet.zexorvpn.site",
        "cabinet.zexor.site",
    ];
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return false;
    }
    let host = authority.split(':').next().unwrap_or("");
    OWN_HOSTS.iter().any(|own| host.eq_ignore_ascii_case(own))
}

/// Адрес зеркала подписки на российском хостинге: тот же код подписки, но запрос идёт через скрипт зеркала.
/// `None`, если ссылка не наша или кода в ней нет.
pub fn mirror_subscription_url(url: &str) -> Option<String> {
    if !is_zexor_service_url(url) {
        return None;
    }
    let rest = url.strip_prefix("https://")?;
    let path = rest.split(['?', '#']).next()?;
    let code = path.rsplit('/').find(|part| !part.is_empty())?;
    let valid = (6..=64).contains(&code.len())
        && code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    // Сам хост тоже может оказаться «последним сегментом» у ссылки без пути — такое зеркалу не отдаём.
    if !valid || code.contains('.') {
        return None;
    }
    Some(format!("https://ru.zexor.site/sub/index.php?p={code}"))
}

pub fn find(id: &str) -> Option<Source> {
    load().into_iter().find(|s| s.id == id)
}

/// Принимаем только обычные web-ссылки: подписка скачивается HTTP-запросом, а
/// `file://` и прочие схемы — это чтение локальных файлов по команде из текстового поля.
pub fn validate_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    let lower = trimmed.to_ascii_lowercase();
    if !(lower.starts_with("https://") || lower.starts_with("http://")) {
        return Err("ссылка подписки должна начинаться с https://".to_string());
    }
    if trimmed.len() > 4096 || trimmed.chars().any(char::is_whitespace) {
        return Err("некорректная ссылка подписки".to_string());
    }
    Ok(trimmed.to_string())
}

pub fn new_id() -> String {
    let mut bytes = [0u8; 6];
    let _ = getrandom::getrandom(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_url_keeps_only_the_subscription_code() {
        assert_eq!(
            mirror_subscription_url("https://sub.zexorvpn.site/fLeDot5XDhCx5DYq").as_deref(),
            Some("https://ru.zexor.site/sub/index.php?p=fLeDot5XDhCx5DYq")
        );
        assert_eq!(
            mirror_subscription_url("https://sub.zexor.site/api/sub/zxf_SkfVgSRMCNQ1?x=1")
                .as_deref(),
            Some("https://ru.zexor.site/sub/index.php?p=zxf_SkfVgSRMCNQ1")
        );
        assert!(mirror_subscription_url("https://example.com/abcdefgh").is_none());
        assert!(mirror_subscription_url("https://sub.zexorvpn.site/").is_none());
        assert!(mirror_subscription_url("https://sub.zexorvpn.site/ab").is_none());
    }

    #[test]
    fn own_subscription_hosts_are_recognised() {
        assert!(is_zexor_service_url(
            "https://sub.zexorvpn.site/api/sub/abc"
        ));
        assert!(is_zexor_service_url("https://SUB.zexor.site/api/sub/abc"));
        assert!(!is_zexor_service_url(
            "https://sub.zexorvpn.site.evil.example/x"
        ));
        assert!(!is_zexor_service_url(
            "https://sub.zexorvpn.site@evil.example/x"
        ));
        assert!(!is_zexor_service_url("http://sub.zexorvpn.site/x"));
        assert!(!is_zexor_service_url("https://example.com/sub"));
    }

    #[test]
    fn roundtrip_through_disk() {
        let dir = std::env::temp_dir().join(format!("zexor-sources-{}", new_id()));
        let path = dir.join("sources.json");
        assert!(load_from(&path).is_empty());

        let list = vec![Source {
            id: "a1".into(),
            name: "Другой сервис".into(),
            url: "https://example.com/sub".into(),
        }];
        save_to(&path, &list).unwrap();
        assert_eq!(load_from(&path), list);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_file_is_treated_as_empty() {
        let dir = std::env::temp_dir().join(format!("zexor-sources-{}", new_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sources.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert!(load_from(&path).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn only_http_links_are_accepted() {
        assert!(validate_url(" https://sub.example.com/abc ").is_ok());
        assert!(validate_url("http://sub.example.com/abc").is_ok());
        assert!(validate_url("file:///C:/secret.txt").is_err());
        assert!(validate_url("vless://x@h:1").is_err());
        assert!(validate_url("https://a b").is_err());
    }
}
