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
    #[error("не удалось открыть браузер: {0}")]
    Browser(String),
    #[error("время ожидания входа истекло — попробуйте ещё раз")]
    LoginExpired,
}

/// Открывает ссылку в браузере по умолчанию. На Windows через
/// `rundll32 url.dll,FileProtocolHandler` — ему URL передаётся одним аргументом
/// без участия командной строки, поэтому `&` в query-строке ничего не ломает
/// (в отличие от `cmd /C start`).
pub fn open_in_browser(url: &str) -> Result<(), AuthError> {
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_opener::OpenerExt;
        let app = ANDROID_APP
            .get()
            .ok_or_else(|| AuthError::Browser("приложение ещё не готово".to_string()))?;
        return app
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|e| AuthError::Browser(e.to_string()));
    }

    #[cfg(not(target_os = "android"))]
    {
        #[cfg(windows)]
        let spawned = std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn();
        #[cfg(not(windows))]
        let spawned = std::process::Command::new("xdg-open").arg(url).spawn();

        spawned
            .map(|_| ())
            .map_err(|e| AuthError::Browser(e.to_string()))
    }
}

/// Дескриптор приложения для открытия ссылок на Android (там это делает плагин, а не внешняя команда).
#[cfg(target_os = "android")]
static ANDROID_APP: std::sync::OnceLock<tauri::AppHandle> = std::sync::OnceLock::new();

#[cfg(target_os = "android")]
pub fn set_android_app(handle: tauri::AppHandle) {
    let _ = ANDROID_APP.set(handle);
}

/// 32 случайных байта в hex (64 символа) — `state` браузерного входа. Сервер
/// требует минимум 16 символов; такой длины достаточно, чтобы его нельзя было
/// угадать и перехватить чужие токены.
pub fn random_state() -> Result<String, AuthError> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes).map_err(|e| AuthError::Browser(e.to_string()))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// Refresh-токен: в Windows — Credential Manager, на остальных платформах (Android) — файл в закрытой
// папке приложения (она недоступна другим приложениям).
#[cfg(windows)]
fn credential_entry() -> Result<keyring::Entry, AuthError> {
    keyring::Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_USER)
        .map_err(|e| AuthError::Credential(e.to_string()))
}

#[cfg(not(windows))]
fn token_file() -> std::path::PathBuf {
    zexor_vpn_core::xray::process::app_data_dir()
        .join("ZexorVPN")
        .join("refresh_token")
}

/// Сохраняет refresh-токен в системном хранилище. Вызывается после каждого
/// успешного логина/обновления — сервер отзывает предыдущий refresh сразу.
#[cfg(windows)]
pub fn persist_refresh_token(refresh_token: &str) -> Result<(), AuthError> {
    credential_entry()?
        .set_password(refresh_token)
        .map_err(|e| AuthError::Credential(e.to_string()))
}

#[cfg(not(windows))]
pub fn persist_refresh_token(refresh_token: &str) -> Result<(), AuthError> {
    let path = token_file();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| AuthError::Credential(e.to_string()))?;
    }
    std::fs::write(&path, refresh_token).map_err(|e| AuthError::Credential(e.to_string()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

#[cfg(windows)]
fn load_refresh_token() -> Option<String> {
    credential_entry().ok()?.get_password().ok()
}

#[cfg(not(windows))]
fn load_refresh_token() -> Option<String> {
    std::fs::read_to_string(token_file())
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

#[cfg(windows)]
pub fn clear_stored_session() {
    if let Ok(entry) = credential_entry() {
        let _ = entry.delete_credential();
    }
}

#[cfg(not(windows))]
pub fn clear_stored_session() {
    let _ = std::fs::remove_file(token_file());
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
