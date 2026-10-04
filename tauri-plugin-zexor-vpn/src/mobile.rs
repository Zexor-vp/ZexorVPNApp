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
    /// Постоянный идентификатор устройства (переживает переустановку).
    #[serde(default)]
    pub device_id: String,
}

#[derive(Debug, Deserialize)]
struct QuickAction {
    action: String,
}

#[derive(Debug, Deserialize)]
struct CrashText {
    text: String,
}

/// Параметры TUN-интерфейса системного VPN. Без них создаётся интерфейс для xray (весь трафик, адреса по умолчанию).
#[derive(Debug, Clone, serde::Serialize)]
pub struct TunParams {
    /// Адреса интерфейса вида `10.0.0.2/32`.
    pub addresses: Vec<String>,
    /// Маршруты вида `0.0.0.0/0`, которые идут в туннель.
    pub routes: Vec<String>,
    pub dns: Vec<String>,
    pub mtu: u16,
}

pub struct Vpn<R: Runtime>(pub PluginHandle<R>);

impl<R: Runtime> Vpn<R> {
    /// Системный запрос «Разрешить приложению создать VPN-подключение». `true` — разрешено.
    pub async fn prepare(&self) -> Result<bool, VpnError> {
        let reply: Granted = self.0.run_mobile_plugin_async("prepare", json!({})).await?;
        Ok(reply.granted)
    }

    /// Поднимает TUN-интерфейс и возвращает его дескриптор. `None` — интерфейс для xray по умолчанию.
    pub async fn establish(&self, params: Option<TunParams>) -> Result<Established, VpnError> {
        let payload = match params {
            Some(params) => serde_json::to_value(params).unwrap_or_else(|_| json!({})),
            None => json!({}),
        };
        Ok(self.0.run_mobile_plugin_async("establish", payload).await?)
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

    /// Действие с плитки/виджета (`"toggle"`), которое ещё не выполнено; пустая строка — нет. Забирается один раз.
    pub async fn quick_action(&self) -> Result<String, VpnError> {
        let reply: QuickAction = self
            .0
            .run_mobile_plugin_async("quickaction", json!({}))
            .await?;
        Ok(reply.action)
    }

    /// Показывает короткое системное сообщение (когда окна приложения нет на экране).
    pub async fn toast(&self, text: &str) -> Result<(), VpnError> {
        let _: serde_json::Value = self
            .0
            .run_mobile_plugin_async("toast", json!({ "text": text }))
            .await?;
        Ok(())
    }

    /// Убирает окно приложения в фон.
    pub async fn background(&self) -> Result<(), VpnError> {
        let _: serde_json::Value = self
            .0
            .run_mobile_plugin_async("background", json!({}))
            .await?;
        Ok(())
    }

    pub fn info_blocking(&self) -> Result<Info, VpnError> {
        Ok(self.0.run_mobile_plugin("info", json!({}))?)
    }
}
