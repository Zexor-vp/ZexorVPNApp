//! Работа с токенами кабинета.
//!
//! Бэкенд выдаёт короткоживущий access (15 мин) и refresh на 7 дней, причём
//! **refresh ротируется при каждом обновлении**: старый сразу отзывается. Отсюда
//! два жёстких правила, которые и кодирует этот модуль:
//!   1. новый refresh сохраняем ДО следующего запроса, иначе теряем сессию;
//!   2. обновление должно быть однопоточным — два параллельных refresh'а
//!      означают, что второй придёт с уже отозванным токеном и разлогинит юзера.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// За сколько секунд до истечения считаем access-токен «уже протухшим».
/// Запас нужен, чтобы токен не истёк в полёте между проверкой и ответом сервера.
pub const EXPIRY_SAFETY_MARGIN_SECS: i64 = 30;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum TokenError {
    #[error("JWT имеет неверный формат")]
    MalformedJwt,
    #[error("в JWT нет поля exp")]
    MissingExpiry,
}

/// Пара токенов в том виде, в каком её отдаёт `/cabinet/auth/email/login`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: String,
}

/// Что делать перед очередным запросом к API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenAction {
    /// Токен ещё годен — идём с ним.
    UseCurrent,
    /// Пора обновиться (истёк или вот-вот истечёт).
    Refresh,
    /// Refresh тоже мёртв — только повторный логин.
    Reauthenticate,
}

impl TokenSet {
    /// Решает, что делать с текущей парой токенов на момент времени `now`.
    pub fn action_at(&self, now_unix: i64) -> TokenAction {
        // Протухший refresh лечится только новым логином, проверяем его первым.
        match jwt_expiry(&self.refresh_token) {
            Ok(exp) if exp <= now_unix => return TokenAction::Reauthenticate,
            // Нечитаемый refresh считаем негодным: лучше попросить логин,
            // чем молча уйти в бесконечные 401.
            Err(_) => return TokenAction::Reauthenticate,
            _ => {}
        }

        match jwt_expiry(&self.access_token) {
            Ok(exp) if exp - EXPIRY_SAFETY_MARGIN_SECS > now_unix => TokenAction::UseCurrent,
            // И протухший, и битый access — повод сходить за новым по refresh.
            _ => TokenAction::Refresh,
        }
    }
}

/// Достаёт `exp` из payload'а JWT без проверки подписи.
///
/// Подпись проверяет сервер; клиенту нужно лишь понимать, когда пора
/// обновляться, поэтому валидация ключа здесь была бы лишней сложностью.
pub fn jwt_expiry(token: &str) -> Result<i64, TokenError> {
    let payload_b64 = token.split('.').nth(1).ok_or(TokenError::MalformedJwt)?;

    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|_| TokenError::MalformedJwt)?;
    let payload: serde_json::Value =
        serde_json::from_slice(&payload_bytes).map_err(|_| TokenError::MalformedJwt)?;

    payload
        .get("exp")
        .and_then(|v| v.as_i64())
        .ok_or(TokenError::MissingExpiry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;

    /// Собирает JWT-подобную строку с нужным exp (подпись роли не играет).
    fn jwt_with_exp(exp: i64) -> String {
        let payload = URL_SAFE_NO_PAD.encode(format!(r#"{{"sub":"842","exp":{exp}}}"#));
        format!("header.{payload}.signature")
    }

    const NOW: i64 = 1_800_000_000;

    #[test]
    fn reads_expiry_from_jwt() {
        assert_eq!(jwt_expiry(&jwt_with_exp(NOW)).unwrap(), NOW);
    }

    #[test]
    fn rejects_malformed_jwt() {
        assert_eq!(jwt_expiry("no-dots-here").unwrap_err(), TokenError::MalformedJwt);
        assert_eq!(jwt_expiry("a.!!!not-base64!!!.c").unwrap_err(), TokenError::MalformedJwt);
        let no_exp = URL_SAFE_NO_PAD.encode(r#"{"sub":"842"}"#);
        assert_eq!(
            jwt_expiry(&format!("h.{no_exp}.s")).unwrap_err(),
            TokenError::MissingExpiry
        );
    }

    #[test]
    fn uses_current_token_while_it_is_comfortably_valid() {
        let tokens = TokenSet {
            access_token: jwt_with_exp(NOW + 600),
            refresh_token: jwt_with_exp(NOW + 7 * 86400),
        };
        assert_eq!(tokens.action_at(NOW), TokenAction::UseCurrent);
    }

    #[test]
    fn refreshes_within_safety_margin_before_expiry() {
        // Токен формально ещё жив (10 секунд), но за время запроса истечёт.
        let tokens = TokenSet {
            access_token: jwt_with_exp(NOW + 10),
            refresh_token: jwt_with_exp(NOW + 7 * 86400),
        };
        assert_eq!(tokens.action_at(NOW), TokenAction::Refresh);
    }

    #[test]
    fn refreshes_expired_access_token() {
        let tokens = TokenSet {
            access_token: jwt_with_exp(NOW - 1),
            refresh_token: jwt_with_exp(NOW + 86400),
        };
        assert_eq!(tokens.action_at(NOW), TokenAction::Refresh);
    }

    #[test]
    fn requires_login_when_refresh_token_expired() {
        let tokens = TokenSet {
            access_token: jwt_with_exp(NOW - 100),
            refresh_token: jwt_with_exp(NOW - 1),
        };
        assert_eq!(tokens.action_at(NOW), TokenAction::Reauthenticate);
    }

    #[test]
    fn expired_refresh_wins_over_still_valid_access() {
        // Пограничный случай: access ещё жив, но refresh уже мёртв — сессию
        // всё равно не продлить, честнее сразу просить логин.
        let tokens = TokenSet {
            access_token: jwt_with_exp(NOW + 600),
            refresh_token: jwt_with_exp(NOW - 1),
        };
        assert_eq!(tokens.action_at(NOW), TokenAction::Reauthenticate);
    }

    #[test]
    fn unreadable_refresh_forces_login_rather_than_retry_loop() {
        let tokens = TokenSet {
            access_token: jwt_with_exp(NOW + 600),
            refresh_token: "garbage".into(),
        };
        assert_eq!(tokens.action_at(NOW), TokenAction::Reauthenticate);
    }

    #[test]
    fn unreadable_access_triggers_refresh_not_login() {
        let tokens = TokenSet {
            access_token: "garbage".into(),
            refresh_token: jwt_with_exp(NOW + 86400),
        };
        assert_eq!(tokens.action_at(NOW), TokenAction::Refresh);
    }
}
