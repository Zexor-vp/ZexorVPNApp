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

/// Одноразовый токен входа через Telegram: пользователь открывает
/// `t.me/{bot_username}?start=webauth_{token}`, а клиент поллит `deeplink_poll`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeepLinkToken {
    pub token: String,
    pub bot_username: String,
    pub expires_in: i64,
}

/// Результат одного опроса входа через Telegram.
#[derive(Debug, Clone)]
pub enum LoginPoll {
    /// Пользователь ещё не подтвердил вход в боте.
    Pending,
    /// Токен истёк или уже использован — нужно начать заново.
    Expired,
    Completed(LoginResponse),
}

/// Результат одного опроса «парного» входа через браузер
/// (`/cabinet/adblock/pair/status`, та же страница входа, что и у MyBlock).
#[derive(Debug, Clone)]
pub enum PairPoll {
    Pending,
    Expired,
    Completed {
        access_token: String,
        refresh_token: String,
    },
}

#[derive(Debug, Deserialize)]
struct PairStatusBody {
    status: String,
    #[serde(default)]
    access_token: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
}

/// Значение заголовка `X-Zexor-Client`: `desktop/<версия приложения>`.
pub fn client_header_value() -> String {
    format!("desktop/{}", env!("CARGO_PKG_VERSION"))
}

fn client_headers() -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Ok(value) = reqwest::header::HeaderValue::from_str(&client_header_value()) {
        headers.insert("X-Zexor-Client", value);
    }
    headers
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
                // Сервер по этому заголовку узнаёт пользователей приложения (у них увеличенная квота грейса).
                .default_headers(client_headers())
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

    /// Универсальный авторизованный запрос к Cabinet API: страницы приложения (тариф,
    /// профиль, рефералы) ходят через него, не требуя отдельной обвязки на каждый метод.
    ///
    /// Разрешены только пути кабинета (`/api/cabinet/...`) без `..` и без схемы/хоста —
    /// токен нельзя отправить на посторонний адрес. Пустой ответ превращается в `null`.
    pub async fn raw_request(
        &self,
        method: &str,
        path: &str,
        access_token: &str,
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, ApiError> {
        if !path.starts_with("/api/cabinet/") || path.contains("..") || path.contains("://") {
            return Err(ApiError::Server {
                status: 400,
                message: "недопустимый путь запроса".to_string(),
            });
        }
        let method = match method.to_ascii_uppercase().as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "PATCH" => reqwest::Method::PATCH,
            "DELETE" => reqwest::Method::DELETE,
            other => {
                return Err(ApiError::Server {
                    status: 400,
                    message: format!("неподдерживаемый метод: {other}"),
                })
            }
        };

        let mut request = self
            .http
            .request(method, self.url(path))
            .bearer_auth(access_token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await?;

        let status = response.status();
        let text = response.text().await?;
        if status.is_success() {
            if text.trim().is_empty() {
                return Ok(serde_json::Value::Null);
            }
            return serde_json::from_str(&text).map_err(|e| ApiError::Decode(e.to_string()));
        }
        Err(match status.as_u16() {
            401 => ApiError::Unauthorized,
            429 => ApiError::RateLimited,
            code => ApiError::Server {
                status: code,
                message: extract_detail(&text),
            },
        })
    }

    /// Запрашивает одноразовый токен для входа через Telegram-бота.
    pub async fn deeplink_request(&self) -> Result<DeepLinkToken, ApiError> {
        let response = self
            .http
            .post(self.url("/api/cabinet/auth/deeplink/request"))
            .send()
            .await?;

        self.parse_or_error(response, |_, _| ApiError::Unauthorized)
            .await
    }

    /// Один опрос входа через Telegram. Бэкенд отвечает 202, пока вход не
    /// подтверждён, 410 — если токен истёк/использован, 200 с парой токенов —
    /// при успехе (токен при этом гасится на сервере, повторно его не получить).
    pub async fn deeplink_poll(&self, token: &str) -> Result<LoginPoll, ApiError> {
        let response = self
            .http
            .post(self.url("/api/cabinet/auth/deeplink/poll"))
            .json(&serde_json::json!({ "token": token }))
            .send()
            .await?;

        match response.status().as_u16() {
            202 => Ok(LoginPoll::Pending),
            410 => Ok(LoginPoll::Expired),
            _ => self
                .parse_or_error(response, |_, _| ApiError::Unauthorized)
                .await
                .map(LoginPoll::Completed),
        }
    }

    /// Один опрос браузерного входа (Google / любой способ на веб-странице).
    /// `state` генерирует само приложение; сервер отдаёт токены один раз и
    /// удаляет сессию.
    pub async fn pair_status(&self, state: &str) -> Result<PairPoll, ApiError> {
        let response = self
            .http
            .get(self.url("/api/cabinet/adblock/pair/status"))
            .query(&[("state", state)])
            .send()
            .await?;

        let body: PairStatusBody = self
            .parse_or_error(response, |_, _| ApiError::Unauthorized)
            .await?;

        match (body.status.as_str(), body.access_token, body.refresh_token) {
            ("completed", Some(access_token), Some(refresh_token)) => Ok(PairPoll::Completed {
                access_token,
                refresh_token,
            }),
            ("expired", _, _) => Ok(PairPoll::Expired),
            _ => Ok(PairPoll::Pending),
        }
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
        .and_then(|v| {
            let detail = v.get("detail")?;
            // Обычно строка; у бизнес-ошибок (например, «не хватает средств») — объект с `message`.
            detail.as_str().map(str::to_string).or_else(|| {
                detail
                    .get("message")
                    .and_then(|m| m.as_str())
                    .map(str::to_string)
            })
        })
        .unwrap_or_else(|| body.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn every_request_identifies_the_desktop_client() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/api/cabinet/ping")
            .match_header(
                "x-zexor-client",
                mockito::Matcher::Regex("^desktop/[0-9]+\\.[0-9]+\\.[0-9]+".to_string()),
            )
            .with_status(200)
            .with_body("{}")
            .create_async()
            .await;

        let client = ApiClient::new(server.url());
        client
            .raw_request("GET", "/api/cabinet/ping", "t", None)
            .await
            .unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn raw_request_sends_bearer_and_returns_json() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("POST", "/api/cabinet/subscription/protocol")
            .match_header("authorization", "Bearer tok")
            .match_body(mockito::Matcher::Json(
                serde_json::json!({"protocol": "wireguard"}),
            ))
            .with_status(200)
            .with_body(r#"{"active_protocol":"wireguard"}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url());
        let value = client
            .raw_request(
                "post",
                "/api/cabinet/subscription/protocol",
                "tok",
                Some(serde_json::json!({"protocol": "wireguard"})),
            )
            .await
            .unwrap();
        assert_eq!(value["active_protocol"], "wireguard");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn raw_request_rejects_foreign_or_traversal_paths() {
        let client = ApiClient::new("http://127.0.0.1:1");
        for path in [
            "/api/other/x",
            "https://evil.example/api/cabinet/x",
            "/api/cabinet/../admin",
            "api/cabinet/x",
        ] {
            let err = client
                .raw_request("GET", path, "t", None)
                .await
                .unwrap_err();
            assert!(
                matches!(err, ApiError::Server { status: 400, .. }),
                "{path}: {err:?}"
            );
        }
        let err = client
            .raw_request("TRACE", "/api/cabinet/x", "t", None)
            .await
            .unwrap_err();
        assert!(matches!(err, ApiError::Server { status: 400, .. }));
    }

    #[tokio::test]
    async fn raw_request_maps_errors_and_nested_detail_message() {
        let mut server = Server::new_async().await;
        let _unauth = server
            .mock("GET", "/api/cabinet/a")
            .with_status(401)
            .create_async()
            .await;
        let _biz = server
            .mock("POST", "/api/cabinet/b")
            .with_status(400)
            .with_body(r#"{"detail":{"code":"insufficient_funds","message":"Не хватает 100 ₽"}}"#)
            .create_async()
            .await;
        let _empty = server
            .mock("DELETE", "/api/cabinet/c")
            .with_status(204)
            .create_async()
            .await;

        let client = ApiClient::new(server.url());
        assert!(matches!(
            client.raw_request("GET", "/api/cabinet/a", "t", None).await,
            Err(ApiError::Unauthorized)
        ));
        match client
            .raw_request("POST", "/api/cabinet/b", "t", None)
            .await
        {
            Err(ApiError::Server {
                status: 400,
                message,
            }) => assert_eq!(message, "Не хватает 100 ₽"),
            other => panic!("ожидалась ошибка 400 с message, получено {other:?}"),
        }
        assert_eq!(
            client
                .raw_request("DELETE", "/api/cabinet/c", "t", None)
                .await
                .unwrap(),
            serde_json::Value::Null
        );
    }

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

    #[tokio::test]
    async fn deeplink_request_returns_token_and_bot() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/api/cabinet/auth/deeplink/request")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"token":"tok123","bot_username":"Zexorvpnbot","expires_in":300}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url());
        let result = client.deeplink_request().await.unwrap();
        assert_eq!(result.token, "tok123");
        assert_eq!(result.bot_username, "Zexorvpnbot");
    }

    #[tokio::test]
    async fn deeplink_poll_202_is_pending() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/api/cabinet/auth/deeplink/poll")
            .match_body(mockito::Matcher::Json(
                serde_json::json!({ "token": "tok123" }),
            ))
            .with_status(202)
            .with_header("content-type", "application/json")
            .with_body(r#"{"detail":"Waiting for confirmation"}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url());
        assert!(matches!(
            client.deeplink_poll("tok123").await.unwrap(),
            LoginPoll::Pending
        ));
    }

    #[tokio::test]
    async fn deeplink_poll_410_is_expired() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/api/cabinet/auth/deeplink/poll")
            .with_status(410)
            .with_body(r#"{"detail":"Token expired or not found"}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url());
        assert!(matches!(
            client.deeplink_poll("tok123").await.unwrap(),
            LoginPoll::Expired
        ));
    }

    #[tokio::test]
    async fn deeplink_poll_200_returns_tokens_for_telegram_only_user() {
        let mut server = Server::new_async().await;
        server
            .mock("POST", "/api/cabinet/auth/deeplink/poll")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{"access_token":"a","refresh_token":"r","token_type":"bearer","expires_in":900,
                    "user":{"id":7,"email":null,"first_name":"Иван"}}"#,
            )
            .create_async()
            .await;

        let client = ApiClient::new(server.url());
        match client.deeplink_poll("tok123").await.unwrap() {
            LoginPoll::Completed(login) => {
                assert_eq!(login.refresh_token, "r");
                assert_eq!(login.user.email, None);
            }
            other => panic!("ожидался Completed, получено {other:?}"),
        }
    }

    #[tokio::test]
    async fn pair_status_maps_all_three_states() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/api/cabinet/adblock/pair/status")
            .match_query(mockito::Matcher::UrlEncoded(
                "state".into(),
                "s-pending".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"pending","access_token":null,"refresh_token":null}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/api/cabinet/adblock/pair/status")
            .match_query(mockito::Matcher::UrlEncoded(
                "state".into(),
                "s-done".into(),
            ))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"completed","access_token":"A","refresh_token":"R"}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/api/cabinet/adblock/pair/status")
            .match_query(mockito::Matcher::UrlEncoded("state".into(), "s-old".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"expired","access_token":null,"refresh_token":null}"#)
            .create_async()
            .await;

        let client = ApiClient::new(server.url());
        assert!(matches!(
            client.pair_status("s-pending").await.unwrap(),
            PairPoll::Pending
        ));
        assert!(matches!(
            client.pair_status("s-old").await.unwrap(),
            PairPoll::Expired
        ));
        match client.pair_status("s-done").await.unwrap() {
            PairPoll::Completed {
                access_token,
                refresh_token,
            } => {
                assert_eq!(access_token, "A");
                assert_eq!(refresh_token, "R");
            }
            other => panic!("ожидался Completed, получено {other:?}"),
        }
    }
}
