//! Tauri-команды логина, вызываемые фронтендом через `invoke`.

use serde::Serialize;
use tauri::State;
use zexor_vpn_core::{ApiError, LoginPoll, PairPoll, TokenSet};

use super::{
    clear_stored_session, ensure_valid_access_token, open_in_browser, persist_refresh_token,
    random_state,
};
use crate::state::{AppState, WEB_LOGIN_BASE_URL};

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

fn api_error(err: ApiError) -> CommandError {
    CommandError::from(super::AuthError::Api(err))
}

/// Общий финал любого способа входа: refresh на диск (раньше, чем в память —
/// см. порядок в `ensure_valid_access_token`), затем пара токенов в сессию.
fn store_session(
    state: &AppState,
    access_token: String,
    refresh_token: String,
) -> Result<(), CommandError> {
    persist_refresh_token(&refresh_token).map_err(CommandError::from)?;
    state.session.lock().unwrap().tokens = Some(TokenSet {
        access_token,
        refresh_token,
    });
    Ok(())
}

#[derive(Debug, Serialize)]
pub struct TelegramLoginStart {
    pub token: String,
}

/// Вход через Telegram: получаем одноразовый токен и открываем бота с
/// `/start webauth_<token>`; дальше фронтенд поллит `poll_telegram_login`.
#[tauri::command]
pub async fn start_telegram_login(
    state: State<'_, AppState>,
) -> Result<TelegramLoginStart, CommandError> {
    let link = state.api.deeplink_request().await.map_err(api_error)?;
    let url = format!(
        "https://t.me/{}?start=webauth_{}",
        link.bot_username, link.token
    );
    open_in_browser(&url).map_err(CommandError::from)?;
    Ok(TelegramLoginStart { token: link.token })
}

/// `Ok(None)` — пользователь ещё не подтвердил вход в боте.
#[tauri::command]
pub async fn poll_telegram_login(
    state: State<'_, AppState>,
    token: String,
) -> Result<Option<SessionInfo>, CommandError> {
    match state.api.deeplink_poll(&token).await.map_err(api_error)? {
        LoginPoll::Pending => Ok(None),
        LoginPoll::Expired => Err(CommandError::from(super::AuthError::LoginExpired)),
        LoginPoll::Completed(response) => {
            store_session(&state, response.access_token, response.refresh_token)?;
            Ok(Some(SessionInfo {
                email: response.user.email,
            }))
        }
    }
}

/// Вход через веб-страницу кабинета (Google и любой другой способ): открываем
/// `/adblock/connect?state=…&source=desktop[&provider=google]` и возвращаем
/// `state`, по которому фронтенд поллит `poll_browser_login`.
#[tauri::command]
pub async fn start_browser_login(provider: Option<String>) -> Result<String, CommandError> {
    let pair_state = random_state().map_err(CommandError::from)?;
    let mut url = format!("{WEB_LOGIN_BASE_URL}/adblock/connect?state={pair_state}&source=desktop");
    if let Some(provider) = provider {
        // Только короткие латинские имена провайдеров — в URL больше ничего не подставляем.
        if !provider.is_empty()
            && provider.len() <= 32
            && provider
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            url.push_str(&format!("&provider={provider}"));
        }
    }
    open_in_browser(&url).map_err(CommandError::from)?;
    Ok(pair_state)
}

#[tauri::command]
pub async fn poll_browser_login(
    state: State<'_, AppState>,
    pair_state: String,
) -> Result<Option<SessionInfo>, CommandError> {
    match state
        .api
        .pair_status(&pair_state)
        .await
        .map_err(api_error)?
    {
        PairPoll::Pending => Ok(None),
        PairPoll::Expired => Err(CommandError::from(super::AuthError::LoginExpired)),
        PairPoll::Completed {
            access_token,
            refresh_token,
        } => {
            store_session(&state, access_token, refresh_token)?;
            Ok(Some(SessionInfo { email: None }))
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
