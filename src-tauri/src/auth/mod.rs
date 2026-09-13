//! Оркестрация логина/сессии поверх `zexor-vpn-core`.
//!
//! Access-токен живёт только в памяти (`AppState::session`), refresh — в
//! Windows Credential Manager. При холодном старте в памяти токенов нет
//! вообще: `ensure_valid_access_token` подставляет пустой access (что даёт
//! `TokenAction::Refresh`) и обновляется по refresh'у из хранилища.

pub mod commands;

use std::time::{SystemTime, UNIX_EPOCH};

use zexor_vpn_core::{ApiError, TokenAction, TokenSet};

use crate::state::{AppState, CREDENTIAL_SERVICE, CREDENTIAL_USER};

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("нужно войти в аккаунт")]
    NeedsLogin,
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("не удалось обратиться к хранилищу учётных данных: {0}")]
    Credential(String),
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn credential_entry() -> Result<keyring::Entry, AuthError> {
    keyring::Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_USER)
        .map_err(|e| AuthError::Credential(e.to_string()))
}

/// Сохраняет refresh-токен в системном хранилище. Вызывается после каждого
/// успешного логина/обновления — сервер отзывает предыдущий refresh сразу.
pub fn persist_refresh_token(refresh_token: &str) -> Result<(), AuthError> {
    credential_entry()?
        .set_password(refresh_token)
        .map_err(|e| AuthError::Credential(e.to_string()))
}

fn load_refresh_token() -> Option<String> {
    credential_entry().ok()?.get_password().ok()
}

pub fn clear_stored_session() {
    if let Ok(entry) = credential_entry() {
        let _ = entry.delete_credential();
    }
}

/// Возвращает годный к использованию access-токен, обновляя его по необходимости.
///
/// Лок над `state.session` не держится во время сетевых вызовов: `MutexGuard`
/// не `Send`, а команды Tauri исполняются на многопоточном рантайме — держать
/// его через `.await` попросту не скомпилируется (и было бы гонкой при
/// параллельных запросах, если бы скомпилировалось).
pub async fn ensure_valid_access_token(state: &AppState) -> Result<String, AuthError> {
    let current = {
        let session = state.session.lock().unwrap();
        session.tokens.clone()
    };

    let tokens = match current {
        Some(tokens) => tokens,
        None => {
            let refresh_token = load_refresh_token().ok_or(AuthError::NeedsLogin)?;
            TokenSet {
                access_token: String::new(),
                refresh_token,
            }
        }
    };

    match tokens.action_at(now_unix()) {
        TokenAction::UseCurrent => Ok(tokens.access_token),
        TokenAction::Refresh => {
            let refreshed = state.api.refresh(&tokens.refresh_token).await?;

            // Порядок важен: сначала сохраняем НОВЫЙ refresh на диск, потом в
            // памяти — если процесс упадёt между этими шагами, при следующем
            // старте мы всё ещё найдём рабочий (уже сохранённый) токен.
            persist_refresh_token(&refreshed.refresh_token)?;

            let new_tokens = TokenSet {
                access_token: refreshed.access_token.clone(),
                refresh_token: refreshed.refresh_token,
            };
            state.session.lock().unwrap().tokens = Some(new_tokens);

            Ok(refreshed.access_token)
        }
        TokenAction::Reauthenticate => {
            clear_stored_session();
            state.session.lock().unwrap().tokens = None;
            Err(AuthError::NeedsLogin)
        }
    }
}
