//! Сборка client-конфига xray-core из разобранного узла подписки.
//!
//! Форма outbound'а сверена с рабочим конфигом, которым вручную проверялась
//! связность через REALITY-ноду: inbound socks/http на localhost, vless-outbound
//! с realitySettings, freedom для прямого трафика и blackhole для рекламы.

use serde_json::{json, Map, Value};

use super::node::Node;
use super::parser::{Security, VlessNode};
use super::routing;
use super::wireguard::WgNode;

/// Как трафик попадает в туннель: через системный прокси (только приложения, которые его уважают) или
/// через виртуальный сетевой адаптер (весь трафик системы, нужны права администратора).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TunnelMode {
    #[default]
    Proxy,
    Tun,
}

#[derive(Debug, Clone)]
pub struct ConfigOptions {
    pub socks_port: u16,
    pub http_port: u16,
    /// Домены для блокировки (адблок). Пустой список — фича выключена.
    pub adblock_domains: Vec<String>,
    /// Домены-исключения: идут через туннель как обычно, даже если попали в блок-лист.
    pub adblock_allow_domains: Vec<String>,
    /// Пускать трафик к приватным адресам мимо туннеля (принтеры, роутер, NAS).
    pub bypass_private: bool,
    pub log_level: String,
    /// Куда xray пишет журнал доступа. Нужен только основному туннелю — по нему считается
    /// счётчик заблокированной рекламы; проверочные запуски журнал не пишут.
    pub access_log_path: Option<String>,
    pub mode: TunnelMode,
    /// Процессы, которые всегда идут напрямую (`chrome.exe`, `steam`, полные пути, `папка/`).
    pub direct_processes: Vec<String>,
    /// `Some` — режим «только выбранные приложения через VPN», остальные напрямую.
    pub vpn_only_processes: Option<Vec<String>>,
    /// Российские сайты (.ru, .su, .рф) напрямую — для собственного балансировщика, как в панельном «AUTO».
    pub bypass_ru: bool,
    /// Android: TUN-интерфейс создал `VpnService`, а его дескриптор передан xray через `XRAY_TUN_FD`.
    /// Маршруты и интерфейс xray не настраивает — только читает пакеты.
    pub android_tun: bool,
}

impl Default for ConfigOptions {
    fn default() -> Self {
        Self {
            socks_port: 10808,
            http_port: 10809,
            adblock_domains: Vec::new(),
            adblock_allow_domains: Vec::new(),
            bypass_private: true,
            log_level: "warning".to_string(),
            access_log_path: None,
            mode: TunnelMode::Proxy,
            direct_processes: Vec::new(),
            vpn_only_processes: None,
            bypass_ru: true,
            android_tun: false,
        }
    }
}

/// Собирает полный конфиг xray для одного выбранного узла.
pub fn build_config(node: &VlessNode, opts: &ConfigOptions) -> Value {
    json!({
        "log": build_log(opts),
        "inbounds": build_inbounds(opts),
        "outbounds": build_outbounds(node),
        "routing": {
            "domainStrategy": "IPIfNonMatch",
            "rules": build_routing_rules(opts),
        }
    })
}

/// Собирает конфиг xray для узла любого поддерживаемого протокола.
pub fn build_node_config(node: &Node, opts: &ConfigOptions) -> Value {
    match node {
        Node::Vless(vless) => build_config(vless, opts),
        Node::WireGuard(wg) => json!({
            "log": build_log(opts),
            "inbounds": build_inbounds(opts),
            "outbounds": build_wireguard_outbounds(wg),
            "routing": {
                "domainStrategy": "IPIfNonMatch",
                "rules": build_routing_rules(opts),
            }
        }),
        Node::Profile(profile) => routing::profile_config(profile, opts),
    }
}

/// WireGuard как outbound xray (пользовательское пространство, без TUN-драйвера) —
/// та же форма, что панель отдаёт Happ в JSON-профиле.
pub(super) fn build_wireguard_outbounds(node: &WgNode) -> Value {
    json!([
        {
            "tag": "proxy",
            "protocol": "wireguard",
            "settings": {
                "secretKey": node.private_key,
                "address": [node.address],
                "peers": [{
                    "publicKey": node.public_key,
                    "endpoint": format!("{}:{}", node.server_address, node.port),
                    "allowedIPs": ["0.0.0.0/0", "::/0"],
                    "keepAlive": 25,
                }],
                "mtu": 1420,
            },
        },
        { "tag": "direct", "protocol": "freedom", "settings": { "domainStrategy": "UseIP" } },
        { "tag": "block", "protocol": "blackhole", "settings": { "response": { "type": "none" } } }
    ])
}

pub(super) fn build_log(opts: &ConfigOptions) -> Value {
    let mut log = serde_json::Map::new();
    log.insert("loglevel".into(), json!(opts.log_level));
    if let Some(path) = &opts.access_log_path {
        log.insert("access".into(), json!(path));
    }
    Value::Object(log)
}

pub(super) fn build_inbounds(opts: &ConfigOptions) -> Value {
    let mut inbounds = json!([
        {
            "tag": "socks-in",
            "listen": "127.0.0.1",
            "port": opts.socks_port,
            "protocol": "socks",
            "settings": { "udp": true, "auth": "noauth" },
            "sniffing": { "enabled": true, "destOverride": ["http", "tls", "quic"] }
        },
        {
            "tag": "http-in",
            "listen": "127.0.0.1",
            "port": opts.http_port,
            "protocol": "http",
            "sniffing": { "enabled": true, "destOverride": ["http", "tls", "quic"] }
        }
    ]);
    if opts.android_tun {
        inbounds
            .as_array_mut()
            .expect("массив inbound'ов")
            .push(json!({
                "tag": "tun-in",
                "port": 0,
                "protocol": "tun",
                "settings": { "name": "tun0", "mtu": 1500 },
                "sniffing": { "enabled": true, "destOverride": ["http", "tls", "quic"] }
            }));
    } else if opts.mode == TunnelMode::Tun {
        // xray сам создаёт адаптер (wintun), назначает ему адрес и DNS, прописывает маршруты и привязывает
        // собственные исходящие к физическому интерфейсу — поэтому петли «туннель через туннель» нет.
        inbounds
            .as_array_mut()
            .expect("массив inbound'ов")
            .push(json!({
                "tag": "tun-in",
                "port": 0,
                "protocol": "tun",
                "settings": {
                    "name": "ZexorVPN",
                    "mtu": 1500,
                    "gateway": ["198.18.0.1/15"],
                    "dns": ["1.1.1.1", "8.8.8.8"],
                    "autoSystemRoutingTable": ["0.0.0.0/0", "::/0"],
                    "autoOutboundsInterface": "auto"
                },
                "sniffing": { "enabled": true, "destOverride": ["http", "tls", "quic"] }
            }));
    }
    inbounds
}

pub(super) fn build_outbounds(node: &VlessNode) -> Value {
    let mut user = Map::new();
    user.insert("id".into(), json!(node.uuid));
    user.insert("encryption".into(), json!("none"));
    if let Some(flow) = &node.flow {
        user.insert("flow".into(), json!(flow));
    }

    json!([
        {
            "tag": "proxy",
            "protocol": "vless",
            "settings": {
                "vnext": [{
                    "address": node.address,
                    "port": node.port,
                    "users": [Value::Object(user)],
                }]
            },
            "streamSettings": build_stream_settings(node),
        },
        { "tag": "direct", "protocol": "freedom", "settings": { "domainStrategy": "UseIP" } },
        { "tag": "block", "protocol": "blackhole", "settings": { "response": { "type": "none" } } }
    ])
}

fn build_stream_settings(node: &VlessNode) -> Value {
    let mut stream = Map::new();
    stream.insert("network".into(), json!(node.network));

    match &node.security {
        Security::Reality {
            server_name,
            public_key,
            short_id,
            fingerprint,
            spider_x,
        } => {
            stream.insert("security".into(), json!("reality"));
            let mut reality = Map::new();
            reality.insert("serverName".into(), json!(server_name));
            reality.insert("publicKey".into(), json!(public_key));
            reality.insert("fingerprint".into(), json!(fingerprint));
            // Пустой shortId допустим только если сервер сконфигурирован с "" —
            // но панель всегда отдаёт конкретный sid, поэтому кладём его как есть.
            reality.insert(
                "shortId".into(),
                json!(short_id.clone().unwrap_or_default()),
            );
            if let Some(spx) = spider_x {
                reality.insert("spiderX".into(), json!(spx));
            }
            stream.insert("realitySettings".into(), Value::Object(reality));
        }
        Security::Tls {
            sni,
            fingerprint,
            alpn,
            allow_insecure,
        } => {
            stream.insert("security".into(), json!("tls"));
            let mut tls = Map::new();
            if let Some(sni) = sni {
                tls.insert("serverName".into(), json!(sni));
            }
            if let Some(fp) = fingerprint {
                tls.insert("fingerprint".into(), json!(fp));
            }
            if !alpn.is_empty() {
                tls.insert("alpn".into(), json!(alpn));
            }
            tls.insert("allowInsecure".into(), json!(allow_insecure));
            stream.insert("tlsSettings".into(), Value::Object(tls));
        }
        Security::None => {
            stream.insert("security".into(), json!("none"));
        }
    }

    match node.network.as_str() {
        "ws" => {
            let mut ws = Map::new();
            ws.insert(
                "path".into(),
                json!(node
                    .transport
                    .get("path")
                    .cloned()
                    .unwrap_or_else(|| "/".into())),
            );
            if let Some(host) = node.transport.get("host") {
                ws.insert("headers".into(), json!({ "Host": host }));
            }
            stream.insert("wsSettings".into(), Value::Object(ws));
        }
        "grpc" => {
            let mut grpc = Map::new();
            if let Some(service) = node.transport.get("serviceName") {
                grpc.insert("serviceName".into(), json!(service));
            }
            grpc.insert(
                "multiMode".into(),
                json!(node
                    .transport
                    .get("mode")
                    .map(|m| m == "multi")
                    .unwrap_or(false)),
            );
            stream.insert("grpcSettings".into(), Value::Object(grpc));
        }
        _ => {}
    }

    Value::Object(stream)
}

fn build_routing_rules(opts: &ConfigOptions) -> Value {
    let proxy = json!({ "outboundTag": "proxy" });

    // Пользовательские правила (адблок, приложения) идут раньше всего прочего.
    let (mut rules, terminal) = routing::user_rules(opts, &proxy);
    if terminal {
        return Value::Array(rules);
    }

    // Локальная сеть и приватные адреса — мимо туннеля.
    if opts.bypass_private {
        rules.push(json!({
            "type": "field",
            "ip": ["geoip:private"],
            "outboundTag": "direct",
        }));
    }

    // Всё остальное — в VPN.
    rules.push(json!({
        "type": "field",
        "network": "tcp,udp",
        "outboundTag": "proxy",
    }));

    Value::Array(rules)
}

/// Приводит строку блок-листа к правилу xray.
///
/// Пустые строки и комментарии отбрасываются. Голый домен превращается в
/// `domain:example.com` (совпадение по поддоменам), готовые префиксы
/// (`domain:` / `full:` / `regexp:` / `geosite:` / `keyword:`) остаются как есть.
pub fn normalize_domain_rule(raw: &str) -> String {
    let line = raw.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
        return String::new();
    }
    if line.contains(':') {
        return line.to_string();
    }
    format!("domain:{line}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xray::parser::parse_vless_uri;

    const REALITY_URI: &str = "vless://00000000-1111-2222-3333-444444444444@203.0.113.10:10443?type=tcp&security=reality&pbk=EXAMPLEPublicKeyForTestsOnly0000000000000000&fp=chrome&sni=example.com&sid=0123456789abcdef&spx=%2F&flow=xtls-rprx-vision#US";

    fn reality_config(opts: ConfigOptions) -> Value {
        let node = parse_vless_uri(REALITY_URI).unwrap();
        build_config(&node, &opts)
    }

    #[test]
    fn access_log_is_written_only_when_a_path_is_given() {
        let cfg = reality_config(ConfigOptions::default());
        assert!(cfg["log"].get("access").is_none());

        let cfg = reality_config(ConfigOptions {
            access_log_path: Some("C:\\data\\access.log".into()),
            ..ConfigOptions::default()
        });
        assert_eq!(cfg["log"]["access"], "C:\\data\\access.log");
    }

    #[test]
    fn allow_rules_come_before_block_rules() {
        let opts = ConfigOptions {
            adblock_domains: vec!["ads.example.com".into()],
            adblock_allow_domains: vec!["good.example.com".into()],
            ..ConfigOptions::default()
        };
        let cfg = reality_config(opts);
        let rules = cfg["routing"]["rules"].as_array().unwrap();
        assert_eq!(rules[0]["outboundTag"], "proxy");
        assert_eq!(rules[0]["domain"][0], "domain:good.example.com");
        assert_eq!(rules[1]["outboundTag"], "block");
    }

    #[test]
    fn builds_wireguard_outbound_like_the_panel_profile() {
        use crate::xray::wireguard::parse_wireguard_uri;
        let wg = parse_wireguard_uri(
            "wireguard://AAAA%2BBBBB%3D@203.0.113.5:51820?publickey=ZZZZ%3D&address=10.66.66.2%2F32#DE",
        )
        .unwrap();
        let cfg = build_node_config(&Node::WireGuard(wg), &ConfigOptions::default());
        let proxy = &cfg["outbounds"][0];
        assert_eq!(proxy["protocol"], "wireguard");
        assert_eq!(proxy["tag"], "proxy");
        assert_eq!(proxy["settings"]["secretKey"], "AAAA+BBBB=");
        assert_eq!(proxy["settings"]["address"][0], "10.66.66.2/32");
        assert_eq!(proxy["settings"]["peers"][0]["publicKey"], "ZZZZ=");
        assert_eq!(
            proxy["settings"]["peers"][0]["endpoint"],
            "203.0.113.5:51820"
        );
        assert_eq!(proxy["settings"]["peers"][0]["allowedIPs"][0], "0.0.0.0/0");
        // inbounds и маршрутизация те же, что у VLESS: socks+http на localhost, остальное в proxy.
        assert_eq!(cfg["inbounds"][0]["tag"], "socks-in");
        let rules = cfg["routing"]["rules"].as_array().unwrap();
        assert_eq!(rules.last().unwrap()["outboundTag"], "proxy");
    }

    #[test]
    fn builds_reality_outbound_matching_verified_shape() {
        let cfg = reality_config(ConfigOptions::default());
        let proxy = &cfg["outbounds"][0];

        assert_eq!(proxy["protocol"], "vless");
        assert_eq!(proxy["tag"], "proxy");

        let vnext = &proxy["settings"]["vnext"][0];
        assert_eq!(vnext["address"], "203.0.113.10");
        assert_eq!(vnext["port"], 10443);
        assert_eq!(
            vnext["users"][0]["id"],
            "00000000-1111-2222-3333-444444444444"
        );
        assert_eq!(vnext["users"][0]["encryption"], "none");
        assert_eq!(vnext["users"][0]["flow"], "xtls-rprx-vision");

        let stream = &proxy["streamSettings"];
        assert_eq!(stream["network"], "tcp");
        assert_eq!(stream["security"], "reality");
        assert_eq!(stream["realitySettings"]["serverName"], "example.com");
        assert_eq!(
            stream["realitySettings"]["publicKey"],
            "EXAMPLEPublicKeyForTestsOnly0000000000000000"
        );
        assert_eq!(stream["realitySettings"]["shortId"], "0123456789abcdef");
        assert_eq!(stream["realitySettings"]["fingerprint"], "chrome");
        assert_eq!(stream["realitySettings"]["spiderX"], "/");
    }

    #[test]
    fn omits_flow_when_node_has_none() {
        let node = parse_vless_uri("vless://u@h:443?security=reality&pbk=K&sni=s.example").unwrap();
        let cfg = build_config(&node, &ConfigOptions::default());
        let user = &cfg["outbounds"][0]["settings"]["vnext"][0]["users"][0];
        assert!(
            user.get("flow").is_none(),
            "flow не должен появляться пустым"
        );
    }

    #[test]
    fn exposes_socks_and_http_inbounds_on_requested_ports() {
        let cfg = reality_config(ConfigOptions {
            socks_port: 11080,
            http_port: 11081,
            ..Default::default()
        });
        assert_eq!(cfg["inbounds"][0]["protocol"], "socks");
        assert_eq!(cfg["inbounds"][0]["port"], 11080);
        assert_eq!(cfg["inbounds"][0]["listen"], "127.0.0.1");
        assert_eq!(cfg["inbounds"][1]["protocol"], "http");
        assert_eq!(cfg["inbounds"][1]["port"], 11081);
    }

    #[test]
    fn always_declares_direct_and_block_outbounds() {
        let cfg = reality_config(ConfigOptions::default());
        assert_eq!(cfg["outbounds"][1]["tag"], "direct");
        assert_eq!(cfg["outbounds"][1]["protocol"], "freedom");
        assert_eq!(cfg["outbounds"][2]["tag"], "block");
        assert_eq!(cfg["outbounds"][2]["protocol"], "blackhole");
    }

    #[test]
    fn adblock_rule_comes_before_catch_all() {
        let cfg = reality_config(ConfigOptions {
            adblock_domains: vec!["doubleclick.net".into(), "googlesyndication.com".into()],
            ..Default::default()
        });
        let rules = cfg["routing"]["rules"].as_array().unwrap();

        assert_eq!(rules[0]["outboundTag"], "block");
        assert_eq!(
            rules[0]["domain"],
            json!(["domain:doubleclick.net", "domain:googlesyndication.com"])
        );
        // Последним всегда идёт "всё остальное — в туннель".
        let last = rules.last().unwrap();
        assert_eq!(last["outboundTag"], "proxy");
        assert_eq!(last["network"], "tcp,udp");
    }

    #[test]
    fn without_adblock_there_is_no_block_rule() {
        let cfg = reality_config(ConfigOptions::default());
        let rules = cfg["routing"]["rules"].as_array().unwrap();
        assert!(rules.iter().all(|r| r["outboundTag"] != "block"));
    }

    #[test]
    fn private_bypass_is_toggleable() {
        let with = reality_config(ConfigOptions::default());
        let rules = with["routing"]["rules"].as_array().unwrap();
        assert!(rules.iter().any(|r| r["ip"] == json!(["geoip:private"])));

        let without = reality_config(ConfigOptions {
            bypass_private: false,
            ..Default::default()
        });
        let rules = without["routing"]["rules"].as_array().unwrap();
        assert!(rules.iter().all(|r| r.get("ip").is_none()));
    }

    #[test]
    fn splits_huge_blocklists_into_several_rules() {
        let domains: Vec<String> = (0..2500).map(|i| format!("ads{i}.example")).collect();
        let cfg = reality_config(ConfigOptions {
            adblock_domains: domains,
            ..Default::default()
        });
        let block_rules: Vec<_> = cfg["routing"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["outboundTag"] == "block")
            .collect();
        assert_eq!(block_rules.len(), 3, "2500 доменов → 3 правила по 1000");
        assert_eq!(block_rules[2]["domain"].as_array().unwrap().len(), 500);
    }

    #[test]
    fn normalizes_blocklist_entries() {
        assert_eq!(normalize_domain_rule("example.com"), "domain:example.com");
        assert_eq!(
            normalize_domain_rule("  example.com  "),
            "domain:example.com"
        );
        assert_eq!(
            normalize_domain_rule("full:ads.example"),
            "full:ads.example"
        );
        assert_eq!(
            normalize_domain_rule("geosite:category-ads"),
            "geosite:category-ads"
        );
        assert_eq!(normalize_domain_rule("# комментарий"), "");
        assert_eq!(normalize_domain_rule("! adblock-комментарий"), "");
        assert_eq!(normalize_domain_rule(""), "");
    }

    #[test]
    fn skips_comment_lines_inside_rule_chunks() {
        let cfg = reality_config(ConfigOptions {
            adblock_domains: vec!["# заголовок".into(), "ads.example".into()],
            ..Default::default()
        });
        assert_eq!(
            cfg["routing"]["rules"][0]["domain"],
            json!(["domain:ads.example"])
        );
    }

    #[test]
    fn builds_ws_transport_settings() {
        let uri = "vless://u@h:443?type=ws&security=tls&sni=h&path=%2Fray&host=cdn.example";
        let node = parse_vless_uri(uri).unwrap();
        let cfg = build_config(&node, &ConfigOptions::default());
        let stream = &cfg["outbounds"][0]["streamSettings"];
        assert_eq!(stream["network"], "ws");
        assert_eq!(stream["wsSettings"]["path"], "/ray");
        assert_eq!(stream["wsSettings"]["headers"]["Host"], "cdn.example");
        assert_eq!(stream["tlsSettings"]["serverName"], "h");
    }
}
