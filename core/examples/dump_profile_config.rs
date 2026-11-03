//! Собирает конфиг xray ровно так, как это делает приложение, для профиля из подписки.
//! Нужен, чтобы проверять режим TUN на Linux (сетевое пространство имён): `cargo run -p zexor-vpn-core --example dump_profile_config -- tun < profile.json > config.json`.
//! Аргумент: `proxy` (по умолчанию) или `tun`; профиль — JSON одного профиля подписки (как отдаёт панель) на stdin.
use std::io::Read;

use zexor_vpn_core::xray::routing::profile_config;
use zexor_vpn_core::{ConfigOptions, Profile, TunnelMode};

fn main() {
    let mode = match std::env::args().nth(1).as_deref() {
        Some("tun") => TunnelMode::Tun,
        _ => TunnelMode::Proxy,
    };
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw).expect("stdin");
    let config: serde_json::Value = serde_json::from_str(&raw).expect("JSON профиля");
    let remark = config["remarks"].as_str().unwrap_or("profile").to_string();
    let opts = ConfigOptions {
        mode,
        log_level: "info".into(),
        access_log_path: Some("/tmp/xray-access.log".into()),
        ..ConfigOptions::default()
    };
    let out = profile_config(&Profile { remark, config }, &opts);
    println!("{}", serde_json::to_string(&out).unwrap());
}
