//! Ядро десктоп-клиента Zexor VPN.
//!
//! Здесь живёт вся логика, не зависящая от Tauri: разбор подписки панели,
//! сборка конфига xray-core, блок-листы, запуск/надзор за процессом xray и
//! управление системным прокси Windows. Платформенные части спрятаны за
//! `#[cfg(windows)]`, поэтому ядро целиком тестируется и на Linux.

pub mod adblock;
pub mod api;
pub mod apps;
pub mod auth;
pub mod awg;
pub mod elevation;
pub mod hwid;
pub mod links;
pub mod probe;
pub mod proxy;
pub mod settings;
pub mod sources;
pub mod xray;

pub use api::{
    ApiClient, ApiError, DeepLinkToken, LoginPoll, PairPoll, SubscriptionData,
    SubscriptionStatusResponse,
};
pub use auth::{jwt_expiry, TokenAction, TokenSet};
pub use xray::config_builder::{build_config, build_node_config, ConfigOptions, TunnelMode};
pub use xray::node::{parse_nodes, placeholder_reason, Node};
pub use xray::parser::{parse_subscription, parse_vless_uri, ParseError, Security, VlessNode};
pub use xray::process::{XrayError, XrayProcess};
pub use xray::profile::Profile;
pub use xray::routing::build_balanced_config;
pub use xray::wireguard::{parse_wireguard_uri, WgNode};
