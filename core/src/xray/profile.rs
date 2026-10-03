//! Профили подписки в формате Happ: JSON-массив готовых конфигов xray.
//!
//! Именно в таком виде панель отдаёт серверы с настройками, которых нет в обычной ссылке (фрагментация
//! ClientHello, sockopt и т. п.), и профиль «AUTO» — с балансировщиками и проверкой доступности серверов.
//! Запустить профиль можно как есть, подменив только локальные inbound'ы приложения.

use serde_json::Value;

use super::parser::ParseError;

#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub remark: String,
    pub config: Value,
}

fn is_proxy_protocol(outbound: &Value) -> bool {
    !matches!(
        outbound.get("protocol").and_then(Value::as_str),
        Some("freedom" | "blackhole" | "dns" | "loopback") | None
    )
}

impl Profile {
    /// Профиль-балансировщик (панельный «AUTO»): не сервер, а набор серверов с выбором лучшего.
    /// Признак — наблюдатель за серверами или балансировщик, под селектор которого попадает больше одного
    /// сервера. У обычного сервера панель тоже кладёт балансировщики (`Main_Balancer`, `MSK_Balancer`),
    /// но каждый смотрит ровно на один исходящий — это не «AUTO».
    pub fn is_balanced(&self) -> bool {
        if self.config.get("burstObservatory").is_some() || self.config.get("observatory").is_some()
        {
            return true;
        }
        let Some(balancers) = self
            .config
            .pointer("/routing/balancers")
            .and_then(Value::as_array)
        else {
            return false;
        };
        balancers.iter().any(|balancer| {
            let selectors: Vec<&str> = balancer
                .get("selector")
                .and_then(Value::as_array)
                .map(|list| list.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let matched = self
                .outbounds()
                .iter()
                .filter(|outbound| is_proxy_protocol(outbound))
                .filter(|outbound| {
                    let tag = outbound
                        .get("tag")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    selectors.iter().any(|selector| tag.starts_with(selector))
                })
                .count();
            matched > 1
        })
    }

    /// Сервер для YouTube без рекламы: по названию, так же узнаёт их и панель.
    pub fn is_youtube(&self) -> bool {
        self.remark.to_lowercase().contains("youtube")
    }

    fn outbounds(&self) -> &[Value] {
        self.config
            .get("outbounds")
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Основной исходящий сервера: с тегом `proxy`, иначе первый vless/wireguard/trojan/…
    pub fn proxy_outbound(&self) -> Option<&Value> {
        let outbounds = self.outbounds();
        outbounds
            .iter()
            .find(|o| o.get("tag").and_then(Value::as_str) == Some("proxy"))
            .or_else(|| {
                outbounds.iter().find(|o| {
                    !matches!(
                        o.get("protocol").and_then(Value::as_str),
                        Some("freedom" | "blackhole" | "dns" | "loopback") | None
                    )
                })
            })
    }

    /// `vless`, `wireguard`, … — протокол основного исходящего.
    pub fn protocol(&self) -> &str {
        self.proxy_outbound()
            .and_then(|o| o.get("protocol"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
    }

    pub fn is_reality(&self) -> bool {
        self.proxy_outbound()
            .and_then(|o| o.pointer("/streamSettings/security"))
            .and_then(Value::as_str)
            == Some("reality")
    }

    /// Адрес и порт сервера (для TCP-пинга). У WireGuard порт UDP — пингуем 443 того же хоста.
    pub fn endpoint(&self) -> Option<(String, u16)> {
        let outbound = self.proxy_outbound()?;
        match outbound.get("protocol").and_then(Value::as_str)? {
            "wireguard" => {
                let endpoint = outbound.pointer("/settings/peers/0/endpoint")?.as_str()?;
                let host = endpoint
                    .rsplit_once(':')
                    .map(|(h, _)| h)
                    .unwrap_or(endpoint);
                Some((host.trim_matches(['[', ']']).to_string(), 443))
            }
            _ => {
                let server = outbound
                    .pointer("/settings/vnext/0")
                    .or_else(|| outbound.pointer("/settings/servers/0"))?;
                Some((
                    server.get("address")?.as_str()?.to_string(),
                    u16::try_from(server.get("port")?.as_u64()?).ok()?,
                ))
            }
        }
    }
}

/// Разбирает тело подписки, если это JSON-массив профилей. `Ok(None)` — это не JSON (обычный список ссылок).
pub fn parse_profiles(body: &str) -> Result<Option<Vec<Profile>>, ParseError> {
    let trimmed = body.trim_start();
    if !trimmed.starts_with('[') {
        return Ok(None);
    }
    let parsed: Value =
        serde_json::from_str(trimmed).map_err(|_| ParseError::UndecodableSubscription)?;
    let Some(items) = parsed.as_array() else {
        return Ok(None);
    };
    let profiles: Vec<Profile> = items
        .iter()
        .filter(|item| {
            item.get("outbounds")
                .and_then(Value::as_array)
                .is_some_and(|o| !o.is_empty())
        })
        .map(|item| Profile {
            remark: item
                .get("remarks")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
            config: item.clone(),
        })
        .collect();
    if profiles.is_empty() {
        return Err(ParseError::NoSupportedNodes);
    }
    Ok(Some(profiles))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub fn vless_profile(remark: &str, address: &str) -> Value {
        json!({
            "remarks": remark,
            "inbounds": [{"tag": "socks", "port": 10808, "protocol": "socks"}],
            "outbounds": [
                {"tag": "proxy", "protocol": "vless",
                 "settings": {"vnext": [{"address": address, "port": 443, "users": [{"id": "00000000-1111-2222-3333-444444444444", "encryption": "none"}]}]},
                 "streamSettings": {"network": "tcp", "security": "reality", "realitySettings": {"serverName": "x.com", "publicKey": "0Dl1mJ7dY0uUe6Yx3Zq2m3X7gqQ0m1b4p9sJ8oA2lFY", "shortId": "0123abcd"}}},
                {"tag": "direct", "protocol": "freedom"},
                {"tag": "block", "protocol": "blackhole"}
            ],
            "routing": {"rules": [{"network": "tcp,udp", "outboundTag": "proxy"}]}
        })
    }

    pub fn wireguard_profile(remark: &str, host: &str) -> Value {
        json!({
            "remarks": remark,
            "outbounds": [
                {"tag": "proxy", "protocol": "wireguard", "settings": {
                    "secretKey": "AAAA", "address": ["10.66.66.2/32"],
                    "peers": [{"publicKey": "BBBB", "endpoint": format!("{host}:51820"), "allowedIPs": ["0.0.0.0/0"]}], "mtu": 1420}},
                {"tag": "direct", "protocol": "freedom"}
            ],
            "routing": {"rules": [{"network": "tcp,udp", "outboundTag": "proxy"}]}
        })
    }

    pub fn auto_profile() -> Value {
        json!({
            "remarks": "🇪🇺🌍 AUTO🌍",
            "outbounds": [
                {"tag": "proxy", "protocol": "vless", "settings": {"vnext": [{"address": "203.0.113.1", "port": 443, "users": [{"id": "00000000-1111-2222-3333-444444444444", "encryption": "none"}]}]}},
                {"tag": "youtube", "protocol": "vless", "settings": {"vnext": [{"address": "203.0.113.2", "port": 443, "users": [{"id": "00000000-1111-2222-3333-444444444444", "encryption": "none"}]}]}},
                {"tag": "direct", "protocol": "freedom"}, {"tag": "block", "protocol": "blackhole"}
            ],
            "routing": {"balancers": [{"tag": "Super_Balancer", "selector": ["proxy"]}, {"tag": "YouTube_Balancer", "selector": ["youtube"]}],
                        "rules": [{"domain": ["geosite:youtube"], "balancerTag": "YouTube_Balancer"}, {"network": "tcp,udp", "balancerTag": "Super_Balancer"}]},
            "burstObservatory": {"subjectSelector": ["proxy", "youtube"], "pingConfig": {"destination": "http://www.gstatic.com/generate_204", "interval": "1m", "timeout": "3s", "sampling": 1}}
        })
    }

    /// Серверный профиль панели: исходящие `main` и `msk`, у каждого свой балансировщик на один сервер.
    fn panel_server_profile() -> Value {
        let user = json!({"id": "00000000-1111-2222-3333-444444444444", "encryption": "none"});
        json!({
            "remarks": "🇨🇿 Чехия ",
            "outbounds": [
                {"tag": "main", "protocol": "vless", "settings": {"vnext": [{"address": "203.0.113.5", "port": 443, "users": [user]}]}},
                {"tag": "msk", "protocol": "vless", "settings": {"vnext": [{"address": "203.0.113.6", "port": 443, "users": [user]}]}},
                {"tag": "direct", "protocol": "freedom"}
            ],
            "routing": {
                "balancers": [
                    {"tag": "Main_Balancer", "selector": ["main"], "strategy": {"type": "random"}},
                    {"tag": "MSK_Balancer", "selector": ["msk"], "strategy": {"type": "random"}}
                ],
                "rules": [{"domain": ["domain:zzluocup.site"], "balancerTag": "MSK_Balancer"}, {"network": "tcp,udp", "balancerTag": "Main_Balancer"}]
            }
        })
    }

    #[test]
    fn panel_server_profile_with_single_server_balancers_is_a_server_not_auto() {
        let server = Profile {
            remark: "x".into(),
            config: panel_server_profile(),
        };
        assert!(!server.is_balanced());
        assert_eq!(server.endpoint(), Some(("203.0.113.5".to_string(), 443)));
        assert!(Profile {
            remark: "AUTO".into(),
            config: auto_profile()
        }
        .is_balanced());
    }

    #[test]
    fn balancer_over_several_servers_without_observatory_is_auto() {
        let mut config = auto_profile();
        config.as_object_mut().unwrap().remove("burstObservatory");
        let second = config["outbounds"][0].clone();
        config["outbounds"].as_array_mut().unwrap().push(second);
        config["outbounds"][4]["tag"] = json!("proxy-2");
        assert!(Profile {
            remark: "AUTO".into(),
            config
        }
        .is_balanced());
    }

    #[test]
    fn plain_link_lists_are_not_profiles() {
        assert_eq!(parse_profiles("vless://x@h:1#a").unwrap(), None);
        assert_eq!(parse_profiles("").unwrap(), None);
    }

    #[test]
    fn parses_servers_balanced_profile_and_youtube_names() {
        let body = serde_json::to_string(&json!([
            auto_profile(),
            vless_profile("🇨🇿 Чехия", "203.0.113.10"),
            vless_profile("🇳🇱 YOUTUBE Без рекламы #3", "203.0.113.20"),
            wireguard_profile("🇩🇪 Germany", "203.0.113.30"),
            {"remarks": "no outbounds"}
        ]))
        .unwrap();
        let profiles = parse_profiles(&body).unwrap().unwrap();
        assert_eq!(profiles.len(), 4); // профиль без outbounds отброшен

        assert!(profiles[0].is_balanced());
        assert!(!profiles[1].is_balanced());
        assert!(profiles[2].is_youtube() && !profiles[1].is_youtube());
        assert_eq!(profiles[1].protocol(), "vless");
        assert!(profiles[1].is_reality());
        assert_eq!(profiles[3].protocol(), "wireguard");
        assert!(!profiles[3].is_reality());
    }

    #[test]
    fn endpoints_for_ping() {
        assert_eq!(
            Profile {
                remark: "a".into(),
                config: vless_profile("a", "203.0.113.10")
            }
            .endpoint(),
            Some(("203.0.113.10".to_string(), 443))
        );
        assert_eq!(
            Profile {
                remark: "b".into(),
                config: wireguard_profile("b", "203.0.113.30")
            }
            .endpoint(),
            Some(("203.0.113.30".to_string(), 443))
        );
    }

    #[test]
    fn empty_or_broken_json_is_an_error() {
        assert!(parse_profiles("[]").is_err());
        assert!(parse_profiles("[{\"remarks\":\"x\"}]").is_err());
        assert!(parse_profiles("[ not json").is_err());
    }
}
