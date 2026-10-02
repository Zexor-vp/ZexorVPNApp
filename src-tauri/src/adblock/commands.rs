//! Tauri-команды блокировщика рекламы.

use serde::Serialize;
use tauri::{AppHandle, State};
use zexor_vpn_core::adblock::settings::{self, AdblockConfig, ListKind, REMOTE_LIST_URL};

use crate::state::AppState;
use crate::xray::{download_subscription, load_bundled_blocklist, reconnect_if_connected};

/// Состояние для страницы адблока: настройки плюс размеры списков.
#[derive(Debug, Serialize)]
pub struct AdblockView {
    pub enabled: bool,
    pub use_remote: bool,
    pub custom_block: Vec<String>,
    pub custom_allow: Vec<String>,
    pub bundled_count: usize,
    pub remote_count: usize,
    pub remote_updated_at: Option<i64>,
    /// Сколько доменов реально блокируется прямо сейчас (с учётом тумблеров).
    pub active_count: usize,
}

fn view(app: &AppHandle, config: &AdblockConfig) -> AdblockView {
    let bundled = load_bundled_blocklist(app);
    AdblockView {
        enabled: config.enabled,
        use_remote: config.use_remote,
        custom_block: config.custom_block.clone(),
        custom_allow: config.custom_allow.clone(),
        bundled_count: bundled.len(),
        remote_count: config.remote_domains.len(),
        remote_updated_at: config.remote_updated_at,
        active_count: config.effective_block(&bundled).len(),
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Сохраняет изменённые настройки и применяет их к работающему туннелю.
async fn commit(
    app: &AppHandle,
    state: &AppState,
    mutate: impl FnOnce(&mut AdblockConfig) -> Result<(), String>,
) -> Result<AdblockView, String> {
    let snapshot = {
        let mut config = state.adblock.lock().unwrap();
        mutate(&mut config)?;
        config.clone()
    };
    settings::save(&snapshot).map_err(|e| format!("не удалось сохранить настройки: {e}"))?;
    reconnect_if_connected(app, state)
        .await
        .map_err(|e| e.to_string())?;
    Ok(view(app, &snapshot))
}

#[tauri::command]
pub fn adblock_settings(app: AppHandle, state: State<'_, AppState>) -> AdblockView {
    view(&app, &state.adblock.lock().unwrap())
}

#[tauri::command]
pub async fn set_adblock_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<AdblockView, String> {
    commit(&app, &state, |c| {
        c.enabled = enabled;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn set_adblock_remote(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<AdblockView, String> {
    commit(&app, &state, |c| {
        c.use_remote = enabled;
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn add_adblock_domain(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: String,
    domain: String,
) -> Result<AdblockView, String> {
    let kind = ListKind::parse(&kind).ok_or("неизвестный список")?;
    commit(&app, &state, |c| c.add_domain(kind, &domain).map(|_| ())).await
}

#[tauri::command]
pub async fn remove_adblock_domain(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: String,
    domain: String,
) -> Result<AdblockView, String> {
    let kind = ListKind::parse(&kind).ok_or("неизвестный список")?;
    commit(&app, &state, |c| {
        c.remove_domain(kind, &domain);
        Ok(())
    })
    .await
}

/// Скачивает дополнительный список Zexor. При ошибке сети прежняя копия остаётся нетронутой.
pub async fn refresh_remote(app: &AppHandle, state: &AppState) -> Result<AdblockView, String> {
    let body = download_subscription(REMOTE_LIST_URL)
        .await
        .map_err(|e| e.to_string())?;
    let domains = zexor_vpn_core::adblock::parse_blocklist(&body);
    if domains.is_empty() {
        return Err("список пуст или в неизвестном формате".to_string());
    }
    commit(app, state, |c| {
        c.set_remote(domains, now_unix());
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn refresh_adblock_remote(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<AdblockView, String> {
    refresh_remote(&app, &state).await
}

/// Фоновое обновление при запуске: раз в сутки, без шума при ошибках.
pub async fn refresh_remote_if_stale(app: AppHandle, state: &AppState) {
    let stale = state.adblock.lock().unwrap().needs_refresh(now_unix());
    if stale {
        if let Err(err) = refresh_remote(&app, state).await {
            tracing::warn!(%err, "не удалось обновить дополнительный список адблока");
        }
    }
}

/// Счётчик заблокированной рекламы с запуска приложения.
#[derive(Debug, Serialize)]
pub struct SessionStats {
    pub blocked: u64,
    /// Идёт ли подсчёт прямо сейчас (VPN подключён).
    pub active: bool,
}

#[tauri::command]
pub fn adblock_session_stats(state: State<'_, AppState>) -> SessionStats {
    let mut stats = state.block_stats.lock().unwrap();
    let live = stats.counter.as_mut().map(|c| c.poll()).unwrap_or(0);
    SessionStats {
        blocked: stats.accumulated + live,
        active: stats.counter.is_some(),
    }
}

#[tauri::command]
pub fn reset_adblock_session_stats(state: State<'_, AppState>) -> SessionStats {
    let mut stats = state.block_stats.lock().unwrap();
    stats.accumulated = 0;
    if let Some(counter) = stats.counter.as_mut() {
        counter.reset();
    }
    SessionStats {
        blocked: 0,
        active: stats.counter.is_some(),
    }
}
