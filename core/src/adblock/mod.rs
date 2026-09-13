//! Разбор блок-листов в плоский список доменов.
//!
//! Списки в интернете (и наш собственный `adblock-filter.txt`) приходят в трёх
//! видах: hosts-файл, синтаксис AdBlock Plus и просто домены построчно. Этот
//! модуль сводит их к одному виду, который дальше превращается в правила
//! маршрутизации xray.

use std::collections::BTreeSet;

/// Хосты, которые нельзя блокировать, даже если они встретились в списке —
/// иначе сломаем локальную машину.
const NEVER_BLOCK: &[&str] = &[
    "localhost",
    "localhost.localdomain",
    "local",
    "broadcasthost",
    "ip6-localhost",
    "ip6-loopback",
    "ip6-localnet",
    "ip6-mcastprefix",
    "ip6-allnodes",
    "ip6-allrouters",
    "ip6-allhosts",
    "0.0.0.0",
];

/// Разбирает содержимое блок-листа любого из поддерживаемых форматов.
///
/// Результат отсортирован и дедуплицирован, чтобы конфиг xray был
/// детерминированным — одинаковый вход всегда даёт одинаковый конфиг.
pub fn parse_blocklist(contents: &str) -> Vec<String> {
    let mut domains: BTreeSet<String> = BTreeSet::new();

    for raw_line in contents.lines() {
        let line = strip_comment(raw_line);
        if line.is_empty() {
            continue;
        }

        if let Some(domain) = parse_line(line) {
            if is_valid_domain(&domain) && !NEVER_BLOCK.contains(&domain.as_str()) {
                domains.insert(domain);
            }
        }
    }

    domains.into_iter().collect()
}

/// Убирает комментарии (`#` и `!`) и лишние пробелы.
fn strip_comment(line: &str) -> &str {
    let line = line.trim();
    if line.starts_with('#') || line.starts_with('!') || line.starts_with("[Adblock") {
        return "";
    }
    // Правила-исключения и косметические фильтры ABP отбрасываем здесь, до
    // обрезки inline-комментария: иначе `example.com##.banner` превратился бы
    // в блокировку самого `example.com`.
    if line.starts_with("@@") || line.contains("##") || line.contains("#@#") {
        return "";
    }
    match line.find('#') {
        Some(idx) => line[..idx].trim(),
        None => line,
    }
}

fn parse_line(line: &str) -> Option<String> {
    // AdBlock Plus: ||example.com^ / ||example.com^$third-party
    if let Some(rest) = line.strip_prefix("||") {
        let domain = rest
            .split(['^', '$', '/'])
            .next()
            .unwrap_or_default()
            .trim();
        return Some(domain.to_ascii_lowercase());
    }

    let mut parts = line.split_whitespace();
    let first = parts.next()?;

    // hosts-формат: "0.0.0.0 ads.example.com" — берём второе поле.
    if is_ip_like(first) {
        let domain = parts.next()?;
        return Some(domain.trim().to_ascii_lowercase());
    }

    // Голый домен, возможно с ведущей точкой или схемой.
    let domain = first
        .trim_start_matches("*.")
        .trim_start_matches('.')
        .trim_end_matches('.');
    Some(domain.to_ascii_lowercase())
}

fn is_ip_like(token: &str) -> bool {
    token == "0.0.0.0"
        || token == "127.0.0.1"
        || token == "::1"
        || token == "::"
        || (token.chars().all(|c| c.is_ascii_digit() || c == '.')
            && token.matches('.').count() == 3)
}

/// Грубая, но достаточная проверка: нам важно не пустить в конфиг мусор,
/// который xray потом не примет. Валидируем полейбльно — дефис в конце любого
/// лейбла (`bad-.example`) так же невалиден, как и в конце всего домена.
fn is_valid_domain(domain: &str) -> bool {
    if domain.is_empty() || domain.len() > 253 || !domain.contains('.') {
        return false;
    }
    domain.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hosts_format() {
        let list = "\
0.0.0.0 ads.example.com
127.0.0.1 tracker.example.net
0.0.0.0 localhost
";
        assert_eq!(
            parse_blocklist(list),
            vec!["ads.example.com", "tracker.example.net"]
        );
    }

    #[test]
    fn parses_plain_domain_list() {
        let list = "ads.example.com\ntracker.example.net\n";
        assert_eq!(
            parse_blocklist(list),
            vec!["ads.example.com", "tracker.example.net"]
        );
    }

    #[test]
    fn parses_adblock_plus_syntax() {
        let list = "\
[Adblock Plus 2.0]
! заголовок
||ads.example.com^
||tracker.example.net^$third-party
||cdn.example.org/banner
";
        assert_eq!(
            parse_blocklist(list),
            vec!["ads.example.com", "cdn.example.org", "tracker.example.net"]
        );
    }

    #[test]
    fn skips_exception_and_cosmetic_rules() {
        let list = "\
@@||good.example.com^
example.com##.banner
example.com#@#.ad
||real-ad.example^
";
        assert_eq!(parse_blocklist(list), vec!["real-ad.example"]);
    }

    #[test]
    fn never_blocks_localhost_entries() {
        let list = "\
127.0.0.1 localhost
127.0.0.1 localhost.localdomain
::1 ip6-localhost
0.0.0.0 broadcasthost
0.0.0.0 real-ad.example
";
        assert_eq!(parse_blocklist(list), vec!["real-ad.example"]);
    }

    #[test]
    fn strips_inline_comments_and_wildcards() {
        let list = "\
ads.example.com # реклама
*.tracker.example.net
.leading-dot.example
trailing-dot.example.
";
        // Порядок — лексикографический (BTreeSet), конфиг должен быть детерминированным.
        assert_eq!(
            parse_blocklist(list),
            vec![
                "ads.example.com",
                "leading-dot.example",
                "tracker.example.net",
                "trailing-dot.example",
            ]
        );
    }

    #[test]
    fn deduplicates_and_sorts() {
        let list = "b.example\na.example\nb.example\n0.0.0.0 a.example\n";
        assert_eq!(parse_blocklist(list), vec!["a.example", "b.example"]);
    }

    #[test]
    fn rejects_garbage_entries() {
        let list = "\
not-a-domain
-bad.example
bad-.example
куча мусора
кириллица.example
";
        // Нет точки, дефис в начале/конце лейбла, не-ASCII — всё мимо.
        assert!(parse_blocklist(list).is_empty());
    }

    #[test]
    fn normalizes_leading_dots_instead_of_rejecting() {
        // `.example.com` и `..example.com` — это тот же домен, а не мусор.
        assert_eq!(parse_blocklist(".ads.example\n..cdn.example\n"), vec!["ads.example", "cdn.example"]);
    }

    #[test]
    fn handles_empty_input() {
        assert!(parse_blocklist("").is_empty());
        assert!(parse_blocklist("# только комментарии\n! и ещё\n").is_empty());
    }

    #[test]
    fn normalizes_case() {
        assert_eq!(parse_blocklist("ADS.Example.COM"), vec!["ads.example.com"]);
    }
}
