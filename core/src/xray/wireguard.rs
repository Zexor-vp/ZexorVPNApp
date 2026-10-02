//! WireGuard-узлы подписки.
//!
//! Для пользователей на протоколе WireGuard панель отдаёт построчно
//! `wireguard://<приватный ключ>@host:port?publickey=...&address=...#remark`
//! (формат Happ: ключи и адрес URL-кодированы, потому что base64 содержит `+ / =`).
//! xray-core умеет WireGuard как outbound в пользовательском пространстве, так что
//! отдельный драйвер/TUN не нужен — туннель работает через тот же системный прокси.

use percent_encoding::percent_decode_str;
use url::Url;

use super::parser::ParseError;

/// Разобранный WireGuard-узел.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WgNode {
    pub remark: String,
    /// Приватный ключ клиента (base64).
    pub private_key: String,
    /// Публичный ключ сервера (base64).
    pub public_key: String,
    /// Адрес клиента внутри туннеля, например `10.66.66.2/32`.
    pub address: String,
    pub server_address: String,
    pub port: u16,
}

fn decode(raw: &str) -> String {
    percent_decode_str(raw).decode_utf8_lossy().into_owned()
}

/// Разбирает одну ссылку `wireguard://...`.
pub fn parse_wireguard_uri(uri: &str) -> Result<WgNode, ParseError> {
    let uri = uri.trim();
    if !uri.starts_with("wireguard://") {
        let scheme = uri.split("://").next().unwrap_or(uri);
        return Err(ParseError::UnsupportedScheme(scheme.to_string()));
    }

    let url = Url::parse(uri).map_err(|e| ParseError::InvalidUri(e.to_string()))?;

    let private_key = decode(url.username());
    if private_key.is_empty() {
        return Err(ParseError::MissingWireguardKey);
    }
    let server_address = url
        .host_str()
        .filter(|h| !h.is_empty())
        .ok_or(ParseError::MissingHost)?
        .to_string();
    let port = url.port().ok_or(ParseError::MissingPort)?;

    let mut public_key = None;
    let mut address = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "publickey" | "publicKey" => public_key = Some(value.into_owned()),
            "address" => address = Some(value.into_owned()),
            _ => {}
        }
    }
    let public_key = public_key
        .filter(|k| !k.is_empty())
        .ok_or(ParseError::MissingWireguardKey)?;
    let address = address
        .filter(|a| !a.is_empty())
        .ok_or(ParseError::MissingWireguardAddress)?;

    let remark = url
        .fragment()
        .map(decode)
        .filter(|r| !r.is_empty())
        .unwrap_or_else(|| server_address.clone());

    Ok(WgNode {
        remark,
        private_key,
        public_key,
        address,
        server_address,
        port,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ключи — заведомо тестовые.
    const URI: &str = "wireguard://AAAA%2BBBBB%2FCCCC%3D@203.0.113.5:51820?publickey=ZZZZ%2BYYYY%2FXXXX%3D&address=10.66.66.2%2F32#%F0%9F%87%A9%F0%9F%87%AA%20Germany";

    #[test]
    fn parses_happ_style_link() {
        let node = parse_wireguard_uri(URI).unwrap();
        assert_eq!(node.private_key, "AAAA+BBBB/CCCC=");
        assert_eq!(node.public_key, "ZZZZ+YYYY/XXXX=");
        assert_eq!(node.address, "10.66.66.2/32");
        assert_eq!(node.server_address, "203.0.113.5");
        assert_eq!(node.port, 51820);
        assert_eq!(node.remark, "🇩🇪 Germany");
    }

    #[test]
    fn rejects_other_schemes_and_missing_parts() {
        assert!(matches!(
            parse_wireguard_uri("vless://x@h:1"),
            Err(ParseError::UnsupportedScheme(_))
        ));
        assert!(matches!(
            parse_wireguard_uri("wireguard://@203.0.113.5:51820?publickey=a&address=b"),
            Err(ParseError::MissingWireguardKey)
        ));
        assert!(matches!(
            parse_wireguard_uri("wireguard://k@203.0.113.5:51820?publickey=a"),
            Err(ParseError::MissingWireguardAddress)
        ));
        assert!(matches!(
            parse_wireguard_uri("wireguard://k@203.0.113.5?publickey=a&address=b"),
            Err(ParseError::MissingPort)
        ));
    }

    #[test]
    fn falls_back_to_host_when_remark_is_missing() {
        let node = parse_wireguard_uri(
            "wireguard://k@203.0.113.5:51820?publickey=a&address=10.0.0.2%2F32",
        )
        .unwrap();
        assert_eq!(node.remark, "203.0.113.5");
    }
}
