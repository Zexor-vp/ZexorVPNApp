//! Список запущенных приложений пользователя для настройки маршрутизации по приложениям.

/// Служебные процессы, которые в пользовательском списке только мешают.
const HIDDEN_PROCESSES: &[&str] = &[
    "svchost",
    "conhost",
    "csrss",
    "dwm",
    "winlogon",
    "fontdrvhost",
    "sihost",
    "ctfmon",
    "taskhostw",
    "runtimebroker",
    "tasklist",
    "cmd",
    "smss",
    "wininit",
    "services",
    "lsass",
    "xray",
    "searchhost",
    "startmenuexperiencehost",
    "textinputhost",
    "shellexperiencehost",
    "applicationframehost",
    "securityhealthsystray",
    "dllhost",
    "backgroundtaskhost",
    "wmiprvse",
    "audiodg",
    "spoolsv",
    "zexor vpn",
    "zexor-vpn-desktop",
];

/// Запущенные приложения в сеансе пользователя (без служб), отсортированные и без повторов.
pub fn running_apps() -> Vec<String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let output = std::process::Command::new("tasklist")
            .args(["/fo", "csv", "/nh", "/fi", "SESSION gt 0"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        match output {
            Ok(out) => parse_tasklist(&String::from_utf8_lossy(&out.stdout)),
            Err(_) => Vec::new(),
        }
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Разбор вывода `tasklist /fo csv /nh`: первое поле каждой строки — имя образа в кавычках.
pub fn parse_tasklist(text: &str) -> Vec<String> {
    let mut names: Vec<String> = text
        .lines()
        .filter_map(|line| {
            let name = line
                .trim()
                .strip_prefix('"')?
                .split('"')
                .next()?
                .trim()
                .to_string();
            let stem = name.to_lowercase();
            let stem = stem.strip_suffix(".exe").unwrap_or(&stem);
            (!name.is_empty() && !HIDDEN_PROCESSES.contains(&stem)).then_some(name)
        })
        .collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasklist_output_is_reduced_to_unique_user_apps() {
        let text = "\"chrome.exe\",\"1234\",\"Console\",\"1\",\"100 K\"\r\n\
                    \"svchost.exe\",\"4\",\"Console\",\"1\",\"10 K\"\r\n\
                    \"Chrome.exe\",\"1235\",\"Console\",\"1\",\"100 K\"\r\n\
                    \"Telegram.exe\",\"77\",\"Console\",\"1\",\"100 K\"\r\n\
                    мусор без кавычек\r\n";
        assert_eq!(parse_tasklist(text), ["chrome.exe", "Telegram.exe"]);
    }
}
