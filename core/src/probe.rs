//! Проверка серверов: быстрый TCP-пинг и «реальная задержка» через настоящий туннель.
//!
//! TCP-пинг показывает, что хост вообще отвечает и насколько он далеко — он дёшев и идёт
//! параллельно по всему списку. Но TCP до порта проходит и тогда, когда провайдер душит
//! именно рукопожатие REALITY/WireGuard (так было с Чехией), поэтому «работает ли сервер
//! на самом деле» решает только реальная проверка: поднимаем отдельный xray на свободных
//! портах, ходим через него на `generate_204` и меряем время. Её же использует авто-режим.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::xray::config_builder::{build_node_config, ConfigOptions};
use crate::xray::node::Node;
use crate::xray::process::XrayProcess;

/// Куда стучаться за пингом. У VLESS это адрес и порт узла. У WireGuard порт UDP, TCP-пинг
/// к нему невозможен, поэтому меряем 443 того же хоста: все наши узлы слушают его, а
/// расстояние до сервера от порта не зависит.
pub fn node_endpoint(node: &Node) -> (String, u16) {
    match node {
        Node::Vless(n) => (n.address.clone(), n.port),
        Node::WireGuard(n) => (n.server_address.clone(), 443),
        Node::Profile(p) => p.endpoint().unwrap_or_default(),
        // UDP: TCP-пинг неприменим, меряем 443 того же хоста (как для WireGuard).
        Node::Awg(n) => (
            n.config
                .peer
                .endpoint
                .rsplit_once(':')
                .map(|(host, _)| host.trim_matches(['[', ']']).to_string())
                .unwrap_or_default(),
            443,
        ),
    }
}

/// Время TCP-соединения в миллисекундах или `None`, если хост не ответил за `timeout`.
pub async fn tcp_ping(host: &str, port: u16, timeout: Duration) -> Option<u32> {
    let started = Instant::now();
    match tokio::time::timeout(timeout, TcpStream::connect((host, port))).await {
        Ok(Ok(_)) => Some(started.elapsed().as_millis().max(1) as u32),
        _ => None,
    }
}

/// TCP-пинг всех узлов параллельно. Результат в том же порядке, что и вход.
pub async fn tcp_ping_many(nodes: &[Node], timeout: Duration) -> Vec<Option<u32>> {
    let mut set = JoinSet::new();
    for (index, node) in nodes.iter().enumerate() {
        let (host, port) = node_endpoint(node);
        set.spawn(async move { (index, tcp_ping(&host, port, timeout).await) });
    }
    let mut result = vec![None; nodes.len()];
    while let Some(joined) = set.join_next().await {
        if let Ok((index, ms)) = joined {
            result[index] = ms;
        }
    }
    result
}

#[derive(Debug, Error)]
pub enum ProbeError {
    #[error("не нашлось свободного порта для проверки")]
    NoFreePort,
    #[error("не удалось запустить проверку: {0}")]
    Start(String),
    #[error("сервер не ответил через туннель: {0}")]
    Unreachable(String),
}

/// Адрес, по которому проверяем, что трафик реально ходит через туннель.
const PROBE_URL: &str = "https://www.gstatic.com/generate_204";

/// Свободный локальный порт: занимаем 0, смотрим, что выдала система, освобождаем.
pub fn free_port() -> Option<u16> {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .ok()
        .and_then(|l| l.local_addr().ok())
        .map(|a| a.port())
}

fn probe_config_path(port: u16) -> PathBuf {
    std::env::temp_dir().join(format!("zexor-probe-{}-{port}.json", std::process::id()))
}

/// Реальная задержка: время ответа `generate_204` через отдельный экземпляр xray с этим узлом.
pub async fn real_delay(binary: &Path, node: &Node) -> Result<u32, ProbeError> {
    let socks_port = free_port().ok_or(ProbeError::NoFreePort)?;
    let http_port = free_port().ok_or(ProbeError::NoFreePort)?;
    let options = ConfigOptions {
        socks_port,
        http_port,
        adblock_domains: Vec::new(),
        adblock_allow_domains: Vec::new(),
        bypass_private: true,
        log_level: "none".to_string(),
        access_log_path: None,
        ..ConfigOptions::default()
    };
    let config = build_node_config(node, &options);
    let config_path = probe_config_path(socks_port);

    // XrayProcess::start спит и пишет файл синхронно — не держим им асинхронный рантайм.
    let binary = binary.to_path_buf();
    let path_for_start = config_path.clone();
    let process =
        tokio::task::spawn_blocking(move || XrayProcess::start(&binary, &config, &path_for_start))
            .await
            .map_err(|e| ProbeError::Start(e.to_string()))?
            .map_err(|e| ProbeError::Start(e.to_string()))?;

    let outcome = measure_through_socks(socks_port).await;

    drop(process);
    let _ = std::fs::remove_file(&config_path);
    outcome
}

async fn measure_through_socks(socks_port: u16) -> Result<u32, ProbeError> {
    // Ждём, пока xray начнёт слушать локальный порт.
    let deadline = Instant::now() + Duration::from_secs(3);
    while tcp_ping("127.0.0.1", socks_port, Duration::from_millis(200))
        .await
        .is_none()
    {
        if Instant::now() > deadline {
            return Err(ProbeError::Start(
                "локальный прокси не поднялся".to_string(),
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let proxy = reqwest::Proxy::all(format!("socks5h://127.0.0.1:{socks_port}"))
        .map_err(|e| ProbeError::Start(e.to_string()))?;
    let client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| ProbeError::Start(e.to_string()))?;

    let started = Instant::now();
    let response = client
        .get(PROBE_URL)
        .send()
        .await
        .map_err(|e| ProbeError::Unreachable(e.to_string()))?;
    if !(response.status().is_success()) {
        return Err(ProbeError::Unreachable(format!(
            "ответ {}",
            response.status()
        )));
    }
    Ok(started.elapsed().as_millis().max(1) as u32)
}

/// Результат проверки одного узла (индекс во входном списке и задержка либо ошибка).
pub type ProbeOutcome = (usize, Result<u32, ProbeError>);

/// Реальная проверка нескольких узлов с ограничением параллельности (каждая — отдельный xray).
pub async fn real_delay_many(
    binary: &Path,
    nodes: &[Node],
    concurrency: usize,
) -> Vec<ProbeOutcome> {
    let gate = Arc::new(Semaphore::new(concurrency.max(1)));
    let mut set = JoinSet::new();
    for (index, node) in nodes.iter().cloned().enumerate() {
        let gate = gate.clone();
        let binary = binary.to_path_buf();
        set.spawn(async move {
            let _permit = gate.acquire_owned().await;
            (index, real_delay(&binary, &node).await)
        });
    }
    let mut out = Vec::with_capacity(nodes.len());
    while let Some(joined) = set.join_next().await {
        if let Ok(item) = joined {
            out.push(item);
        }
    }
    out
}

/// Лучший (с минимальной задержкой) из успешно проверенных.
pub fn best_of(outcomes: &[ProbeOutcome]) -> Option<(usize, u32)> {
    outcomes
        .iter()
        .filter_map(|(index, result)| result.as_ref().ok().map(|ms| (*index, *ms)))
        .min_by_key(|(_, ms)| *ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xray::wireguard::parse_wireguard_uri;

    #[tokio::test]
    async fn tcp_ping_reaches_a_local_listener_and_misses_a_closed_port() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(tcp_ping("127.0.0.1", port, Duration::from_secs(2))
            .await
            .is_some());

        let closed = free_port().unwrap();
        assert!(tcp_ping("127.0.0.1", closed, Duration::from_millis(500))
            .await
            .is_none());
    }

    #[tokio::test]
    async fn tcp_ping_many_keeps_input_order() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let open_port = listener.local_addr().unwrap().port();
        let closed_port = free_port().unwrap();

        let uri = |port: u16| {
            format!("vless://00000000-1111-2222-3333-444444444444@127.0.0.1:{port}?type=tcp&security=none#n")
        };
        let nodes: Vec<Node> = [open_port, closed_port, open_port]
            .iter()
            .map(|p| Node::Vless(crate::xray::parser::parse_vless_uri(&uri(*p)).unwrap()))
            .collect();

        let result = tcp_ping_many(&nodes, Duration::from_millis(800)).await;
        assert!(result[0].is_some() && result[2].is_some());
        assert!(result[1].is_none());
    }

    #[test]
    fn wireguard_is_pinged_on_tcp_443() {
        let wg = parse_wireguard_uri(
            "wireguard://k@203.0.113.5:51820?publickey=a&address=10.0.0.2%2F32#x",
        )
        .unwrap();
        assert_eq!(
            node_endpoint(&Node::WireGuard(wg)),
            ("203.0.113.5".to_string(), 443)
        );
    }

    #[test]
    fn best_of_picks_the_fastest_successful_node() {
        let outcomes: Vec<ProbeOutcome> = vec![
            (0, Ok(300)),
            (1, Err(ProbeError::Unreachable("x".into()))),
            (2, Ok(120)),
            (3, Ok(200)),
        ];
        assert_eq!(best_of(&outcomes), Some((2, 120)));
        assert_eq!(best_of(&[(0, Err(ProbeError::NoFreePort))]), None);
    }

    #[tokio::test]
    async fn real_delay_reports_missing_binary_as_start_error() {
        let wg = parse_wireguard_uri(
            "wireguard://k@203.0.113.5:51820?publickey=a&address=10.0.0.2%2F32#x",
        )
        .unwrap();
        let err = real_delay(Path::new("/definitely/not/xray"), &Node::WireGuard(wg))
            .await
            .unwrap_err();
        assert!(matches!(err, ProbeError::Start(_)));
    }
}

/// Живая проверка против настоящего xray и узла: `ZEXOR_TEST_XRAY` — путь к xray,
/// `ZEXOR_TEST_NODE_URI` — vless:// или wireguard:// ссылка. Запускается вручную:
/// `cargo test -p zexor-vpn-core real_delay_live -- --ignored --nocapture`.
#[cfg(test)]
mod live {
    use super::*;
    use crate::xray::node::parse_nodes;

    #[tokio::test]
    #[ignore = "нужен xray и живой узел"]
    async fn real_delay_live() {
        let binary = std::env::var("ZEXOR_TEST_XRAY").expect("ZEXOR_TEST_XRAY");
        let uri = std::env::var("ZEXOR_TEST_NODE_URI").expect("ZEXOR_TEST_NODE_URI");
        let nodes = parse_nodes(&uri).unwrap();
        let ms = real_delay(Path::new(&binary), &nodes[0]).await;
        println!(
            "real_delay({}) = {:?}",
            nodes[0].protocol(),
            ms.as_ref().map_err(|e| e.to_string())
        );
        assert!(ms.is_ok());
    }
}
