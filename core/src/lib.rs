//! Ядро десктоп-клиента Zexor VPN.
//!
//! Здесь живёт вся логика, не зависящая от Tauri: разбор подписки панели,
//! сборка конфига xray-core, блок-листы, запуск/надзор за процессом xray и
//! управление системным прокси Windows. Платформенные части спрятаны за
//! `#[cfg(windows)]`, поэтому ядро целиком тестируется и на Linux.

pub mod adblock;
pub mod api;
pub mod auth;
pub mod proxy;
pub mod xray;

pub use api::{ApiClient, ApiError, SubscriptionData, SubscriptionStatusResponse};
pub use auth::{jwt_expiry, TokenAction, TokenSet};
pub use xray::config_builder::{build_config, ConfigOptions};
pub use xray::parser::{parse_subscription, parse_vless_uri, ParseError, Security, VlessNode};
pub use xray::process::{XrayError, XrayProcess};
