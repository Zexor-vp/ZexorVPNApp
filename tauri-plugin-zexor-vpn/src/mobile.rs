use serde::Deserialize;
use serde_json::json;
use tauri::plugin::PluginHandle;
use tauri::Runtime;

#[derive(Debug, thiserror::Error)]
pub enum VpnError {
    #[error("{0}")]
    Plugin(String),
}

impl From<tauri::plugin::mobile::PluginInvokeError> for VpnError {
    fn from(err: tauri::plugin::mobile::PluginInvokeError) -> Self {
        VpnError::Plugin(err.to_string())
    }
}

#[derive(Debug, Deserialize)]
struct Granted {
    granted: bool,
}

/// Результат `establish`: дескриптор TUN (уже продублирован для Rust — закрывать его должен Rust).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Established {
    pub fd: i32,
}

/// Пути приложения на устройстве.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Info {
    /// Папка нативных библиотек: там лежит `libxray.so` (исполняемый xray, упакованный как библиотека).
    pub lib_dir: String,
    /// Закрытая папка приложения; geoip/geosite скопированы в `<files_dir>/geo`.
    pub files_dir: String,
    /// VPN-сервис сейчас держит туннель.
    pub running: bool,
    /// Модель устройства и версия Android (для отчётов об ошибках).
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub os_version: String,
}

#[derive(Debug, Deserialize)]
struct CrashText {
    text: String,
}

pub struct Vpn<R: Runtime>(pub PluginHandle<R>);

impl<R: Runtime> Vpn<R> {
    /// Системный запрос «Разрешить приложению создать VPN-подключение». `true` — разрешено.
    pub async fn prepare(&self) -> Result<bool, VpnError> {
        let reply: Granted = self.0.run_mobile_plugin_async("prepare", json!({})).await?;
        Ok(reply.granted)
    }

    /// Поднимает TUN-интерфейс и возвращает его дескриптор.
    pub async fn establish(&self) -> Result<Established, VpnError> {
        Ok(self
            .0
            .run_mobile_plugin_async("establish", json!({}))
            .await?)
    }

    pub async fn info(&self) -> Result<Info, VpnError> {
        Ok(self.0.run_mobile_plugin_async("info", json!({})).await?)
    }

    /// Закрывает TUN и останавливает сервис (блокирующий вызов — его можно делать из любого потока).
    pub fn stop(&self) -> Result<(), VpnError> {
        let _: serde_json::Value = self.0.run_mobile_plugin("stop", json!({}))?;
        Ok(())
    }

    /// Отчёт о прошлом сбое приложения (пустая строка — сбоев не было); при чтении он сбрасывается.
    pub async fn crashes(&self) -> Result<String, VpnError> {
        let reply: CrashText = self.0.run_mobile_plugin_async("crashes", json!({})).await?;
        Ok(reply.text)
    }

    pub fn info_blocking(&self) -> Result<Info, VpnError> {
        Ok(self.0.run_mobile_plugin("info", json!({}))?)
    }
}
