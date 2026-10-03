//! Проверка обновлений на Android: встроенный updater работает только для установщика Windows, поэтому здесь
//! смотрим последний релиз на GitHub и отдаём ссылку на APK — пользователь скачивает и ставит его поверх.

use serde::Serialize;
use serde_json::Value;

const LATEST_RELEASE: &str = "https://api.github.com/repos/Zexor-vp/ZexorVPNApp/releases/latest";

#[derive(Debug, Serialize)]
pub struct ApkUpdate {
    pub version: String,
    pub url: String,
}

/// `None` — установлена последняя версия.
#[tauri::command]
pub async fn check_apk_update() -> Result<Option<ApkUpdate>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(format!("ZexorVPN-Android/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .get(LATEST_RELEASE)
        .send()
        .await
        .map_err(|_| "не удалось связаться с сервером обновлений".to_string())?;
    if !response.status().is_success() {
        return Err("сервер обновлений ответил ошибкой, повторите позже".to_string());
    }
    let body = response
        .text()
        .await
        .map_err(|_| "не удалось прочитать ответ сервера обновлений".to_string())?;
    let release: Value = serde_json::from_str(&body)
        .map_err(|_| "сервер обновлений вернул непонятный ответ".to_string())?;
    pick_update(&release, env!("CARGO_PKG_VERSION"))
}

fn pick_update(release: &Value, current: &str) -> Result<Option<ApkUpdate>, String> {
    let tag = release["tag_name"]
        .as_str()
        .unwrap_or("")
        .trim_start_matches('v');
    if tag.is_empty() || !is_newer(tag, current) {
        return Ok(None);
    }
    let url = release["assets"]
        .as_array()
        .and_then(|assets| {
            assets.iter().find_map(|asset| {
                let name = asset["name"].as_str()?;
                let url = asset["browser_download_url"].as_str()?;
                name.ends_with(".apk").then(|| url.to_string())
            })
        })
        .ok_or_else(|| "в новой версии пока нет файла для Android, зайдите позже".to_string())?;
    Ok(Some(ApkUpdate {
        version: tag.to_string(),
        url,
    }))
}

fn parse(version: &str) -> Vec<u64> {
    version
        .split(['.', '-'])
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

fn is_newer(candidate: &str, current: &str) -> bool {
    parse(candidate) > parse(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn newer_versions_are_compared_numerically() {
        assert!(is_newer("0.3.10", "0.3.9"));
        assert!(!is_newer("0.3.8", "0.3.8"));
        assert!(!is_newer("0.3.7", "0.3.8"));
    }

    #[test]
    fn apk_asset_is_picked_from_release() {
        let release = json!({
            "tag_name": "v0.4.0",
            "assets": [
                {"name": "setup.exe", "browser_download_url": "https://x/setup.exe"},
                {"name": "Zexor-VPN-0.4.0-android-arm64.apk", "browser_download_url": "https://x/a.apk"}
            ]
        });
        let update = pick_update(&release, "0.3.8").unwrap().unwrap();
        assert_eq!(update.version, "0.4.0");
        assert_eq!(update.url, "https://x/a.apk");
        assert!(pick_update(&release, "0.4.0").unwrap().is_none());
    }
}
