//! Идентификатор устройства для панели.
//!
//! Remnawave считает подключённые устройства по заголовку `x-hwid`: каждое новое устройство занимает
//! место в лимите, а при исчерпании лимита вместо серверов приходит заглушка «HWID: …». Приложение
//! должно присылать один и тот же стабильный id, иначе каждый запуск будет «новым устройством».

use std::path::{Path, PathBuf};

pub fn file_path() -> PathBuf {
    crate::xray::process::app_data_dir().join("device_id")
}

/// Читает сохранённый id или создаёт новый (случайный UUID v4) и сохраняет его.
pub fn load_or_create() -> String {
    load_or_create_at(&file_path())
}

pub fn load_or_create_at(path: &Path) -> String {
    if let Ok(existing) = std::fs::read_to_string(path) {
        let id = existing.trim();
        if is_valid(id) {
            return id.to_string();
        }
    }
    let id = new_uuid_v4();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(path, &id);
    id
}

fn is_valid(id: &str) -> bool {
    id.len() == 36 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

fn new_uuid_v4() -> String {
    let mut bytes = [0u8; 16];
    // Без системного генератора откатываемся на время+pid: id всё равно будет уникальным на машине.
    if getrandom::getrandom(&mut bytes).is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        bytes.copy_from_slice(&(nanos ^ (u128::from(std::process::id()) << 64)).to_le_bytes());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// Заголовки, которыми приложение представляется панели при скачивании подписки: `Happ/…` в User-Agent
/// даёт формат с готовыми профилями, остальное регистрирует устройство в списке пользователя.
pub fn subscription_headers(hwid: &str) -> Vec<(&'static str, String)> {
    // Панель показывает пользователю в списке устройств именно эти значения.
    let (agent, device_os, model) = if cfg!(target_os = "android") {
        ("Android", "Android", "Zexor VPN Android")
    } else {
        ("Desktop", "Windows", "Zexor VPN Desktop")
    };
    vec![
        (
            "User-Agent",
            format!("Happ/2.0.0 ZexorVPN-{agent}/{}", env!("CARGO_PKG_VERSION")),
        ),
        ("x-hwid", hwid.to_string()),
        ("x-device-os", device_os.to_string()),
        ("x-ver-os", std::env::consts::OS.to_string()),
        ("x-device-model", model.to_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_stable_across_calls_and_looks_like_a_uuid_v4() {
        let dir = std::env::temp_dir().join(format!("zexor-hwid-{}", std::process::id()));
        let path = dir.join("device_id");
        let first = load_or_create_at(&path);
        assert_eq!(first, load_or_create_at(&path));
        assert_eq!(first.len(), 36);
        assert_eq!(first.as_bytes()[14], b'4');
        assert!(matches!(first.as_bytes()[19], b'8' | b'9' | b'a' | b'b'));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_file_is_replaced_with_a_fresh_id() {
        let dir = std::env::temp_dir().join(format!("zexor-hwid-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("device_id");
        std::fs::write(&path, "garbage").unwrap();
        let id = load_or_create_at(&path);
        assert!(is_valid(&id));
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), id);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn headers_identify_a_happ_style_client_and_the_device() {
        let headers = subscription_headers("abc");
        assert!(headers
            .iter()
            .any(|(k, v)| *k == "User-Agent" && v.starts_with("Happ/")));
        assert!(headers.iter().any(|(k, v)| *k == "x-hwid" && v == "abc"));
    }
}
