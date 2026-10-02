//! Проверка ссылок, которые интерфейс просит открыть во внешнем браузере.

/// Домены, которые приложение разрешает открывать в браузере по просьбе интерфейса.
const ALLOWED_HOSTS: &[&str] = &["cabinet.zexor.site", "cabinet.zexorvpn.site", "t.me"];

/// Достаёт хост из `https://host/...`, не втягивая лишних зависимостей, и сверяет с белым
/// списком. Принимаются только `https://`; логин:пароль@ в ссылке запрещён — иначе
/// `https://t.me@evil.example/` прошёл бы проверку префикса.
pub fn is_allowed_external_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') || authority.contains('\\') {
        return false;
    }
    let host = authority.split(':').next().unwrap_or("");
    ALLOWED_HOSTS
        .iter()
        .any(|allowed| host.eq_ignore_ascii_case(allowed))
}

/// Страница оплаты от платёжного провайдера: домены у каждого свои и меняются, поэтому белого списка нет —
/// ссылку отдаёт наш же бэкенд. Но открываем только обычный `https://` без логина в адресе и без пробелов.
pub fn is_safe_https_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    if url.len() > 4096 || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    !authority.is_empty() && !authority.contains('@') && !authority.contains('\\')
}

#[cfg(test)]
mod tests {
    use super::is_allowed_external_url;

    #[test]
    fn payment_urls_must_be_plain_https() {
        assert!(super::is_safe_https_url(
            "https://yoomoney.ru/checkout/payments/v2/contract?orderId=1"
        ));
        assert!(!super::is_safe_https_url("http://pay.example.com/x"));
        assert!(!super::is_safe_https_url(
            "https://user:pw@pay.example.com/"
        ));
        assert!(!super::is_safe_https_url("https://pay.example.com/a b"));
        assert!(!super::is_safe_https_url("javascript:alert(1)"));
        assert!(!super::is_safe_https_url("https:///nohost"));
    }

    #[test]
    fn allows_only_whitelisted_https_hosts() {
        assert!(is_allowed_external_url(
            "https://cabinet.zexor.site/balance"
        ));
        assert!(is_allowed_external_url("https://t.me/ZexorVPNsupport"));
        assert!(!is_allowed_external_url("http://t.me/x"));
        assert!(!is_allowed_external_url("https://t.me@evil.example/"));
        assert!(!is_allowed_external_url("https://evil.example/t.me"));
        assert!(!is_allowed_external_url(
            "https://cabinet.zexor.site.evil.example/"
        ));
        assert!(!is_allowed_external_url(
            "file:///C:/Windows/System32/calc.exe"
        ));
    }
}
