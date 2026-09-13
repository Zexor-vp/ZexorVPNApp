//! Tauri-команды блокировщика рекламы.

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::state::AppState;
use crate::xray::{connect_to_node, disconnect_now, fetch_nodes};

#[derive(Debug, Serialize)]
pub struct AdblockState {
    pub enabled: bool,
}

#[tauri::command]
pub fn adblock_settings(state: State<'_, AppState>) -> AdblockState {
    AdblockState {
        enabled: state.adblock.lock().unwrap().enabled,
    }
}

/// Меняет тумблер и, если сейчас есть активное подключение, сразу
/// переподключается к тому же узлу — иначе новое правило применится только
/// после ручного переподключения, а пользователь ожидает мгновенного эффекта.
#[tauri::command]
pub async fn set_adblock_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), String> {
    state.adblock.lock().unwrap().enabled = enabled;

    let currently_connected_to = state.connection.lock().unwrap().connected_node.clone();
    let Some(remark) = currently_connected_to else {
        return Ok(());
    };

    let nodes = fetch_nodes(&state).await.map_err(|e| e.to_string())?;
    let Some(node) = nodes.into_iter().find(|n| n.remark == remark) else {
        // Подписка успела смениться и узла больше нет — просто отключаемся,
        // это лучше, чем оставить туннель в рассинхроне с настройками.
        disconnect_now(&state).map_err(|e| e.to_string())?;
        return Ok(());
    };

    disconnect_now(&state).map_err(|e| e.to_string())?;
    connect_to_node(&app, &state, &node)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
