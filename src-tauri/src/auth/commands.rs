//! Tauri-команды логина, вызываемые фронтендом через `invoke`.

use serde::Serialize;
use tauri::State;
use zexor_vpn_core::TokenSet;

use super::{clear_stored_session, ensure_valid_access_token, persist_refresh_token};
use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct SessionInfo {
    pub email: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum CommandError {
    NeedsLogin(String),
    InvalidCredentials(String),
    Network(String),
    Other(String),
}

impl From<super::AuthError> for CommandError {
    fn from(err: super::AuthError) -> Self {
        match err {
            super::AuthError::NeedsLogin => CommandError::NeedsLogin(err.to_string()),
            super::AuthError::Api(zexor_vpn_core::ApiError::InvalidCredentials) => {
                CommandError::InvalidCredentials(err.to_string())
            }
            super::AuthError::Api(zexor_vpn_core::ApiError::Network(_)) => {
                CommandError::Network(err.to_string())
            }
            other => CommandError::Other(other.to_string()),
        }
    }
}

#[tauri::command]
pub async fn login(
    state: State<'_, AppState>,
    email: String,
    password: String,
) -> Result<SessionInfo, CommandError> {
    let response = state
        .api
        .login(&email, &password)
        .await
        .map_err(|e| CommandError::from(super::AuthError::Api(e)))?;

    persist_refresh_token(&response.refresh_token).map_err(CommandError::from)?;

    let mut session = state.session.lock().unwrap();
    session.tokens = Some(TokenSet {
        access_token: response.access_token,
        refresh_token: response.refresh_token,
    });

    Ok(SessionInfo {
        email: response.user.email,
    })
}

#[tauri::command]
pub async fn logout(state: State<'_, AppState>) -> Result<(), CommandError> {
    // Выход должен гарантированно останавливать туннель — иначе пользователь
    // разлогинился в UI, а трафик всё ещё идёт через чей-то чужой сервер.
    state.shutdown_blocking();

    clear_stored_session();
    state.session.lock().unwrap().tokens = None;
    Ok(())
}

/// Вызывается при старте приложения: молча проверяет/обновляет токен и
/// сообщает, показывать ли экран логина или сразу дашборд.
#[tauri::command]
pub async fn current_session(
    state: State<'_, AppState>,
) -> Result<Option<SessionInfo>, CommandError> {
    match ensure_valid_access_token(&state).await {
        Ok(_access_token) => Ok(Some(SessionInfo { email: None })),
        Err(super::AuthError::NeedsLogin) => Ok(None),
        Err(other) => Err(other.into()),
    }
}
