//! Tauri-команды подключения, вызываемые фронтендом.

use serde::Serialize;
use tauri::{AppHandle, State};

use super::{connect_to_node, disconnect_now, fetch_nodes, ConnectError};
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum CommandError {
    NeedsLogin(String),
    NoSubscription(String),
    Other(String),
}

impl From<ConnectError> for CommandError {
    fn from(err: ConnectError) -> Self {
        match err {
            ConnectError::Auth(crate::auth::AuthError::NeedsLogin) => {
                CommandError::NeedsLogin(err.to_string())
            }
            ConnectError::NoSubscription => CommandError::NoSubscription(err.to_string()),
            other => CommandError::Other(other.to_string()),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct NodeSummary {
    pub remark: String,
    pub is_reality: bool,
}

#[derive(Debug, Serialize)]
pub struct NodesResponse {
    pub nodes: Vec<NodeSummary>,
}

#[tauri::command]
pub async fn list_nodes(state: State<'_, AppState>) -> Result<NodesResponse, CommandError> {
    let nodes = fetch_nodes(&state).await?;
    Ok(NodesResponse {
        nodes: nodes
            .iter()
            .map(|n| NodeSummary {
                remark: n.remark.clone(),
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
) -> Result<(), CommandError> {
    let nodes = fetch_nodes(&state).await?;
    let node = nodes
        .into_iter()
        .find(|n| n.remark == node_remark)
        .ok_or_else(|| CommandError::Other(format!("узел \"{node_remark}\" не найден")))?;

    connect_to_node(&app, &state, &node).await?;
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
        drop(connection);
        // Процесс умер сам — прокси за ним не убрать некому, кроме нас.
        let _ = zexor_vpn_core::proxy::restore();
        return Ok(ConnectionStatus {
            connected: false,
            node_remark: None,
        });
    }

    Ok(ConnectionStatus {
        connected: still_running,
        node_remark: connection.connected_node.clone(),
    })
}
