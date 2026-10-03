//! Tauri-команды подключения и управления подписками, вызываемые фронтендом.

use serde::Serialize;
use std::time::Duration;

use tauri::{AppHandle, State};
use zexor_vpn_core::probe;

use super::{
    connect_auto, connect_to_node, disconnect_now, download_subscription, fetch_nodes,
    resolve_xray_binary, ConnectError,
};
use crate::state::AppState;
use zexor_vpn_core::sources::{self, Source, ACCOUNT_SOURCE};

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum CommandError {
    NeedsLogin(String),
    NoSubscription(String),
    Network(String),
    /// Режиму TUN нужны права администратора — интерфейс предложит перезапуск.
    NeedsElevation(String),
    Other(String),
}

impl From<ConnectError> for CommandError {
    fn from(err: ConnectError) -> Self {
        match err {
            ConnectError::Auth(crate::auth::AuthError::NeedsLogin) => {
                CommandError::NeedsLogin(err.to_string())
            }
            ConnectError::Auth(crate::auth::AuthError::Api(zexor_vpn_core::ApiError::Network(
                _,
            )))
            | ConnectError::FetchSubscription(_) => CommandError::Network(err.to_string()),
            ConnectError::NoSubscription => CommandError::NoSubscription(err.to_string()),
            ConnectError::NeedsElevation => CommandError::NeedsElevation(err.to_string()),
            other => CommandError::Other(other.to_string()),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct NodeSummary {
    pub remark: String,
    /// `vless` или `wireguard`.
    pub protocol: String,
    pub is_reality: bool,
}

#[derive(Debug, Serialize)]
pub struct NodesResponse {
    pub nodes: Vec<NodeSummary>,
}

#[tauri::command]
pub async fn list_nodes(
    state: State<'_, AppState>,
    source_id: Option<String>,
) -> Result<NodesResponse, CommandError> {
    let nodes = fetch_nodes(&state, source_id.as_deref()).await?;
    Ok(NodesResponse {
        nodes: nodes
            .iter()
            // Панельный «AUTO» — не сервер: его место занимает переключатель «Авто выбор».
            .filter(|n| !n.is_balanced())
            .map(|n| NodeSummary {
                remark: n.remark().to_string(),
                protocol: n.protocol().to_string(),
                is_reality: n.is_reality(),
            })
            .collect(),
    })
}

#[tauri::command]
pub async fn connect(
    app: AppHandle,
    state: State<'_, AppState>,
    node_remark: String,
    source_id: Option<String>,
) -> Result<(), CommandError> {
    let source_id = source_id.unwrap_or_else(|| ACCOUNT_SOURCE.to_string());
    let nodes = fetch_nodes(&state, Some(&source_id)).await?;
    let node = nodes
        .into_iter()
        .find(|n| n.remark() == node_remark)
        .ok_or_else(|| CommandError::Other(format!("узел \"{node_remark}\" не найден")))?;

    // Смена узла «на лету»: сначала гасим прежний туннель, иначе порты заняты.
    disconnect_now(&state)?;
    connect_to_node(&app, &state, &node, &source_id).await?;
    Ok(())
}

#[tauri::command]
pub async fn disconnect(state: State<'_, AppState>) -> Result<(), CommandError> {
    disconnect_now(&state)?;
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct ConnectionStatus {
    pub connected: bool,
    pub node_remark: Option<String>,
    pub source_id: Option<String>,
    /// Подключение идёт в авто-режиме.
    pub auto: bool,
}

#[tauri::command]
pub async fn connection_status(
    state: State<'_, AppState>,
) -> Result<ConnectionStatus, CommandError> {
    let mut connection = state.connection.lock().unwrap();

    // Процесс мог упасть сам по себе (краш xray) — не будем врать интерфейсу,
    // что мы всё ещё подключены.
    let still_running = connection
        .process
        .as_mut()
        .map(|p| p.is_running())
        .unwrap_or(false);

    if !still_running && connection.process.is_some() {
        connection.process = None;
        connection.connected_node = None;
        connection.connected_source = None;
        connection.auto = false;
        drop(connection);
        super::finish_block_stats(&state);
        // Процесс умер сам — прокси за ним не убрать некому, кроме нас.
        let _ = zexor_vpn_core::proxy::restore();
        return Ok(ConnectionStatus {
            connected: false,
            node_remark: None,
            source_id: None,
            auto: false,
        });
    }

    Ok(ConnectionStatus {
        connected: still_running,
        node_remark: connection.connected_node.clone(),
        source_id: connection.connected_source.clone(),
        auto: connection.auto,
    })
}

#[derive(Debug, Serialize)]
pub struct SourceSummary {
    pub id: String,
    pub name: String,
    /// Подписку аккаунта удалить нельзя.
    pub removable: bool,
}

#[tauri::command]
pub fn list_sources() -> Vec<SourceSummary> {
    let mut list = vec![SourceSummary {
        id: ACCOUNT_SOURCE.to_string(),
        name: "Zexor".to_string(),
        removable: false,
    }];
    list.extend(sources::load().into_iter().map(|s| SourceSummary {
        id: s.id,
        name: s.name,
        removable: true,
    }));
    list
}

/// Добавляет чужую подписку. Перед сохранением ссылка реально скачивается и
/// разбирается: «плюс» с мёртвой или пустой ссылкой только засорил бы список.
#[tauri::command]
pub async fn add_source(name: String, url: String) -> Result<SourceSummary, CommandError> {
    let url = sources::validate_url(&url).map_err(CommandError::Other)?;
    let body = download_subscription(&url).await?;
    let nodes = zexor_vpn_core::parse_nodes(&body).map_err(ConnectError::from)?;

    let name = {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            format!("Подписка ({} серв.)", nodes.len())
        } else {
            trimmed.chars().take(40).collect()
        }
    };

    let mut all = sources::load();
    if all.iter().any(|s| s.url == url) {
        return Err(CommandError::Other(
            "эта подписка уже добавлена".to_string(),
        ));
    }
    let source = Source {
        id: sources::new_id(),
        name,
        url,
    };
    all.push(source.clone());
    sources::save(&all).map_err(|e| CommandError::Other(e.to_string()))?;

    Ok(SourceSummary {
        id: source.id,
        name: source.name,
        removable: true,
    })
}

#[tauri::command]
pub fn remove_source(state: State<'_, AppState>, id: String) -> Result<(), CommandError> {
    if id == ACCOUNT_SOURCE {
        return Err(CommandError::Other(
            "подписку аккаунта удалить нельзя".to_string(),
        ));
    }
    let connected_here = state.connection.lock().unwrap().connected_source.as_deref() == Some(&id);
    if connected_here {
        disconnect_now(&state)?;
    }
    let mut all = sources::load();
    all.retain(|s| s.id != id);
    sources::save(&all).map_err(|e| CommandError::Other(e.to_string()))
}

#[derive(Debug, Serialize)]
pub struct NodePing {
    pub remark: String,
    /// Задержка в мс; `None` — сервер не отвечает.
    pub ms: Option<u32>,
}

/// Быстрый TCP-пинг всех серверов подписки (параллельно). Показывает, что хост жив и как далеко.
#[tauri::command]
pub async fn ping_nodes(
    app: AppHandle,
    state: State<'_, AppState>,
    source_id: Option<String>,
) -> Result<Vec<NodePing>, CommandError> {
    let nodes = fetch_nodes(&state, source_id.as_deref()).await?;
    let nodes: Vec<_> = nodes.into_iter().filter(|n| !n.is_balanced()).collect();
    let pings = probe::tcp_ping_many(&nodes, Duration::from_secs(3)).await;
    // Замеры серверов нашей подписки уходят на сервер (анонимно, можно отключить в профиле).
    if source_id.is_none() || source_id.as_deref() == Some(ACCOUNT_SOURCE) {
        crate::sync::report_pings(
            &app,
            nodes
                .iter()
                .zip(&pings)
                .map(|(node, ms)| (node.remark().trim().to_string(), *ms))
                .collect(),
        );
    }
    Ok(nodes
        .iter()
        .zip(pings)
        .map(|(node, ms)| NodePing {
            remark: node.remark().to_string(),
            ms,
        })
        .collect())
}

#[derive(Debug, Serialize)]
pub struct NodeTest {
    pub ok: bool,
    pub ms: Option<u32>,
    pub error: Option<String>,
}

/// Реальная проверка одного сервера: отдельный xray, запрос через туннель, время ответа.
/// Только она отвечает, работает ли сервер на самом деле (TCP может проходить, а рукопожатие — нет).
#[tauri::command]
pub async fn test_node(
    app: AppHandle,
    state: State<'_, AppState>,
    node_remark: String,
    source_id: Option<String>,
) -> Result<NodeTest, CommandError> {
    let nodes = fetch_nodes(&state, source_id.as_deref()).await?;
    let node = nodes
        .into_iter()
        .find(|n| n.remark() == node_remark)
        .ok_or_else(|| CommandError::Other(format!("узел \"{node_remark}\" не найден")))?;
    let binary = resolve_xray_binary(&app)?;

    Ok(match probe::real_delay(&binary, &node).await {
        Ok(ms) => NodeTest {
            ok: true,
            ms: Some(ms),
            error: None,
        },
        Err(err) => NodeTest {
            ok: false,
            ms: None,
            error: Some(err.to_string()),
        },
    })
}

#[derive(Debug, Serialize)]
pub struct AutoResult {
    /// К чему подключились: «AUTO» (балансировщик) или имя выбранного сервера.
    pub remark: String,
    /// Задержка выбранного сервера; у балансировщика не определена (он выбирает постоянно).
    pub ms: Option<u32>,
}

/// Авто-режим (см. [`connect_auto`]): на аккаунте Zexor работает балансировщик, на чужой подписке
/// выбирается самый быстрый сервер.
#[tauri::command]
pub async fn auto_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    source_id: Option<String>,
) -> Result<AutoResult, CommandError> {
    let source_id = source_id.unwrap_or_else(|| ACCOUNT_SOURCE.to_string());
    let nodes = fetch_nodes(&state, Some(&source_id)).await?;

    disconnect_now(&state)?;
    let outcome = connect_auto(&app, &state, &nodes, &source_id).await?;
    Ok(AutoResult {
        remark: outcome.label,
        ms: outcome.ms,
    })
}

/// Android: причина прошлого вылета приложения (пусто, если его не было). На других платформах всегда пусто.
#[tauri::command]
pub async fn crash_report(app: AppHandle) -> String {
    #[cfg(target_os = "android")]
    {
        use tauri::Manager;
        if let Some(vpn) = app.try_state::<tauri_plugin_zexor_vpn::Vpn<tauri::Wry>>() {
            return vpn.crashes().await.unwrap_or_default();
        }
    }
    let _ = app;
    String::new()
}
