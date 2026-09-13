//! Оркестрация подключения: получить подписку → распарсить узлы → собрать
//! конфиг → поднять процесс → включить системный прокси.

pub mod commands;

use std::path::PathBuf;

use tauri::{AppHandle, Manager};
use zexor_vpn_core::{ConfigOptions, VlessNode, XrayProcess};

use crate::auth::{ensure_valid_access_token, AuthError};
use crate::state::AppState;

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error("у аккаунта нет активной подписки")]
    NoSubscription,
    #[error("не удалось получить содержимое подписки: {0}")]
    FetchSubscription(String),
    #[error("не удалось разобрать подписку: {0}")]
    Parse(#[from] zexor_vpn_core::ParseError),
    #[error("узел \"{0}\" не найден в подписке")]
    NodeNotFound(String),
    #[error("бинарник xray не найден: {0}")]
    BinaryMissing(String),
    #[error(transparent)]
    Process(#[from] zexor_vpn_core::XrayError),
    #[error("не удалось включить системный прокси: {0}")]
    Proxy(String),
}

/// Скачивает и разбирает актуальный список узлов подписки текущего пользователя.
pub async fn fetch_nodes(state: &AppState) -> Result<Vec<VlessNode>, ConnectError> {
    let access_token = ensure_valid_access_token(state).await?;

    let status = state
        .api
        .subscription_info(&access_token)
        .await
        .map_err(AuthError::Api)?;

    let subscription = status.subscription.ok_or(ConnectError::NoSubscription)?;
    let subscription_url = subscription
        .subscription_url
        .ok_or(ConnectError::NoSubscription)?;

    let body = reqwest::get(&subscription_url)
        .await
        .map_err(|e| ConnectError::FetchSubscription(e.to_string()))?
        .text()
        .await
        .map_err(|e| ConnectError::FetchSubscription(e.to_string()))?;

    Ok(zexor_vpn_core::parse_subscription(&body)?)
}

/// Путь к бандленному sidecar-бинарнику xray.
///
/// Имя файла жёстко зашито под конкретный target triple, которым его кладёт
/// наш собственный CI (`.github/workflows/build.yml`) — там же, где Tauri
/// ожидает sidecar по конвенции `<externalBin>-<target-triple>.exe`. Приложение
/// собирается только под Windows x64, поэтому один вариант достаточен.
pub fn resolve_xray_binary(app: &AppHandle) -> Result<PathBuf, ConnectError> {
    let resource_dir = app
        .path()
        .resource_dir()
        .map_err(|e| ConnectError::BinaryMissing(e.to_string()))?;
    let path = resource_dir
        .join("binaries")
        .join("xray-x86_64-pc-windows-msvc.exe");

    if !path.exists() {
        return Err(ConnectError::BinaryMissing(path.display().to_string()));
    }
    Ok(path)
}

/// Читает бандленный дефолтный блок-лист рекламы. Отсутствие файла не
/// считается ошибкой — просто подключаемся без адблока.
pub fn load_bundled_blocklist(app: &AppHandle) -> Vec<String> {
    let Ok(resource_dir) = app.path().resource_dir() else {
        return Vec::new();
    };
    let path = resource_dir
        .join("resources")
        .join("adblock")
        .join("default.txt");
    match std::fs::read_to_string(&path) {
        Ok(contents) => zexor_vpn_core::adblock::parse_blocklist(&contents),
        Err(err) => {
            tracing::warn!(?err, path = %path.display(), "не удалось прочитать дефолтный блок-лист");
            Vec::new()
        }
    }
}

/// Полный цикл подключения к конкретному узлу подписки.
pub async fn connect_to_node(
    app: &AppHandle,
    state: &AppState,
    node: &VlessNode,
) -> Result<(), ConnectError> {
    let binary = resolve_xray_binary(app)?;

    let adblock_domains = if state.adblock.lock().unwrap().enabled {
        load_bundled_blocklist(app)
    } else {
        Vec::new()
    };

    let options = ConfigOptions {
        adblock_domains,
        ..ConfigOptions::default()
    };
    let config = zexor_vpn_core::build_config(node, &options);
    let config_path = zexor_vpn_core::xray::process::config_file_path();

    let process = XrayProcess::start(&binary, &config, &config_path)?;

    zexor_vpn_core::proxy::enable(options.http_port)
        .map_err(|e| ConnectError::Proxy(e.to_string()))?;

    let mut connection = state.connection.lock().unwrap();
    connection.process = Some(process);
    connection.connected_node = Some(node.remark.clone());

    Ok(())
}

/// Останавливает туннель и откатывает системный прокси. Безопасно вызывать,
/// даже если подключения не было.
pub fn disconnect_now(state: &AppState) -> Result<(), ConnectError> {
    let mut connection = state.connection.lock().unwrap();
    if let Some(mut process) = connection.process.take() {
        process.stop();
    }
    connection.connected_node = None;
    drop(connection);

    zexor_vpn_core::proxy::restore().map_err(|e| ConnectError::Proxy(e.to_string()))
}
