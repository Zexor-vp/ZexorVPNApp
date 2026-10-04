//! Единый тип узла подписки: VLESS или WireGuard.
//!
//! Какой из протоколов отдаст панель, зависит от выбранного пользователем
//! протокола подписки (переключается в боте/кабинете/приложении), поэтому в одном
//! теле подписки приходят узлы одного вида, но клиент не должен на это полагаться.

use super::parser::{decode_subscription_body, parse_vless_uri, ParseError, VlessNode};
use super::profile::{parse_profiles, Profile};
use super::wireguard::{parse_wireguard_uri, WgNode};

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Vless(VlessNode),
    WireGuard(WgNode),
    /// Готовый профиль xray из подписки в формате Happ (сервер или панельный «AUTO»).
    Profile(Profile),
}

impl Node {
    pub fn remark(&self) -> &str {
        match self {
            Node::Vless(n) => &n.remark,
            Node::WireGuard(n) => &n.remark,
            Node::Profile(p) => &p.remark,
        }
    }

    /// Короткое имя протокола для интерфейса.
    pub fn protocol(&self) -> &str {
        match self {
            Node::Vless(_) => "vless",
            Node::WireGuard(_) => "wireguard",
            Node::Profile(p) => p.protocol(),
        }
    }

    pub fn is_reality(&self) -> bool {
        match self {
            Node::Vless(n) => n.is_reality(),
            Node::WireGuard(_) => false,
            Node::Profile(p) => p.is_reality(),
        }
    }

    /// Профиль-балансировщик (панельный «AUTO»): в списке серверов его не показываем.
    pub fn is_balanced(&self) -> bool {
        matches!(self, Node::Profile(p) if p.is_balanced())
    }
}

/// Разбирает тело подписки в список узлов обоих поддерживаемых протоколов.
/// Неподдерживаемые схемы и битые строки молча пропускаются.
/// Панель при проблемах с подпиской отдаёт вместо серверов «заглушки»: узлы, имя которых — текст сообщения
/// (лимит устройств, подписка закончилась, нет серверов...). Возвращает понятное объяснение, если весь
/// список состоит из таких заглушек.
pub fn placeholder_reason(nodes: &[Node]) -> Option<&'static str> {
    if nodes.is_empty() || nodes.len() > 8 {
        return None;
    }
    let text = nodes
        .iter()
        .map(|n| n.remark().to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    let has = |needle: &str| text.contains(needle);
    if has("лимит устройств") || has("сбросом устройств") || has("device limit")
    {
        Some("превышен лимит устройств — удалите ненужное устройство в кабинете или в боте (сброс устройств)")
    } else if has("hwid") {
        Some("сервис подписки не принял это устройство (HWID) — напишите в поддержку")
    } else if has("подписка отключена") {
        Some("подписка отключена — напишите в поддержку")
    } else if has("подписка") && has("закончилась") {
        Some("подписка закончилась — продлите её в кабинете или в боте")
    } else if has("лимит трафика") {
        Some("достигнут лимит трафика — продлите подписку или дождитесь сброса")
    } else if has("код ошибки") || (has("напишите") && has("поддержку")) {
        Some("в подписке нет доступных серверов — напишите в поддержку")
    } else {
        None
    }
}

pub fn parse_nodes(body: &str) -> Result<Vec<Node>, ParseError> {
    // Формат Happ: JSON-массив готовых профилей xray.
    if let Some(profiles) = parse_profiles(body)? {
        return Ok(profiles.into_iter().map(Node::Profile).collect());
    }

    let decoded = decode_subscription_body(body)?;

    let nodes: Vec<Node> = decoded
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            if line.starts_with("vless://") {
                parse_vless_uri(line).ok().map(Node::Vless)
            } else if line.starts_with("wireguard://") {
                parse_wireguard_uri(line).ok().map(Node::WireGuard)
            } else {
                None
            }
        })
        .collect();

    if nodes.is_empty() {
        return Err(ParseError::NoSupportedNodes);
    }
    Ok(nodes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VLESS: &str = "vless://00000000-1111-2222-3333-444444444444@203.0.113.10:443?type=tcp&security=reality&pbk=EXAMPLEPublicKeyForTestsOnly0000000000000000&fp=chrome&sni=example.com&sid=0123456789abcdef&flow=xtls-rprx-vision#Czech";
    const WG: &str = "wireguard://AAAA%2BBBBB%3D@203.0.113.5:51820?publickey=ZZZZ%3D&address=10.66.66.2%2F32#Germany";

    fn fake(remarks: &[&str]) -> Vec<Node> {
        let body = remarks
            .iter()
            .map(|r| format!("vless://00000000-1111-2222-3333-444444444444@0.0.0.0:1?type=tcp&security=none#{}", r.replace(' ', "%20")))
            .collect::<Vec<_>>()
            .join("\n");
        parse_nodes(&body).unwrap()
    }

    #[test]
    fn placeholder_lists_are_recognised() {
        let limit = fake(&[
            "ЛИМИТ УСТРОЙСТВ",
            "ПРЕВЫШЕН",
            "Воспользуйтесь ",
            "Сбросом устройств ",
        ]);
        assert!(placeholder_reason(&limit)
            .unwrap()
            .starts_with("превышен лимит устройств"));
        let empty = fake(&["Напишите ", "В поддержку ", "Код ошибки:", "12"]);
        assert!(placeholder_reason(&empty)
            .unwrap()
            .contains("нет доступных серверов"));
        let expired = fake(&["⌛ ПОДПИСКА", "ЗАКОНЧИЛАСЬ"]);
        assert!(placeholder_reason(&expired)
            .unwrap()
            .contains("закончилась"));
        let real = parse_nodes(VLESS).unwrap();
        assert!(placeholder_reason(&real).is_none());
    }

    #[test]
    fn plain_wireguard_list_is_parsed() {
        let nodes = parse_nodes(WG).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].protocol(), "wireguard");
        assert_eq!(nodes[0].remark(), "Germany");
    }

    #[test]
    fn mixed_list_keeps_both_and_skips_unknown_schemes() {
        let body = format!("{VLESS}\ntrojan://x@h:1#skip\n{WG}\n");
        let nodes = parse_nodes(&body).unwrap();
        let protocols: Vec<_> = nodes.iter().map(Node::protocol).collect();
        assert_eq!(protocols, ["vless", "wireguard"]);
        assert!(nodes[0].is_reality());
        assert!(!nodes[1].is_reality());
    }

    #[test]
    fn base64_wireguard_list_is_parsed() {
        use base64::engine::general_purpose::STANDARD;
        use base64::Engine;
        let nodes = parse_nodes(&STANDARD.encode(WG)).unwrap();
        assert_eq!(nodes[0].protocol(), "wireguard");
    }

    #[test]
    fn nothing_supported_is_an_error() {
        assert_eq!(
            parse_nodes("trojan://x@h:1#only").unwrap_err(),
            ParseError::NoSupportedNodes
        );
    }
}
