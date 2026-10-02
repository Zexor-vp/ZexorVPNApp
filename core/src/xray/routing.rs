//! Правила маршрутизации поверх конфигов xray: адблок, приложения (напрямую / только через VPN),
//! запуск профилей подписки как есть и собственный балансировщик приложения.

use serde_json::{json, Value};

use super::config_builder::{
    build_inbounds, build_log, build_wireguard_outbounds, normalize_domain_rule, ConfigOptions,
    TunnelMode,
};
use super::node::Node;
use super::profile::Profile;

/// Сколько доменов класть в одно правило маршрутизации. Xray спокойно ест длинные списки,
/// но дробление упрощает чтение конфига и его диффы.
const DOMAINS_PER_RULE: usize = 1000;

/// Домены YouTube для собственного балансировщика: серверы «YouTube без рекламы» включаются на них сами.
/// Явный список, а не `geosite:youtube`, чтобы балансировщик не зависел от скачанных баз geosite.
const YOUTUBE_DOMAINS: &[&str] = &[
    "domain:youtube.com",
    "domain:youtu.be",
    "domain:googlevideo.com",
    "domain:ytimg.com",
    "domain:youtube-nocookie.com",
    "domain:ggpht.com",
    "domain:youtubei.googleapis.com",
];

/// Имена процессов для правила: xray сравнивает имя с учётом регистра (и без `.exe`), а Windows
/// регистр не различает — поэтому добавляем и вариант в нижнем регистре.
fn process_names(apps: &[String]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for app in apps.iter().map(|a| a.trim()).filter(|a| !a.is_empty()) {
        for variant in [app.to_string(), app.to_lowercase()] {
            if !names.contains(&variant) {
                names.push(variant);
            }
        }
    }
    names
}

/// Куда по умолчанию уходит трафик в конфиге: `{"outboundTag": ..}` или `{"balancerTag": ..}` последнего правила.
pub fn target_of(rule: &Value) -> Value {
    if let Some(tag) = rule.get("balancerTag") {
        json!({ "balancerTag": tag })
    } else {
        json!({ "outboundTag": rule.get("outboundTag").cloned().unwrap_or_else(|| json!("proxy")) })
    }
}

fn with_target(mut rule: Value, target: &Value) -> Value {
    if let (Some(rule_obj), Some(target_obj)) = (rule.as_object_mut(), target.as_object()) {
        for (key, value) in target_obj {
            rule_obj.insert(key.clone(), value.clone());
        }
    }
    rule
}

/// Правила адблока: сначала исключения пользователя (идут по умолчанию), потом блокировка в blackhole.
pub fn adblock_rules(opts: &ConfigOptions, default_target: &Value) -> Vec<Value> {
    let mut rules = Vec::new();

    let allow: Vec<String> = opts
        .adblock_allow_domains
        .iter()
        .map(|d| normalize_domain_rule(d))
        .filter(|d| !d.is_empty())
        .collect();
    if !allow.is_empty() && !opts.adblock_domains.is_empty() {
        rules.push(with_target(
            json!({ "type": "field", "domain": allow }),
            default_target,
        ));
    }

    for chunk in opts.adblock_domains.chunks(DOMAINS_PER_RULE) {
        let domains: Vec<String> = chunk
            .iter()
            .map(|d| normalize_domain_rule(d))
            .filter(|d| !d.is_empty())
            .collect();
        if !domains.is_empty() {
            rules.push(json!({ "type": "field", "domain": domains, "outboundTag": "block" }));
        }
    }
    rules
}

/// Все пользовательские правила, которые идут раньше правил самого профиля. Второй элемент — `true`,
/// если среди них есть «терминальное» («только выбранные через VPN»): остальные правила тогда не нужны.
pub fn user_rules(opts: &ConfigOptions, default_target: &Value) -> (Vec<Value>, bool) {
    let mut rules = Vec::new();

    // В TUN-режиме весь IPv6 режем: туннель его не несёт, а без блокировки приложения «зависали» бы на AAAA.
    if opts.mode == TunnelMode::Tun {
        rules.push(json!({ "type": "field", "ip": ["::/0"], "outboundTag": "block" }));
    }

    rules.extend(adblock_rules(opts, default_target));

    let direct: Vec<&String> = opts
        .direct_processes
        .iter()
        .filter(|p| !p.trim().is_empty())
        .collect();
    if !direct.is_empty() {
        rules.push(json!({ "type": "field", "process": direct, "outboundTag": "direct" }));
    }

    if let Some(only) = &opts.vpn_only_processes {
        let selected = process_names(only);
        if !selected.is_empty() {
            rules.push(with_target(
                json!({ "type": "field", "process": selected }),
                default_target,
            ));
        }
        // Всё остальное — напрямую: приложения вне списка VPN не касается.
        rules.push(json!({ "type": "field", "network": "tcp,udp", "outboundTag": "direct" }));
        return (rules, true);
    }
    (rules, false)
}

fn ensure_outbound(config: &mut Value, tag: &str, protocol: &str) {
    let Some(outbounds) = config.get_mut("outbounds").and_then(Value::as_array_mut) else {
        return;
    };
    if !outbounds
        .iter()
        .any(|o| o.get("tag").and_then(Value::as_str) == Some(tag))
    {
        outbounds.push(json!({ "tag": tag, "protocol": protocol }));
    }
}

/// Конфиг для профиля подписки: сам профиль (серверы, балансировщики, наблюдатель) остаётся как есть,
/// подменяются только локальные входы приложения и добавляются пользовательские правила.
pub fn profile_config(profile: &Profile, opts: &ConfigOptions) -> Value {
    let mut config = profile.config.clone();
    config["log"] = build_log(opts);
    config["inbounds"] = build_inbounds(opts);
    ensure_outbound(&mut config, "direct", "freedom");
    ensure_outbound(&mut config, "block", "blackhole");

    let own_rules: Vec<Value> = config
        .pointer("/routing/rules")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let default_target = own_rules
        .last()
        .map(target_of)
        .unwrap_or_else(|| json!({ "outboundTag": "proxy" }));

    let (mut rules, terminal) = user_rules(opts, &default_target);
    if !terminal {
        rules.extend(own_rules);
    }
    if !config.get("routing").is_some_and(Value::is_object) {
        config["routing"] = json!({ "domainStrategy": "IPIfNonMatch" });
    }
    config["routing"]["rules"] = Value::Array(rules);
    config
}

fn least_load() -> Value {
    json!({
        "type": "leastLoad",
        "settings": { "maxRTT": "2s", "expected": 2, "baselines": ["1s", "2s"], "tolerance": 0.3 }
    })
}

/// Основной исходящий узла для балансировщика, с новым тегом.
fn outbound_for(node: &Node, tag: &str) -> Option<Value> {
    let mut outbound = match node {
        Node::Profile(profile) if !profile.is_balanced() => profile.proxy_outbound()?.clone(),
        Node::Profile(_) => return None,
        Node::Vless(vless) => super::config_builder::build_outbounds(vless)
            .get(0)?
            .clone(),
        Node::WireGuard(wg) => build_wireguard_outbounds(wg).get(0)?.clone(),
    };
    outbound["tag"] = json!(tag);
    Some(outbound)
}

fn is_youtube_node(node: &Node) -> bool {
    node.remark().to_lowercase().contains("youtube")
}

/// Собственный балансировщик приложения (по образцу «AUTO» панели): обычные серверы объединены в
/// `Super_Balancer`, серверы «YouTube без рекламы» — в `YouTube_Balancer`, и YouTube сам уходит на них.
/// Лучший сервер выбирается по живым замерам (`burstObservatory`), а не один раз при подключении.
/// `None` — не из чего собирать.
pub fn build_balanced_config(nodes: &[Node], opts: &ConfigOptions) -> Option<Value> {
    let mut outbounds = Vec::new();
    let mut has_youtube = false;
    let (mut main_index, mut yt_index) = (0usize, 0usize);

    for node in nodes {
        if is_youtube_node(node) {
            if let Some(outbound) = outbound_for(node, &format!("youtube-{yt_index}")) {
                outbounds.push(outbound);
                yt_index += 1;
                has_youtube = true;
            }
        } else if let Some(outbound) = outbound_for(node, &format!("proxy-{main_index}")) {
            outbounds.push(outbound);
            main_index += 1;
        }
    }
    if main_index == 0 && yt_index == 0 {
        return None;
    }
    // Только YouTube-серверы (странный случай): они же и основные.
    let main_selector = if main_index > 0 { "proxy-" } else { "youtube-" };
    let main_fallback = if main_index > 0 {
        "proxy-0"
    } else {
        "youtube-0"
    };

    outbounds.push(json!({ "tag": "direct", "protocol": "freedom", "settings": { "domainStrategy": "UseIP" } }));
    outbounds.push(json!({ "tag": "block", "protocol": "blackhole", "settings": { "response": { "type": "none" } } }));

    let mut balancers = vec![json!({
        "tag": "Super_Balancer", "selector": [main_selector], "strategy": least_load(), "fallbackTag": main_fallback
    })];
    if has_youtube && main_index > 0 {
        balancers.push(json!({
            "tag": "YouTube_Balancer", "selector": ["youtube-"], "strategy": least_load(), "fallbackTag": "youtube-0"
        }));
    }

    let default_target = json!({ "balancerTag": "Super_Balancer" });
    let (mut rules, terminal) = user_rules(opts, &default_target);
    if !terminal {
        rules.push(json!({ "type": "field", "protocol": ["bittorrent"], "outboundTag": "direct" }));
        if opts.bypass_private {
            rules
                .push(json!({ "type": "field", "ip": ["geoip:private"], "outboundTag": "direct" }));
        }
        if opts.bypass_ru {
            // Российские сайты — напрямую (как в «AUTO»): госуслуги, банки и т. п. не должны видеть иностранный IP.
            rules.push(json!({ "type": "field", "domain": ["domain:ru", "domain:su", "domain:xn--p1ai"], "outboundTag": "direct" }));
        }
        if has_youtube && main_index > 0 {
            rules.push(json!({ "type": "field", "domain": YOUTUBE_DOMAINS, "balancerTag": "YouTube_Balancer" }));
        }
        rules.push(
            json!({ "type": "field", "network": "tcp,udp", "balancerTag": "Super_Balancer" }),
        );
    }

    let mut subjects = vec![main_selector];
    if has_youtube && main_index > 0 {
        subjects.push("youtube-");
    }

    Some(json!({
        "log": build_log(opts),
        "inbounds": build_inbounds(opts),
        "outbounds": outbounds,
        "routing": { "domainStrategy": "IPIfNonMatch", "balancers": balancers, "rules": rules },
        "burstObservatory": {
            "subjectSelector": subjects,
            "pingConfig": {
                "destination": "http://www.gstatic.com/generate_204",
                "interval": "1m",
                "connectivity": "",
                "timeout": "3s",
                "sampling": 1
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::xray::profile::tests::{auto_profile, vless_profile, wireguard_profile};

    fn profile_node(value: Value) -> Node {
        Node::Profile(Profile {
            remark: value["remarks"].as_str().unwrap().to_string(),
            config: value,
        })
    }

    fn rule_targets(config: &Value) -> Vec<Value> {
        config["routing"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(target_of)
            .collect()
    }

    #[test]
    fn profile_runs_as_is_but_with_local_inbounds_and_user_rules() {
        let profile = Profile {
            remark: "x".into(),
            config: vless_profile("x", "203.0.113.10"),
        };
        let opts = ConfigOptions {
            adblock_domains: vec!["ads.example.com".into()],
            direct_processes: vec!["steam.exe".into()],
            ..ConfigOptions::default()
        };
        let cfg = profile_config(&profile, &opts);

        assert_eq!(cfg["inbounds"][0]["tag"], "socks-in"); // входы приложения вместо входов профиля
        assert_eq!(cfg["outbounds"][0]["tag"], "proxy"); // сервер не тронут
        let rules = cfg["routing"]["rules"].as_array().unwrap();
        assert_eq!(rules[0]["outboundTag"], "block"); // реклама
        assert_eq!(rules[1]["process"][0], "steam.exe"); // приложение напрямую
        assert_eq!(rules[1]["outboundTag"], "direct");
        assert_eq!(rules.last().unwrap()["outboundTag"], "proxy"); // собственные правила профиля сохранены
    }

    #[test]
    fn balanced_profile_keeps_its_balancers_and_allow_rules_follow_its_default_target() {
        let profile = Profile {
            remark: "AUTO".into(),
            config: auto_profile(),
        };
        let opts = ConfigOptions {
            adblock_domains: vec!["ads.example.com".into()],
            adblock_allow_domains: vec!["good.example.com".into()],
            ..ConfigOptions::default()
        };
        let cfg = profile_config(&profile, &opts);
        assert!(cfg.get("burstObservatory").is_some());
        assert_eq!(cfg["routing"]["balancers"][0]["tag"], "Super_Balancer");
        // исключение «не блокировать» идёт туда же, куда весь остальной трафик профиля — в балансировщик
        assert_eq!(cfg["routing"]["rules"][0]["balancerTag"], "Super_Balancer");
        assert_eq!(cfg["routing"]["rules"][1]["outboundTag"], "block");
    }

    #[test]
    fn vpn_only_mode_routes_the_rest_directly_and_drops_profile_rules() {
        let profile = Profile {
            remark: "x".into(),
            config: vless_profile("x", "203.0.113.10"),
        };
        let opts = ConfigOptions {
            vpn_only_processes: Some(vec!["chrome.exe".into()]),
            ..ConfigOptions::default()
        };
        let cfg = profile_config(&profile, &opts);
        let rules = cfg["routing"]["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0]["process"][0], "chrome.exe");
        assert_eq!(rules[0]["outboundTag"], "proxy");
        assert_eq!(rules[1]["outboundTag"], "direct");
    }

    #[test]
    fn tun_mode_blocks_ipv6_first() {
        let profile = Profile {
            remark: "x".into(),
            config: vless_profile("x", "203.0.113.10"),
        };
        let cfg = profile_config(
            &profile,
            &ConfigOptions {
                mode: TunnelMode::Tun,
                ..ConfigOptions::default()
            },
        );
        assert_eq!(cfg["routing"]["rules"][0]["ip"][0], "::/0");
        assert_eq!(cfg["routing"]["rules"][0]["outboundTag"], "block");
        assert!(cfg["inbounds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["protocol"] == "tun"));
    }

    #[test]
    fn balancer_splits_youtube_servers_and_routes_youtube_to_them() {
        let nodes = vec![
            profile_node(vless_profile("🇨🇿 Чехия", "203.0.113.10")),
            profile_node(vless_profile("🇸🇪 Швеция", "203.0.113.11")),
            profile_node(vless_profile("🇳🇱 YOUTUBE Без рекламы #3", "203.0.113.20")),
            profile_node(auto_profile()), // панельный AUTO не берём
        ];
        let cfg = build_balanced_config(&nodes, &ConfigOptions::default()).unwrap();

        let tags: Vec<&str> = cfg["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["tag"].as_str().unwrap())
            .collect();
        assert_eq!(tags, ["proxy-0", "proxy-1", "youtube-0", "direct", "block"]);

        let balancers = cfg["routing"]["balancers"].as_array().unwrap();
        assert_eq!(balancers[0]["tag"], "Super_Balancer");
        assert_eq!(balancers[1]["tag"], "YouTube_Balancer");
        assert_eq!(balancers[1]["selector"][0], "youtube-");
        assert_eq!(
            cfg["burstObservatory"]["subjectSelector"],
            json!(["proxy-", "youtube-"])
        );

        let targets = rule_targets(&cfg);
        let yt_rule = cfg["routing"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["balancerTag"] == "YouTube_Balancer")
            .unwrap();
        assert!(yt_rule["domain"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d == "domain:googlevideo.com"));
        assert_eq!(targets.last().unwrap()["balancerTag"], "Super_Balancer");
    }

    #[test]
    fn balancer_without_youtube_servers_has_a_single_balancer_and_works_for_wireguard() {
        let nodes = vec![
            profile_node(wireguard_profile("🇩🇪 Germany", "203.0.113.30")),
            profile_node(wireguard_profile("🇺🇸 United States", "203.0.113.31")),
        ];
        let cfg = build_balanced_config(&nodes, &ConfigOptions::default()).unwrap();
        assert_eq!(cfg["routing"]["balancers"].as_array().unwrap().len(), 1);
        assert_eq!(cfg["outbounds"][0]["protocol"], "wireguard");
        assert_eq!(
            cfg["burstObservatory"]["subjectSelector"],
            json!(["proxy-"])
        );
        assert!(cfg["routing"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["balancerTag"] != "YouTube_Balancer"));
    }

    #[test]
    fn nothing_to_balance_gives_none() {
        assert!(build_balanced_config(&[], &ConfigOptions::default()).is_none());
        assert!(
            build_balanced_config(&[profile_node(auto_profile())], &ConfigOptions::default())
                .is_none()
        );
    }
}

#[cfg(test)]
mod dump {
    use super::*;
    use crate::xray::profile::tests::{auto_profile, vless_profile, wireguard_profile};

    /// `ZEXOR_DUMP_DIR=/tmp/x cargo test -p zexor-vpn-core dump_configs -- --ignored` — выгружает конфиги
    /// для проверки настоящим `xray run -test`.
    #[test]
    #[ignore = "ручная выгрузка конфигов"]
    fn dump_configs() {
        let dir = std::env::var("ZEXOR_DUMP_DIR").expect("ZEXOR_DUMP_DIR");
        let node = |v: Value| {
            Node::Profile(Profile {
                remark: v["remarks"].as_str().unwrap().into(),
                config: v,
            })
        };
        let opts = ConfigOptions {
            adblock_domains: vec!["ads.example.com".into()],
            adblock_allow_domains: vec!["ok.example.com".into()],
            direct_processes: vec!["steam.exe".into()],
            ..ConfigOptions::default()
        };
        let tun = ConfigOptions {
            mode: TunnelMode::Tun,
            ..opts.clone()
        };
        let only = ConfigOptions {
            vpn_only_processes: Some(vec!["chrome.exe".into()]),
            ..opts.clone()
        };
        let nodes = vec![
            node(vless_profile("A", "203.0.113.10")),
            node(vless_profile("YouTube B", "203.0.113.20")),
            node(wireguard_profile("W", "203.0.113.30")),
        ];
        let write = |name: &str, v: Value| {
            std::fs::write(format!("{dir}/{name}.json"), v.to_string()).unwrap()
        };
        write(
            "profile",
            profile_config(
                &Profile {
                    remark: "x".into(),
                    config: vless_profile("x", "203.0.113.10"),
                },
                &opts,
            ),
        );
        write(
            "auto",
            profile_config(
                &Profile {
                    remark: "x".into(),
                    config: auto_profile(),
                },
                &opts,
            ),
        );
        write(
            "tun",
            profile_config(
                &Profile {
                    remark: "x".into(),
                    config: auto_profile(),
                },
                &tun,
            ),
        );
        write(
            "only",
            profile_config(
                &Profile {
                    remark: "x".into(),
                    config: vless_profile("x", "203.0.113.10"),
                },
                &only,
            ),
        );
        write("balanced", build_balanced_config(&nodes, &opts).unwrap());
        write("balanced_tun", build_balanced_config(&nodes, &tun).unwrap());
        write(
            "balanced_only",
            build_balanced_config(&nodes, &only).unwrap(),
        );
        write(
            "balanced_wg",
            build_balanced_config(&nodes[2..], &opts).unwrap(),
        );
    }
}
