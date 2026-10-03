//! Оркестрация подключения: получить подписку → распарсить узлы → собрать
//! конфиг → поднять процесс → включить системный прокси.

pub mod commands;

use std::path::PathBuf;
use std::time::Duration;

use tauri::{AppHandle, Manager};
use zexor_vpn_core::adblock::counter::BlockCounter;
use zexor_vpn_core::{probe, ConfigOptions, Node, TunnelMode, XrayProcess};

use crate::auth::{ensure_valid_access_token, AuthError};
use crate::state::AppState;
use zexor_vpn_core::sources::{self, ACCOUNT_SOURCE};

#[derive(Debug, thiserror::Error)]
pub enum ConnectError {
    #[error(transparent)]
    Auth(#[from] AuthError),
    #[error("у аккаунта нет активной подписки")]
    NoSubscription,
    #[error("не удалось получить содержимое подписки: {0}")]
    FetchSubscription(String),
    /// Сервер подписки ответил, но отказал или вернул не подписку — текст уже понятен пользователю.
    #[error("{0}")]
    SubscriptionRejected(String),
    #[error("не удалось разобрать подписку: {0}")]
    Parse(#[from] zexor_vpn_core::ParseError),
    #[error("узел \"{0}\" не найден в подписке")]
    NodeNotFound(String),
    #[error("подписка не найдена — возможно, она была удалена")]
    SourceNotFound,
    #[error("бинарник xray не найден: {0}")]
    BinaryMissing(String),
    #[error(transparent)]
    Process(#[from] zexor_vpn_core::XrayError),
    #[error("не удалось включить системный прокси: {0}")]
    Proxy(String),
    #[error("режиму TUN нужны права администратора")]
    NeedsElevation,
    #[error("для режима TUN не хватает файла {0}")]
    TunDriverMissing(String),
    #[error("в подписке нет серверов, к которым можно подключиться автоматически")]
    NothingToBalance,
    #[error("ни один сервер не ответил — возможно, их блокирует провайдер")]
    NoWorkingServer,
}

/// Скачивает тело подписки по ссылке. Свой User-Agent нужен, чтобы панель отдала
/// обычный список ссылок, а не JSON-профили для Happ.
pub async fn download_subscription(url: &str) -> Result<String, ConnectError> {
    download_with_headers(url, Vec::new()).await
}

/// Подписка аккаунта скачивается «как Happ»: с идентификатором устройства и JSON-профилями xray —
/// в них лежат серверные правила маршрутизации и балансировщик «AUTO». Чужим сервисам идентификатор
/// устройства не отправляем.
async fn download_account_subscription(url: &str) -> Result<String, ConnectError> {
    let hwid = zexor_vpn_core::hwid::load_or_create();
    download_with_headers(url, zexor_vpn_core::hwid::subscription_headers(&hwid)).await
}

async fn download_with_headers(
    url: &str,
    headers: Vec<(&'static str, String)>,
) -> Result<String, ConnectError> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("ZexorVPN-Desktop/", env!("CARGO_PKG_VERSION")))
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| ConnectError::FetchSubscription(e.to_string()))?;
    let mut request = client.get(url);
    // `header` заменяет значение, поэтому наш User-Agent по умолчанию уступает Happ-овскому.
    for (name, value) in headers {
        request = request.header(name, value);
    }
    // Текст ошибки reqwest содержит адрес подписки (а в нём — ключ пользователя), поэтому наружу
    // он не отдаётся: только короткое объяснение.
    let response = request
        .send()
        .await
        .map_err(|_| ConnectError::FetchSubscription("сервер подписки не отвечает".to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err(ConnectError::SubscriptionRejected(match status.as_u16() {
            401 | 403 => {
                "подписка недоступна: проверьте, что она активна и не превышен лимит устройств"
                    .to_string()
            }
            404 => "подписка не найдена — возможно, она удалена или ссылка изменилась".to_string(),
            429 => "слишком много запросов, повторите через минуту".to_string(),
            500..=599 => {
                "сервис подписки временно недоступен (идёт обновление), повторите через минуту"
                    .to_string()
            }
            code => format!("сервер подписки ответил кодом {code}"),
        }));
    }
    let body = response.text().await.map_err(|_| {
        ConnectError::FetchSubscription("не удалось прочитать ответ сервера подписки".to_string())
    })?;
    // Вместо подписки может прийти HTML-страница (заглушка прокси, ошибка шлюза).
    if body.trim_start().starts_with('<') {
        return Err(ConnectError::SubscriptionRejected(
            "сервис подписки вернул страницу вместо подписки, повторите через минуту".to_string(),
        ));
    }
    Ok(body)
}

/// Ссылка подписки аккаунта из кабинета.
async fn account_subscription_url(state: &AppState) -> Result<String, ConnectError> {
    let access_token = ensure_valid_access_token(state).await?;

    let status = state
        .api
        .subscription_info(&access_token)
        .await
        .map_err(AuthError::Api)?;

    let subscription = status.subscription.ok_or(ConnectError::NoSubscription)?;
    subscription
        .subscription_url
        .ok_or(ConnectError::NoSubscription)
}

/// Скачивает и разбирает актуальный список узлов выбранной подписки: `None` или
/// `"account"` — подписка аккаунта, иначе id добавленной пользователем.
pub async fn fetch_nodes(
    state: &AppState,
    source_id: Option<&str>,
) -> Result<Vec<Node>, ConnectError> {
    let body = match source_id {
        None | Some(ACCOUNT_SOURCE) => {
            let url = account_subscription_url(state).await?;
            download_account_subscription(&url).await?
        }
        Some(id) => {
            let url = sources::find(id).ok_or(ConnectError::SourceNotFound)?.url;
            download_subscription(&url).await?
        }
    };
    Ok(zexor_vpn_core::parse_nodes(&body)?)
}

/// Путь к бандленному sidecar-бинарнику xray.
///
/// Tauri кладёт `externalBin` рядом с исполняемым файлом приложения под именем без target triple
/// (`xray.exe`); остальные варианты — для запуска из исходников и на случай другой раскладки.
pub fn resolve_xray_binary(app: &AppHandle) -> Result<PathBuf, ConnectError> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("xray.exe"));
            candidates.push(dir.join("xray"));
        }
    }
    if let Ok(resource_dir) = app.path().resource_dir() {
        candidates.push(resource_dir.join("xray.exe"));
        candidates.push(
            resource_dir
                .join("binaries")
                .join("xray-x86_64-pc-windows-msvc.exe"),
        );
    }
    candidates
        .iter()
        .find(|path| path.is_file())
        .cloned()
        .ok_or_else(|| {
            ConnectError::BinaryMissing(
                candidates
                    .first()
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|| "xray.exe".to_string()),
            )
        })
}

/// Папка с geosite.dat/geoip.dat — xray берёт её из `XRAY_LOCATION_ASSET`. Задаётся один раз при запуске
/// приложения, дочерние процессы (в том числе пробные xray для проверки серверов) наследуют.
pub fn export_geo_assets_dir(app: &AppHandle) {
    let Ok(resource_dir) = app.path().resource_dir() else {
        return;
    };
    let dir = resource_dir.join("resources");
    if dir.join("geosite.dat").is_file() {
        std::env::set_var("XRAY_LOCATION_ASSET", dir);
    }
}

/// Режиму TUN xray нужен `wintun.dll` рядом с собой (драйвер адаптера). В установщике он лежит среди
/// ресурсов; перед запуском кладём копию к `xray.exe`.
fn ensure_wintun(app: &AppHandle, xray_binary: &std::path::Path) -> Result<(), ConnectError> {
    let Some(target_dir) = xray_binary.parent() else {
        return Ok(());
    };
    let target = target_dir.join("wintun.dll");
    if target.is_file() {
        return Ok(());
    }
    let source = app
        .path()
        .resource_dir()
        .map(|dir| dir.join("resources").join("wintun.dll"))
        .map_err(|e| ConnectError::TunDriverMissing(e.to_string()))?;
    if !source.is_file() {
        return Err(ConnectError::TunDriverMissing("wintun.dll".to_string()));
    }
    std::fs::copy(&source, &target)
        .map(|_| ())
        .map_err(|e| ConnectError::TunDriverMissing(format!("wintun.dll ({e})")))
}

/// Читает бандленный дефолтный блок-лист рекламы. Отсутствие файла не
/// считается ошибкой — просто подключаемся без адблока.
pub fn load_bundled_blocklist(app: &AppHandle) -> Vec<String> {
    let Ok(resource_dir) = app.path().resource_dir() else {
        return Vec::new();
    };
    let path = resource_dir
        .join("resources")
        .join("adblock")
        .join("default.txt");
    match std::fs::read_to_string(&path) {
        Ok(contents) => zexor_vpn_core::adblock::parse_blocklist(&contents),
        Err(err) => {
            tracing::warn!(?err, path = %path.display(), "не удалось прочитать дефолтный блок-лист");
            Vec::new()
        }
    }
}

/// Параметры сборки конфига: адблок, режим туннеля и маршрутизация по приложениям из настроек.
fn build_options(
    app: &AppHandle,
    state: &AppState,
    xray_binary: &std::path::Path,
) -> Result<ConfigOptions, ConnectError> {
    let bundled = load_bundled_blocklist(app);
    let (adblock_domains, adblock_allow_domains) = {
        let cfg = state.adblock.lock().unwrap();
        (cfg.effective_block(&bundled), cfg.custom_allow.clone())
    };

    // Журнал доступа нужен счётчику заблокированной рекламы. Файл каждой сессии — новый.
    let access_log = zexor_vpn_core::xray::process::app_data_dir().join("access.log");
    let _ = std::fs::remove_file(&access_log);

    let mut options = ConfigOptions {
        adblock_domains,
        adblock_allow_domains,
        access_log_path: Some(access_log.to_string_lossy().into_owned()),
        ..ConfigOptions::default()
    };
    state.settings.lock().unwrap().apply_to(&mut options);

    if options.mode == TunnelMode::Tun {
        if !zexor_vpn_core::elevation::is_elevated() {
            return Err(ConnectError::NeedsElevation);
        }
        ensure_wintun(app, xray_binary)?;
    }
    Ok(options)
}

/// Что запускаем: один сервер (профиль подписки как есть) или собственный балансировщик из набора серверов.
enum Target<'a> {
    Node(&'a Node),
    Balanced(&'a [Node]),
}

fn start_tunnel(
    app: &AppHandle,
    state: &AppState,
    target: Target<'_>,
    label: &str,
    source_id: &str,
    auto: bool,
) -> Result<(), ConnectError> {
    let binary = resolve_xray_binary(app)?;
    let options = build_options(app, state, &binary)?;

    let config = match target {
        Target::Node(node) => zexor_vpn_core::build_node_config(node, &options),
        Target::Balanced(nodes) => zexor_vpn_core::build_balanced_config(nodes, &options)
            .ok_or(ConnectError::NothingToBalance)?,
    };
    let config_path = zexor_vpn_core::xray::process::config_file_path();
    let process = XrayProcess::start(&binary, &config, &config_path)?;

    // В режиме TUN трафик забирает сам адаптер, системный прокси не нужен (и мешал бы: часть
    // приложений слала бы трафик на локальный порт, минуя правила по приложениям).
    if options.mode == TunnelMode::Proxy {
        zexor_vpn_core::proxy::enable(options.http_port)
            .map_err(|e| ConnectError::Proxy(e.to_string()))?;
    }

    let access_log = options.access_log_path.clone().map(PathBuf::from);
    if let Some(path) = access_log {
        state.block_stats.lock().unwrap().counter = Some(BlockCounter::new(path));
    }

    let mut connection = state.connection.lock().unwrap();
    connection.process = Some(process);
    connection.connected_node = Some(label.to_string());
    connection.connected_source = Some(source_id.to_string());
    connection.auto = auto;
    Ok(())
}

/// Полный цикл подключения к конкретному узлу подписки.
pub async fn connect_to_node(
    app: &AppHandle,
    state: &AppState,
    node: &Node,
    source_id: &str,
) -> Result<(), ConnectError> {
    start_tunnel(
        app,
        state,
        Target::Node(node),
        node.remark(),
        source_id,
        false,
    )
}

/// Подпись подключения в авто-режиме.
pub const AUTO_LABEL: &str = "AUTO";

/// Сколько лучших по TCP-пингу серверов проверять по-настоящему — каждая проверка поднимает свой xray.
const AUTO_CANDIDATES: usize = 6;
/// Сколько проверок идёт одновременно.
const AUTO_CONCURRENCY: usize = 3;

/// Серверы подписки без панельного «AUTO» (его не выбирают как сервер).
fn plain_servers(nodes: &[Node]) -> Vec<Node> {
    nodes.iter().filter(|n| !n.is_balanced()).cloned().collect()
}

/// Самый быстрый рабочий сервер: берёт самые близкие по TCP, проверяет их реально (через туннель)
/// и возвращает лучший.
pub async fn fastest_node(app: &AppHandle, nodes: Vec<Node>) -> Result<(Node, u32), ConnectError> {
    let pings = probe::tcp_ping_many(&nodes, Duration::from_secs(3)).await;

    // WireGuard меряем по TCP 443 «на всякий случай»: чужая подписка может его и не слушать, поэтому
    // такие узлы не отсеиваем по пингу, а оставляем на реальную проверку.
    let mut ranked: Vec<(Node, u32)> = nodes
        .into_iter()
        .zip(pings)
        .filter_map(|(node, ms)| match (ms, &node) {
            (Some(ms), _) => Some((node, ms)),
            (None, Node::WireGuard(_)) => Some((node, u32::MAX)),
            (None, n) if n.protocol() == "wireguard" => Some((node, u32::MAX)),
            (None, _) => None,
        })
        .collect();
    if ranked.is_empty() {
        return Err(ConnectError::NoWorkingServer);
    }
    ranked.sort_by_key(|(_, ms)| *ms);
    let candidates: Vec<Node> = ranked
        .into_iter()
        .take(AUTO_CANDIDATES)
        .map(|(node, _)| node)
        .collect();

    let binary = resolve_xray_binary(app)?;
    let outcomes = probe::real_delay_many(&binary, &candidates, AUTO_CONCURRENCY).await;
    let (index, ms) = probe::best_of(&outcomes).ok_or(ConnectError::NoWorkingServer)?;
    Ok((candidates[index].clone(), ms))
}

/// Что получилось при авто-подключении.
pub struct AutoOutcome {
    pub label: String,
    /// Задержка выбранного сервера — только когда клиент выбирал его сам (чужие подписки).
    pub ms: Option<u32>,
}

/// Авто-режим.
/// * Подписка аккаунта, VLESS: серверный «AUTO» с балансировщиком панели.
/// * Подписка аккаунта, WireGuard (и нет серверного «AUTO»): собственный балансировщик приложения —
///   сам включает YouTube-серверы и следит за живостью.
/// * Чужая подписка: один раз выбираем самый быстрый сервер.
pub async fn connect_auto(
    app: &AppHandle,
    state: &AppState,
    nodes: &[Node],
    source_id: &str,
) -> Result<AutoOutcome, ConnectError> {
    let servers = plain_servers(nodes);

    if source_id == ACCOUNT_SOURCE {
        let is_wireguard = servers.iter().any(|n| n.protocol() == "wireguard");
        if !is_wireguard {
            if let Some(panel_auto) = nodes.iter().find(|n| n.is_balanced()) {
                start_tunnel(
                    app,
                    state,
                    Target::Node(panel_auto),
                    AUTO_LABEL,
                    source_id,
                    true,
                )?;
                return Ok(AutoOutcome {
                    label: AUTO_LABEL.to_string(),
                    ms: None,
                });
            }
        }
        if servers.is_empty() {
            return Err(ConnectError::NothingToBalance);
        }
        start_tunnel(
            app,
            state,
            Target::Balanced(&servers),
            AUTO_LABEL,
            source_id,
            true,
        )?;
        return Ok(AutoOutcome {
            label: AUTO_LABEL.to_string(),
            ms: None,
        });
    }

    if servers.is_empty() {
        // В чужой подписке только готовый балансировщик — запускаем его как есть.
        let Some(balanced) = nodes.iter().find(|n| n.is_balanced()) else {
            return Err(ConnectError::NothingToBalance);
        };
        start_tunnel(
            app,
            state,
            Target::Node(balanced),
            AUTO_LABEL,
            source_id,
            true,
        )?;
        return Ok(AutoOutcome {
            label: AUTO_LABEL.to_string(),
            ms: None,
        });
    }
    let (node, ms) = fastest_node(app, servers).await?;
    start_tunnel(
        app,
        state,
        Target::Node(&node),
        node.remark(),
        source_id,
        true,
    )?;
    Ok(AutoOutcome {
        label: node.remark().to_string(),
        ms: Some(ms),
    })
}

/// Останавливает туннель и откатывает системный прокси. Безопасно вызывать,
/// даже если подключения не было.
pub fn disconnect_now(state: &AppState) -> Result<(), ConnectError> {
    let mut connection = state.connection.lock().unwrap();
    if let Some(mut process) = connection.process.take() {
        process.stop();
    }
    connection.connected_node = None;
    connection.connected_source = None;
    connection.auto = false;
    drop(connection);
    finish_block_stats(state);

    zexor_vpn_core::proxy::restore().map_err(|e| ConnectError::Proxy(e.to_string()))
}

/// Если сейчас есть подключение — переподключается так же (тот же узел или авто-режим): правила
/// (адблок, приложения, режим туннеля) читаются при сборке конфига, так что без этого новая настройка
/// подействовала бы только после ручного переподключения, а пользователь ждёт мгновенного эффекта.
pub async fn reconnect_if_connected(app: &AppHandle, state: &AppState) -> Result<(), ConnectError> {
    let (remark, source_id, auto) = {
        let connection = state.connection.lock().unwrap();
        (
            connection.connected_node.clone(),
            connection.connected_source.clone(),
            connection.auto,
        )
    };
    let Some(remark) = remark else {
        return Ok(());
    };
    let source_id = source_id.unwrap_or_else(|| ACCOUNT_SOURCE.to_string());

    let nodes = fetch_nodes(state, Some(&source_id)).await?;
    if auto {
        disconnect_now(state)?;
        return connect_auto(app, state, &nodes, &source_id)
            .await
            .map(|_| ());
    }
    let Some(node) = nodes.into_iter().find(|n| n.remark() == remark) else {
        // Подписка успела смениться и узла больше нет — просто отключаемся,
        // это лучше, чем оставить туннель в рассинхроне с настройками.
        return disconnect_now(state);
    };

    disconnect_now(state)?;
    connect_to_node(app, state, &node, &source_id).await
}

/// Закрывает счёт текущей сессии: дочитывает журнал, прибавляет итог к накопленному и убирает файл.
/// Вызывать после остановки xray, чтобы журнал уже не писался.
pub fn finish_block_stats(state: &AppState) {
    let mut stats = state.block_stats.lock().unwrap();
    if let Some(mut counter) = stats.counter.take() {
        stats.accumulated += counter.poll();
        let _ = std::fs::remove_file(counter.path());
    }
}
