//! Фоновые задачи приложения для вошедшего пользователя:
//! * команда «обновить подписку» от админа — приложение раз в несколько минут сверяет счётчик на
//!   сервере и, если он изменился, перечитывает подписку (и переподключается, если идёт туннель);
//! * анонимные замеры доступности серверов из сети пользователя — для карты блокировок в админке.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use zexor_vpn_core::sources::ACCOUNT_SOURCE;

use crate::auth::ensure_valid_access_token;
use crate::state::AppState;
use crate::xray::reconnect_if_connected;

/// Как часто сверяем счётчик с сервером.
const SYNC_CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// Первую проверку не делаем сразу при запуске: пусть приложение сначала загрузит интерфейс.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(45);
/// Не чаще раза в полчаса отправляем замеры (сервер дополнительно ограничивает до раза в 10 минут).
const REPORT_INTERVAL: Duration = Duration::from_secs(30 * 60);

/// Событие для интерфейса: подписка изменилась, пора перечитать серверы и данные.
pub const SUBSCRIPTION_CHANGED_EVENT: &str = "subscription-changed";

static LAST_REPORT: Mutex<Option<Instant>> = Mutex::new(None);

pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        loop {
            check_sync_command(&app).await;
            tokio::time::sleep(SYNC_CHECK_INTERVAL).await;
        }
    });
}

async fn check_sync_command(app: &AppHandle) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    // Без входа в аккаунт команда не нужна: токена нет, и это не ошибка.
    let Ok(token) = ensure_valid_access_token(&state).await else {
        return;
    };
    let Ok(value) = state
        .api
        .raw_request("GET", "/api/cabinet/app/sync-epoch", &token, None)
        .await
    else {
        return;
    };
    let Some(epoch) = value.get("epoch").and_then(|v| v.as_i64()) else {
        return;
    };

    let previous = {
        let mut settings = state.settings.lock().unwrap();
        let previous = settings.last_sync_epoch;
        if previous == Some(epoch) {
            return;
        }
        settings.last_sync_epoch = Some(epoch);
        if let Err(err) = zexor_vpn_core::settings::save(&settings) {
            tracing::warn!(?err, "не удалось сохранить эпоху синхронизации");
        }
        previous
    };
    // Самый первый запуск: счётчик только запоминаем, обновлять нечего.
    if previous.is_none() {
        return;
    }

    tracing::info!(epoch, "админ запросил обновление подписки");
    let connected_via_account = state
        .connection
        .lock()
        .unwrap()
        .connected_source
        .as_deref()
        .is_some_and(|source| source == ACCOUNT_SOURCE);
    if connected_via_account {
        // Серверы могли смениться: переподключаемся к тому же серверу по свежей подписке.
        if let Err(err) = reconnect_if_connected(app, &state).await {
            tracing::warn!(
                ?err,
                "не удалось переподключиться после обновления подписки"
            );
        }
    }
    let _ = app.emit(SUBSCRIPTION_CHANGED_EVENT, ());
}

/// Отправляет замеры серверов подписки аккаунта (`None` — не ответил). Тихо ничего не делает, если
/// пользователь отключил отправку, не вошёл или отправляли недавно.
pub fn report_pings(app: &AppHandle, results: Vec<(String, Option<u32>)>) {
    if results.is_empty() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        if !state.settings.lock().unwrap().telemetry_enabled {
            return;
        }
        {
            let mut last = LAST_REPORT.lock().unwrap();
            if last.is_some_and(|at| at.elapsed() < REPORT_INTERVAL) {
                return;
            }
            *last = Some(Instant::now());
        }
        let Ok(token) = ensure_valid_access_token(&state).await else {
            return;
        };
        let body = json!({
            "app_version": env!("CARGO_PKG_VERSION"),
            "results": results
                .into_iter()
                .map(|(server, ms)| json!({ "server": server, "tcp_ms": ms }))
                .collect::<Vec<_>>(),
        });
        if let Err(err) = state
            .api
            .raw_request(
                "POST",
                "/api/cabinet/telemetry/server-reachability",
                &token,
                Some(body),
            )
            .await
        {
            tracing::debug!(?err, "замеры доступности не отправлены");
        }
    });
}

/// Как часто спрашиваем сервер про обращения в поддержку. Таймеры в самой странице (WebView2) замедляются,
/// когда окно закрыто другими окнами или спрятано в трей, поэтому опрос живёт здесь, в Rust.
const SUPPORT_POLL_INTERVAL: Duration = Duration::from_secs(20);

/// Событие для интерфейса: свежий список обращений (тот же JSON, что отдаёт `GET /tickets`).
pub const SUPPORT_EVENT: &str = "support-tickets";

pub fn spawn_support_poll(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(15)).await;
        // обращение → id последнего уже замеченного ответа поддержки
        let mut known: HashMap<i64, i64> = HashMap::new();
        let mut first = true;
        loop {
            poll_support(&app, &mut known, &mut first).await;
            tokio::time::sleep(SUPPORT_POLL_INTERVAL).await;
        }
    });
}

async fn poll_support(app: &AppHandle, known: &mut HashMap<i64, i64>, first: &mut bool) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(token) = ensure_valid_access_token(&state).await else {
        return;
    };
    let Ok(value) = state
        .api
        .raw_request(
            "GET",
            "/api/cabinet/tickets?page=1&per_page=30",
            &token,
            None,
        )
        .await
    else {
        return;
    };

    let mut new_reply_titles: Vec<String> = Vec::new();
    if let Some(items) = value.get("items").and_then(|v| v.as_array()) {
        for item in items {
            let Some(id) = item.get("id").and_then(|v| v.as_i64()) else {
                continue;
            };
            let Some(last) = item.get("last_message").filter(|m| !m.is_null()) else {
                continue;
            };
            let from_admin = last
                .get("is_from_admin")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let Some(last_id) = last.get("id").and_then(|v| v.as_i64()) else {
                continue;
            };
            if !from_admin {
                continue;
            }
            let previous = known.insert(id, last_id);
            if previous.map_or(true, |p| p < last_id) {
                new_reply_titles.push(
                    item.get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                );
            }
        }
    }

    // Самый первый опрос только запоминает, что уже есть, — иначе пришла бы пачка старых уведомлений.
    if !*first && !new_reply_titles.is_empty() {
        // Если окно приложения сейчас на виду, ответ видно и так — системное уведомление не нужно.
        let focused = app
            .get_webview_window("main")
            .is_some_and(|w| w.is_focused().unwrap_or(false) && w.is_visible().unwrap_or(false));
        if !focused {
            let title = state.labels.lock().unwrap().support_reply.clone();
            for body in new_reply_titles {
                let _ = app.notification().builder().title(&title).body(body).show();
            }
        }
    }
    *first = false;
    let _ = app.emit(SUPPORT_EVENT, value);
}
