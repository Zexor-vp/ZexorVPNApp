//! Универсальный проход к Cabinet API для страниц приложения (тариф, профиль, рефералы).
//!
//! Фронтенд не видит токенов: он просит «GET /api/cabinet/...», а токен подставляется здесь,
//! в Rust, с обновлением по refresh при необходимости. Допустимость пути проверяет ядро
//! (`ApiClient::raw_request`) — токен нельзя отправить на посторонний адрес.

use serde::Serialize;
use serde_json::Value;
use tauri::State;
use zexor_vpn_core::ApiError;

use crate::auth::ensure_valid_access_token;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum CabinetError {
    NeedsLogin(String),
    Network(String),
    Other(String),
}

#[tauri::command]
pub async fn cabinet_request(
    state: State<'_, AppState>,
    method: String,
    path: String,
    body: Option<Value>,
) -> Result<Value, CabinetError> {
    let token = ensure_valid_access_token(&state)
        .await
        .map_err(|e| match e {
            crate::auth::AuthError::NeedsLogin => CabinetError::NeedsLogin(e.to_string()),
            crate::auth::AuthError::Api(ApiError::Network(_)) => {
                CabinetError::Network(e.to_string())
            }
            other => CabinetError::Other(other.to_string()),
        })?;

    state
        .api
        .raw_request(&method, &path, &token, body)
        .await
        .map_err(|e| match e {
            ApiError::Unauthorized => {
                CabinetError::NeedsLogin("сессия истекла — войдите снова".to_string())
            }
            ApiError::Network(_) => CabinetError::Network(e.to_string()),
            ApiError::Server { message, .. } => CabinetError::Other(message),
            other => CabinetError::Other(other.to_string()),
        })
}

/// Загружает фото для обращения в поддержку: фронтенд присылает картинку в base64, токен подставляется здесь.
/// Возвращает `{ file_id, media_type, ... }` — `file_id` затем уходит вместе с текстом сообщения.
#[tauri::command]
pub async fn upload_support_photo(
    state: State<'_, AppState>,
    mime: String,
    data_base64: String,
) -> Result<Value, CabinetError> {
    use base64::Engine;

    if !matches!(
        mime.as_str(),
        "image/jpeg" | "image/png" | "image/webp" | "image/gif"
    ) {
        return Err(CabinetError::Other(
            "поддерживаются только фото JPEG, PNG, WebP и GIF".to_string(),
        ));
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|_| CabinetError::Other("не удалось прочитать файл".to_string()))?;
    if bytes.is_empty() || bytes.len() > 9 * 1024 * 1024 {
        return Err(CabinetError::Other(
            "файл слишком большой (максимум 9 МБ)".to_string(),
        ));
    }
    let token = ensure_valid_access_token(&state)
        .await
        .map_err(|e| match e {
            crate::auth::AuthError::NeedsLogin => CabinetError::NeedsLogin(e.to_string()),
            crate::auth::AuthError::Api(ApiError::Network(_)) => {
                CabinetError::Network(e.to_string())
            }
            other => CabinetError::Other(other.to_string()),
        })?;
    state
        .api
        .upload_photo(&token, &mime, bytes)
        .await
        .map_err(|e| match e {
            ApiError::Unauthorized => {
                CabinetError::NeedsLogin("сессия истекла — войдите снова".to_string())
            }
            ApiError::Network(_) => CabinetError::Network(e.to_string()),
            ApiError::Server { message, .. } => CabinetError::Other(message),
            other => CabinetError::Other(other.to_string()),
        })
}

/// Открывает страницу кабинета/Telegram в браузере (пополнение баланса, поддержка и т.п.).
#[tauri::command]
pub fn open_external(url: String) -> Result<(), CabinetError> {
    if !zexor_vpn_core::links::is_allowed_external_url(&url) {
        return Err(CabinetError::Other("эту ссылку открыть нельзя".to_string()));
    }
    crate::auth::open_in_browser(&url).map_err(|e| CabinetError::Other(e.to_string()))
}

/// Открывает страницу оплаты, которую вернул бэкенд (`payment_url`), в браузере по умолчанию.
#[tauri::command]
pub fn open_payment_url(url: String) -> Result<(), CabinetError> {
    if !zexor_vpn_core::links::is_safe_https_url(&url) {
        return Err(CabinetError::Other(
            "ссылка на оплату выглядит небезопасной".to_string(),
        ));
    }
    crate::auth::open_in_browser(&url).map_err(|e| CabinetError::Other(e.to_string()))
}
