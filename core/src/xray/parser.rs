//! Парсер подписочных ссылок Remnawave → структуры подключения.
//!
//! Панель отдаёт по `subscription_url` список строк (обычно base64), где каждая
//! строка — URI вида `vless://uuid@host:port?...#remark`. Из query-параметров
//! нужно достать ровно то, что потом уйдёт в `streamSettings` xray-конфига.

use std::collections::HashMap;

use percent_encoding::percent_decode_str;
use thiserror::Error;
use url::Url;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("не удалось декодировать подписку: содержимое не base64 и не список ссылок")]
    UndecodableSubscription,
    #[error("в подписке нет ни одной поддерживаемой ссылки (ожидался vless://)")]
    NoSupportedNodes,
    #[error("некорректный URI: {0}")]
    InvalidUri(String),
    #[error("неподдерживаемая схема: {0}")]
    UnsupportedScheme(String),
    #[error("в ссылке отсутствует UUID пользователя")]
    MissingUuid,
    #[error("в ссылке отсутствует адрес сервера")]
    MissingHost,
    #[error("в ссылке отсутствует порт")]
    MissingPort,
    #[error("REALITY требует параметр pbk (публичный ключ)")]
    MissingRealityPublicKey,
    #[error("REALITY требует параметр sni (serverName)")]
    MissingRealityServerName,
}

/// Транспортная безопасность узла.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Security {
    None,
    Tls {
        sni: Option<String>,
        fingerprint: Option<String>,
        alpn: Vec<String>,
        allow_insecure: bool,
    },
    Reality {
        /// `sni` — домен-маскировка, он же serverName.
        server_name: String,
        /// `pbk` — публичный ключ, выведенный из приватного ключа сервера.
        public_key: String,
        /// `sid` — один из shortId, сконфигурированных на инбаунде.
        short_id: Option<String>,
        /// `fp` — отпечаток TLS-клиента (chrome/edge/firefox/...).
        fingerprint: String,
        /// `spx` — spiderX, путь для маскировочного запроса.
        spider_x: Option<String>,
    },
}

/// Разобранный узел подписки, готовый к сборке в outbound xray.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VlessNode {
    pub remark: String,
    pub address: String,
    pub port: u16,
    pub uuid: String,
    pub flow: Option<String>,
    /// `type` из query: tcp / ws / grpc / http ...
    pub network: String,
    pub security: Security,
    /// Дополнительные параметры транспорта (path, host, serviceName...).
    pub transport: HashMap<String, String>,
}

impl VlessNode {
    /// REALITY-узлы — основной боевой вариант в этой панели.
    pub fn is_reality(&self) -> bool {
        matches!(self.security, Security::Reality { .. })
    }
}

fn decode_component(raw: &str) -> String {
    percent_decode_str(raw).decode_utf8_lossy().into_owned()
}

/// Разбирает одну ссылку `vless://...`.
pub fn parse_vless_uri(uri: &str) -> Result<VlessNode, ParseError> {
    let uri = uri.trim();
    if !uri.starts_with("vless://") {
        let scheme = uri.split("://").next().unwrap_or(uri);
        return Err(ParseError::UnsupportedScheme(scheme.to_string()));
    }

    let parsed = Url::parse(uri).map_err(|e| ParseError::InvalidUri(e.to_string()))?;

    let uuid = decode_component(parsed.username());
    if uuid.is_empty() {
        return Err(ParseError::MissingUuid);
    }

    let address = parsed
        .host_str()
        .ok_or(ParseError::MissingHost)?
        .to_string();
    if address.is_empty() {
        return Err(ParseError::MissingHost);
    }

    let port = parsed.port().ok_or(ParseError::MissingPort)?;

    let params: HashMap<String, String> = parsed
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    let network = params
        .get("type")
        .cloned()
        .unwrap_or_else(|| "tcp".to_string());

    let security = match params.get("security").map(String::as_str).unwrap_or("none") {
        "reality" => {
            let public_key = params
                .get("pbk")
                .filter(|s| !s.is_empty())
                .cloned()
                .ok_or(ParseError::MissingRealityPublicKey)?;
            let server_name = params
                .get("sni")
                .filter(|s| !s.is_empty())
                .cloned()
                .ok_or(ParseError::MissingRealityServerName)?;
            Security::Reality {
                server_name,
                public_key,
                short_id: params.get("sid").filter(|s| !s.is_empty()).cloned(),
                // Без fp сервер видит дефолтный Go-отпечаток — подставляем chrome,
                // как это делают штатные клиенты.
                fingerprint: params
                    .get("fp")
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .unwrap_or_else(|| "chrome".to_string()),
                spider_x: params.get("spx").filter(|s| !s.is_empty()).cloned(),
            }
        }
        "tls" => Security::Tls {
            sni: params.get("sni").filter(|s| !s.is_empty()).cloned(),
            fingerprint: params.get("fp").filter(|s| !s.is_empty()).cloned(),
            alpn: params
                .get("alpn")
                .map(|v| {
                    v.split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default(),
            allow_insecure: matches!(
                params.get("allowInsecure").map(String::as_str),
                Some("1") | Some("true")
            ),
        },
        _ => Security::None,
    };

    let mut transport = HashMap::new();
    for key in ["path", "host", "serviceName", "headerType", "mode"] {
        if let Some(value) = params.get(key).filter(|s| !s.is_empty()) {
            transport.insert(key.to_string(), value.clone());
        }
    }

    let remark = parsed
        .fragment()
        .map(decode_component)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("{address}:{port}"));

    Ok(VlessNode {
        remark,
        address,
        port,
        uuid,
        flow: params.get("flow").filter(|s| !s.is_empty()).cloned(),
        network,
        security,
        transport,
    })
}

/// Разбирает тело подписки: base64-блоб либо просто список ссылок построчно.
///
/// Неподдерживаемые протоколы (vmess/trojan/ss) молча пропускаются — панель может
/// отдавать их в одном списке с vless, и это не повод падать.
pub fn parse_subscription(body: &str) -> Result<Vec<VlessNode>, ParseError> {
    let decoded = decode_subscription_body(body)?;

    let nodes: Vec<VlessNode> = decoded
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter(|line| line.starts_with("vless://"))
        .filter_map(|line| parse_vless_uri(line).ok())
        .collect();

    if nodes.is_empty() {
        return Err(ParseError::NoSupportedNodes);
    }
    Ok(nodes)
}

/// base64 (обычный или url-safe, с padding и без) → текст. Если это уже текст со
/// ссылками — возвращаем как есть.
fn decode_subscription_body(body: &str) -> Result<String, ParseError> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return Err(ParseError::UndecodableSubscription);
    }

    // Панель нередко отдаёт уже готовый список без base64.
    if trimmed.contains("://") {
        return Ok(trimmed.to_string());
    }

    use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};

    let compact: String = trimmed.split_whitespace().collect();
    for engine in [
        &STANDARD as &dyn DynEngine,
        &STANDARD_NO_PAD,
        &URL_SAFE,
        &URL_SAFE_NO_PAD,
    ] {
        if let Some(text) = engine.try_decode_utf8(&compact) {
            if text.contains("://") {
                return Ok(text);
            }
        }
    }

    Err(ParseError::UndecodableSubscription)
}

/// Небольшая обёртка, чтобы перебрать несколько base64-алфавитов одним циклом.
trait DynEngine {
    fn try_decode_utf8(&self, input: &str) -> Option<String>;
}

impl<T: base64::Engine> DynEngine for T {
    fn try_decode_utf8(&self, input: &str) -> Option<String> {
        self.decode(input)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine as _;

    /// Ссылка того же вида, что отдаёт панель. Значения — из RFC 5737/документации:
    /// настоящие UUID и ключи REALITY в репозиторий класть нельзя, это доступ к серверу.
    const REALITY_URI: &str = "vless://00000000-1111-2222-3333-444444444444@203.0.113.10:10443?type=tcp&security=reality&pbk=EXAMPLEPublicKeyForTestsOnly0000000000000000&fp=chrome&sni=example.com&sid=0123456789abcdef&spx=%2F&flow=xtls-rprx-vision#%F0%9F%87%BA%F0%9F%87%B8%20%D0%A1%D0%A8%D0%90";

    #[test]
    fn parses_reality_uri_fully() {
        let node = parse_vless_uri(REALITY_URI).expect("должен распарситься");

        assert_eq!(node.uuid, "00000000-1111-2222-3333-444444444444");
        assert_eq!(node.address, "203.0.113.10");
        assert_eq!(node.port, 10443);
        assert_eq!(node.network, "tcp");
        assert_eq!(node.flow.as_deref(), Some("xtls-rprx-vision"));
        // Фрагмент должен быть декодирован из percent-encoding.
        assert_eq!(node.remark, "🇺🇸 США");
        assert!(node.is_reality());

        match node.security {
            Security::Reality {
                server_name,
                public_key,
                short_id,
                fingerprint,
                spider_x,
            } => {
                assert_eq!(server_name, "example.com");
                assert_eq!(public_key, "EXAMPLEPublicKeyForTestsOnly0000000000000000");
                assert_eq!(short_id.as_deref(), Some("0123456789abcdef"));
                assert_eq!(fingerprint, "chrome");
                assert_eq!(spider_x.as_deref(), Some("/"));
            }
            other => panic!("ожидался REALITY, получено {other:?}"),
        }
    }

    #[test]
    fn defaults_fingerprint_to_chrome_when_absent() {
        let uri = "vless://uuid-1@example.com:443?security=reality&pbk=KEY&sni=example.org";
        let node = parse_vless_uri(uri).unwrap();
        match node.security {
            Security::Reality { fingerprint, .. } => assert_eq!(fingerprint, "chrome"),
            other => panic!("ожидался REALITY, получено {other:?}"),
        }
    }

    #[test]
    fn falls_back_to_host_port_remark_without_fragment() {
        let uri = "vless://uuid-1@example.com:8443?security=none";
        let node = parse_vless_uri(uri).unwrap();
        assert_eq!(node.remark, "example.com:8443");
        assert_eq!(node.security, Security::None);
    }

    #[test]
    fn parses_plain_tls_node() {
        let uri = "vless://uuid-1@example.com:443?security=tls&sni=example.com&alpn=h2,http/1.1&allowInsecure=1";
        let node = parse_vless_uri(uri).unwrap();
        match node.security {
            Security::Tls {
                sni,
                alpn,
                allow_insecure,
                ..
            } => {
                assert_eq!(sni.as_deref(), Some("example.com"));
                assert_eq!(alpn, vec!["h2".to_string(), "http/1.1".to_string()]);
                assert!(allow_insecure);
            }
            other => panic!("ожидался TLS, получено {other:?}"),
        }
    }

    #[test]
    fn parses_ipv6_host() {
        let uri = "vless://uuid-1@[2001:db8::1]:443?security=reality&pbk=KEY&sni=a.example";
        let node = parse_vless_uri(uri).unwrap();
        assert_eq!(node.address, "[2001:db8::1]");
        assert_eq!(node.port, 443);
    }

    #[test]
    fn rejects_reality_without_public_key() {
        let uri = "vless://uuid-1@example.com:443?security=reality&sni=example.org";
        assert_eq!(
            parse_vless_uri(uri).unwrap_err(),
            ParseError::MissingRealityPublicKey
        );
    }

    #[test]
    fn rejects_reality_without_sni() {
        let uri = "vless://uuid-1@example.com:443?security=reality&pbk=KEY";
        assert_eq!(
            parse_vless_uri(uri).unwrap_err(),
            ParseError::MissingRealityServerName
        );
    }

    #[test]
    fn rejects_missing_uuid_and_port() {
        assert_eq!(
            parse_vless_uri("vless://@example.com:443").unwrap_err(),
            ParseError::MissingUuid
        );
        assert_eq!(
            parse_vless_uri("vless://uuid-1@example.com").unwrap_err(),
            ParseError::MissingPort
        );
    }

    #[test]
    fn rejects_other_schemes() {
        assert_eq!(
            parse_vless_uri("vmess://whatever").unwrap_err(),
            ParseError::UnsupportedScheme("vmess".to_string())
        );
    }

    #[test]
    fn parses_base64_subscription_with_several_nodes() {
        let plain = format!(
            "{REALITY_URI}\nvless://uuid-2@example.com:443?security=tls&sni=example.com#Node%202\n"
        );
        let encoded = STANDARD.encode(plain.as_bytes());

        let nodes = parse_subscription(&encoded).unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].remark, "🇺🇸 США");
        assert_eq!(nodes[1].remark, "Node 2");
    }

    #[test]
    fn parses_base64_split_across_lines() {
        // Некоторые панели переносят base64 по строкам — склейка обязательна.
        let plain = format!("{REALITY_URI}\n");
        let encoded = STANDARD.encode(plain.as_bytes());
        let wrapped = encoded
            .as_bytes()
            .chunks(40)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect::<Vec<_>>()
            .join("\n");

        let nodes = parse_subscription(&wrapped).unwrap();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn parses_plaintext_subscription() {
        let body = format!("{REALITY_URI}\n");
        let nodes = parse_subscription(&body).unwrap();
        assert_eq!(nodes.len(), 1);
    }

    #[test]
    fn skips_unsupported_protocols_but_keeps_vless() {
        let plain = format!("trojan://x@y:443#t\n{REALITY_URI}\nss://abc#s\n");
        let nodes = parse_subscription(&plain).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].address, "203.0.113.10");
    }

    #[test]
    fn errors_when_no_vless_nodes_present() {
        let plain = "trojan://x@y:443#t\nss://abc#s\n";
        assert_eq!(
            parse_subscription(plain).unwrap_err(),
            ParseError::NoSupportedNodes
        );
    }

    #[test]
    fn errors_on_garbage_body() {
        assert_eq!(
            parse_subscription("не base64 и не ссылки").unwrap_err(),
            ParseError::UndecodableSubscription
        );
        assert_eq!(
            parse_subscription("   ").unwrap_err(),
            ParseError::UndecodableSubscription
        );
    }
}
