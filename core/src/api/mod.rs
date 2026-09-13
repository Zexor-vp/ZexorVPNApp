//! Клиент к Cabinet API (тому же бэкенду, что обслуживает веб-кабинет).
//!
//! Три операции: логин по email/паролю, обновление токена (с учётом ротации —
//! сервер отзывает старый refresh при каждом использовании) и получение данных
//! подписки для дашборда.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ApiError {
    #[error("сеть недоступна или сервер не ответил: {0}")]
    Network(String),
    #[error("неверный email или пароль")]
    InvalidCredentials,
    #[error("сессия истекла, нужен повторный вход")]
    Unauthorized,
    #[error("слишком много попыток, попробуйте позже")]
    RateLimited,
    #[error("сервер вернул ошибку ({status}): {message}")]
    Server { status: u16, message: String },
    #[error("не удалось разобрать ответ сервера: {0}")]
    Decode(String),
}

impl From<reqwest::Error> for ApiError {
    fn from(err: reqwest::Error) -> Self {
        ApiError::Network(err.to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
    pub user: UserInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: i64,
    pub email: Option<String>,
    #[serde(default)]
    pub first_name: Option<String>,
}

/// Зеркалит `SubscriptionData` из `app/cabinet/schemas/subscription.py` — поля
/// нужны один в один, чтобы дашборд показывал то же, что и веб-кабинет.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionData {
    pub subscription_url: Option<String>,
    pub days_left: i64,
    #[serde(default)]
    pub hours_left: i64,
    #[serde(default)]
    pub minutes_left: i64,
    pub traffic_limit_gb: i64,
    pub traffic_used_gb: f64,
    pub device_limit: i64,
    #[serde(default)]
    pub connected_squads: Vec<String>,
    #[serde(default)]
    pub servers: Vec<ServerInfo>,
    pub tariff_name: Option<String>,
    pub is_active: bool,
    #[serde(default)]
    pub is_expired: bool,
    #[serde(default)]
    pub hide_subscription_link: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub uuid: String,
    pub name: String,
    pub country_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionStatusResponse {
    pub has_subscription: bool,
    pub subscription: Option<SubscriptionData>,
}

pub struct ApiClient {
    base_url: String,
    http: reqwest::Client,
}

impl ApiClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .expect("не удалось создать HTTP-клиент"),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    pub async fn login(&self, email: &str, password: &str) -> Result<LoginResponse, ApiError> {
        let response = self
            .http
            .post(self.url("/api/cabinet/auth/email/login"))
            .json(&serde_json::json!({ "email": email, "password": password }))
            .send()
            .await?;

        self.parse_or_error(response, |status, _| match status {
            401 => ApiError::InvalidCredentials,
            _ => ApiError::InvalidCredentials, // логин: любая 4xx трактуется как неверные данные
        })
        .await
    }

    /// Обновляет пару токенов. **Вызывающий обязан сохранить оба поля нового
    /// ответа** (в т.ч. новый `refresh_token`) до следующего запроса — старый
    /// refresh сервер отзывает сразу же.
    pub async fn refresh(&self, refresh_token: &str) -> Result<RefreshResponse, ApiError> {
        let response = self
            .http
            .post(self.url("/api/cabinet/auth/refresh"))
            .json(&serde_json::json!({ "refresh_token": refresh_token }))
            .send()
            .await?;

        self.parse_or_error(response, |_, _| ApiError::Unauthorized)
            .await
    }

    pub async fn subscription_info(
        &self,
        access_token: &str,
    ) -> Result<SubscriptionStatusResponse, ApiError> {
        let response = self
            .http
            .get(self.url("/api/cabinet/subscription/info"))
            .bearer_auth(access_token)
            .send()
            .await?;

        self.parse_or_error(response, |_, _| ApiError::Unauthorized)
            .await
    }

    /// Общая обработка ответа: 2xx → десериализуем; 401 → отдаём вызывающему
    /// решать (разный смысл для логина и для остальных запросов); 429 →
    /// `RateLimited`; всё прочее — `Server` с телом ответа, если получится его
    /// прочитать как текст.
    async fn parse_or_error<T: for<'de> Deserialize<'de>>(
        &self,
        response: reqwest::Response,
        on_unauthorized: impl FnOnce(u16, &str) -> ApiError,
    ) -> Result<T, ApiError> {
        let status = response.status();
        if status.is_success() {
            let text = response
                .text()
                .await
                .map_err(|e| ApiError::Decode(e.to_string()))?;
            return serde_json::from_str(&text).map_err(|e| ApiError::Decode(e.to_string()));
        }

        let body = response.text().await.unwrap_or_default();
        Err(match status.as_u16() {
            401 => on_unauthorized(401, &body),
            429 => ApiError::RateLimited,
            code => ApiError::Server {
                status: code,
                message: extract_detail(&body),
            },
        })
    }
}

/// Бэкенд отдаёт ошибки как `{"detail": "..."}` (FastAPI) — вытаскиваем
/// человекочитаемое сообщение, если оно есть, иначе отдаём тело как есть.
fn extract_detail(body: &str) -> String {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v.get("detail").and_then(|d| d.as_str()).map(str::to_string))
        .unwrap_or_else(|| body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn login_returns_tokens_on_success() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/api/cabinet/auth/email/login")
            .match_body(mockito::Matcher::Json(serde_json::json!({
                "email": "user@example.com",
                "password": "hunter2"
            })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"access_token":"a","refresh_token":"r","token_type":"bearer","expires_in":900,
                    "user":{"id":842,"email":"user@example.com"}}"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url());
        let result = client.login("user@example.com", "hunter2").await.unwrap();

        mock.assert_async().await;
        assert_eq!(result.access_token, "a");
        assert_eq!(result.refresh_token, "r");
        assert_eq!(result.user.id, 842);
    }

    #[tokio::test]
    async fn login_maps_401_to_invalid_credentials() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/api/cabinet/auth/email/login")
            .with_status(401)
            .with_body(r#"{"detail":"Invalid email or password"}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url());
        let err = client.login("user@example.com", "wrong").await.unwrap_err();
        assert!(matches!(err, ApiError::InvalidCredentials));
    }

    #[tokio::test]
    async fn login_maps_429_to_rate_limited() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/api/cabinet/auth/email/login")
            .with_status(429)
            .create_async()
            .await;

        let client = ApiClient::new(server.url());
        let err = client.login("u@e.com", "p").await.unwrap_err();
        assert!(matches!(err, ApiError::RateLimited));
    }

    #[tokio::test]
    async fn refresh_returns_new_rotated_pair() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/api/cabinet/auth/refresh")
            .match_body(mockito::Matcher::Json(serde_json::json!({ "refresh_token": "old" })))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"access_token":"new-a","refresh_token":"new-r","token_type":"bearer","expires_in":900}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url());
        let result = client.refresh("old").await.unwrap();

        // Ключевая проверка: пришёл именно НОВЫЙ refresh, а не эхо старого —
        // сервер ротирует его, и вызывающий обязан сохранить это значение.
        assert_eq!(result.refresh_token, "new-r");
        assert_eq!(result.access_token, "new-a");
    }

    #[tokio::test]
    async fn refresh_with_dead_token_is_unauthorized() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/api/cabinet/auth/refresh")
            .with_status(401)
            .create_async()
            .await;

        let client = ApiClient::new(server.url());
        let err = client.refresh("dead").await.unwrap_err();
        assert!(matches!(err, ApiError::Unauthorized));
    }

    #[tokio::test]
    async fn subscription_info_parses_full_payload() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/api/cabinet/subscription/info")
            .match_header("authorization", "Bearer a")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"has_subscription":true,"subscription":{
                    "subscription_url":"https://sub.example/abc",
                    "days_left":2,"hours_left":5,"minutes_left":30,
                    "traffic_limit_gb":100,"traffic_used_gb":14.9,
                    "device_limit":2,"connected_squads":["486eb495"],
                    "servers":[{"uuid":"u1","name":"Frankfurt","country_code":"de"}],
                    "tariff_name":"Standard","is_active":true,"is_expired":false,
                    "hide_subscription_link":false
                }}"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url());
        let result = client.subscription_info("a").await.unwrap();

        assert!(result.has_subscription);
        let sub = result.subscription.unwrap();
        assert_eq!(sub.days_left, 2);
        assert_eq!(sub.traffic_used_gb, 14.9);
        assert_eq!(sub.servers[0].name, "Frankfurt");
        assert_eq!(sub.tariff_name.as_deref(), Some("Standard"));
    }

    #[tokio::test]
    async fn subscription_info_handles_no_subscription() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/api/cabinet/subscription/info")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"has_subscription":false,"subscription":null}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url());
        let result = client.subscription_info("a").await.unwrap();
        assert!(!result.has_subscription);
        assert!(result.subscription.is_none());
    }

    #[tokio::test]
    async fn subscription_info_rejects_expired_access_token() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/api/cabinet/subscription/info")
            .with_status(401)
            .create_async()
            .await;

        let client = ApiClient::new(server.url());
        let err = client.subscription_info("expired").await.unwrap_err();
        assert!(matches!(err, ApiError::Unauthorized));
    }

    #[tokio::test]
    async fn server_error_carries_detail_message() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/api/cabinet/subscription/info")
            .with_status(500)
            .with_body(r#"{"detail":"internal error"}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url());
        let err = client.subscription_info("a").await.unwrap_err();
        match err {
            ApiError::Server { status, message } => {
                assert_eq!(status, 500);
                assert_eq!(message, "internal error");
            }
            other => panic!("ожидался Server, получено {other:?}"),
        }
    }

    #[tokio::test]
    async fn malformed_json_body_is_a_decode_error() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/api/cabinet/subscription/info")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body("не json вообще")
            .create_async()
            .await;

        let client = ApiClient::new(server.url());
        let err = client.subscription_info("a").await.unwrap_err();
        assert!(matches!(err, ApiError::Decode(_)));
    }
}
