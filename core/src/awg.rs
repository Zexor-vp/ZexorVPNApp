//! AmneziaWG: разбор конфига `.conf`, перевод его в протокол управления (UAPI) и запуск `amneziawg-go`.
//!
//! AmneziaWG — это WireGuard с маскировкой: меняются заголовки пакетов и добавляются «мусорные» пакеты
//! (параметры Jc, Jmin, Jmax, S1…S4, H1…H4, I1…I5), поэтому провайдер не узнаёт протокол по сигнатуре.
//! Движок — `amneziawg-go` (MIT): отдельный процесс, который получает готовый TUN-интерфейс (его создаёт
//! системный VPN Android) и настраивается текстом по unix-сокету, как обычный wireguard-go.

use std::net::{SocketAddr, ToSocketAddrs};

use base64::Engine;

#[derive(Debug, thiserror::Error)]
pub enum AwgError {
    #[error("{0}")]
    Config(String),
    #[error("не удалось запустить AmneziaWG: {0}")]
    Spawn(String),
    #[error("AmneziaWG завершился сразу после старта{0}")]
    ExitedImmediately(String),
    #[error("не удалось настроить туннель AmneziaWG: {0}")]
    Uapi(String),
}

/// Параметры маскировки, которые передаются движку как есть (значения бывают числами, диапазонами и строками вида
/// `<b 0x...>`). Порядок важен не для всех, но сохраняем как в конфиге.
const OBFUSCATION_KEYS: &[&str] = &[
    "jc", "jmin", "jmax", "s1", "s2", "s3", "s4", "h1", "h2", "h3", "h4", "i1", "i2", "i3", "i4",
    "i5",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwgPeer {
    pub public_key: String,
    pub preshared_key: Option<String>,
    /// `хост:порт` как в конфиге (хост может быть именем — перед запуском он превращается в IP).
    pub endpoint: String,
    pub allowed_ips: Vec<String>,
    pub persistent_keepalive: Option<u16>,
}

/// Сервер AmneziaWG в списке серверов: имя и разобранный конфиг.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwgNode {
    pub remark: String,
    pub config: AwgConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwgConfig {
    pub private_key: String,
    /// Адреса интерфейса в виде `IP/длина`.
    pub addresses: Vec<String>,
    pub dns: Vec<String>,
    pub mtu: Option<u16>,
    /// Пары (имя в нижнем регистре, значение) из списка [`OBFUSCATION_KEYS`].
    pub obfuscation: Vec<(String, String)>,
    pub peer: AwgPeer,
}

fn decode_key(label: &str, value: &str) -> Result<[u8; 32], AwgError> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value.trim())
        .map_err(|_| {
            AwgError::Config(format!("{label}: ключ записан неверно (ожидается base64)"))
        })?;
    <[u8; 32]>::try_from(bytes.as_slice())
        .map_err(|_| AwgError::Config(format!("{label}: ключ должен быть длиной 32 байта")))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// `10.0.0.2` → `10.0.0.2/32`, `fd00::2` → `fd00::2/128`: у адреса интерфейса маска обязательна.
fn with_prefix(address: &str) -> String {
    if address.contains('/') {
        address.to_string()
    } else if address.contains(':') {
        format!("{address}/128")
    } else {
        format!("{address}/32")
    }
}

impl AwgConfig {
    /// Разбирает текст `.conf` (раздел `[Interface]` и первый `[Peer]`).
    pub fn parse(text: &str) -> Result<Self, AwgError> {
        #[derive(PartialEq)]
        enum Section {
            None,
            Interface,
            Peer,
            Other,
        }
        let mut section = Section::None;
        let mut peers_seen = 0;

        let mut private_key = None;
        let mut addresses = Vec::new();
        let mut dns = Vec::new();
        let mut mtu = None;
        let mut obfuscation: Vec<(String, String)> = Vec::new();

        let mut public_key = None;
        let mut preshared_key = None;
        let mut endpoint = None;
        let mut allowed_ips = Vec::new();
        let mut keepalive = None;

        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                section = match line[1..line.len() - 1].trim().to_ascii_lowercase().as_str() {
                    "interface" => Section::Interface,
                    "peer" => {
                        peers_seen += 1;
                        if peers_seen == 1 {
                            Section::Peer
                        } else {
                            // Остальные пары не используются: приложение подключается к одному серверу.
                            Section::Other
                        }
                    }
                    _ => Section::Other,
                };
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim();

            match section {
                Section::Interface => match key.as_str() {
                    "privatekey" => private_key = Some(value.to_string()),
                    "address" => addresses.extend(split_list(value).iter().map(|a| with_prefix(a))),
                    "dns" => dns.extend(split_list(value)),
                    "mtu" => mtu = value.parse::<u16>().ok(),
                    k if OBFUSCATION_KEYS.contains(&k) => {
                        obfuscation.push((k.to_string(), value.to_string()))
                    }
                    _ => {}
                },
                Section::Peer => match key.as_str() {
                    "publickey" => public_key = Some(value.to_string()),
                    "presharedkey" => preshared_key = Some(value.to_string()),
                    "endpoint" => endpoint = Some(value.to_string()),
                    "allowedips" => allowed_ips.extend(split_list(value)),
                    "persistentkeepalive" => {
                        keepalive = value.parse::<u16>().ok().filter(|v| *v > 0)
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        let private_key =
            private_key.ok_or_else(|| AwgError::Config("в конфиге нет PrivateKey".to_string()))?;
        decode_key("PrivateKey", &private_key)?;
        if addresses.is_empty() {
            return Err(AwgError::Config("в конфиге нет Address".to_string()));
        }
        let public_key = public_key
            .ok_or_else(|| AwgError::Config("в конфиге нет [Peer] с PublicKey".to_string()))?;
        decode_key("PublicKey", &public_key)?;
        if let Some(psk) = &preshared_key {
            decode_key("PresharedKey", psk)?;
        }
        let endpoint = endpoint
            .ok_or_else(|| AwgError::Config("в конфиге нет Endpoint сервера".to_string()))?;
        if !endpoint.contains(':') {
            return Err(AwgError::Config(
                "Endpoint должен быть вида адрес:порт".to_string(),
            ));
        }
        if allowed_ips.is_empty() {
            allowed_ips = vec!["0.0.0.0/0".to_string(), "::/0".to_string()];
        }

        Ok(Self {
            private_key,
            addresses,
            dns,
            mtu,
            obfuscation,
            peer: AwgPeer {
                public_key,
                preshared_key,
                endpoint,
                allowed_ips,
                persistent_keepalive: keepalive,
            },
        })
    }

    /// IP-адрес сервера: если в конфиге имя, оно разрешается обычным DNS (до подъёма туннеля).
    pub fn resolve_endpoint(&self) -> Result<SocketAddr, AwgError> {
        let mut found: Vec<SocketAddr> = self
            .peer
            .endpoint
            .to_socket_addrs()
            .map_err(|_| {
                AwgError::Config("не удалось определить адрес сервера AmneziaWG".to_string())
            })?
            .collect();
        // IPv4 надёжнее: у многих хостингов IPv6 для UDP не настроен.
        found.sort_by_key(|a| a.is_ipv6());
        found.into_iter().next().ok_or_else(|| {
            AwgError::Config("не удалось определить адрес сервера AmneziaWG".to_string())
        })
    }

    /// Настройка туннеля в формате UAPI (как у `wg setconf`, ключи в hex). Заканчивается пустой строкой.
    pub fn to_uapi(&self, endpoint: SocketAddr) -> Result<String, AwgError> {
        let mut out = String::from("set=1\n");
        out.push_str(&format!(
            "private_key={}\n",
            hex(&decode_key("PrivateKey", &self.private_key)?)
        ));
        for (key, value) in &self.obfuscation {
            out.push_str(&format!("{key}={value}\n"));
        }
        out.push_str("replace_peers=true\n");
        out.push_str(&format!(
            "public_key={}\n",
            hex(&decode_key("PublicKey", &self.peer.public_key)?)
        ));
        if let Some(psk) = &self.peer.preshared_key {
            out.push_str(&format!(
                "preshared_key={}\n",
                hex(&decode_key("PresharedKey", psk)?)
            ));
        }
        out.push_str(&format!("endpoint={endpoint}\n"));
        if let Some(seconds) = self.peer.persistent_keepalive {
            out.push_str(&format!("persistent_keepalive_interval={seconds}\n"));
        }
        out.push_str("replace_allowed_ips=true\n");
        for ip in &self.peer.allowed_ips {
            out.push_str(&format!("allowed_ip={ip}\n"));
        }
        out.push('\n');
        Ok(out)
    }

    /// MTU туннеля: из конфига, а по умолчанию 1280 — запас под добавки маскировки.
    pub fn tunnel_mtu(&self) -> u16 {
        self.mtu.unwrap_or(1280)
    }
}

/// Строка подписки для приложения: `awg://<конфиг в base64url>#<имя сервера>` (так её отдаёт наш сервер подписок).
pub fn parse_awg_uri(line: &str) -> Result<AwgNode, AwgError> {
    let rest = line
        .trim()
        .strip_prefix("awg://")
        .ok_or_else(|| AwgError::Config("это не ссылка awg://".to_string()))?;
    let (token, fragment) = rest.split_once('#').unwrap_or((rest, ""));
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token.trim_end_matches('='))
        .map_err(|_| AwgError::Config("конфиг в ссылке awg:// записан неверно".to_string()))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| AwgError::Config("конфиг в ссылке awg:// не текст".to_string()))?;
    let config = AwgConfig::parse(&text)?;
    let remark = percent_encoding::percent_decode_str(fragment)
        .decode_utf8_lossy()
        .trim()
        .to_string();
    Ok(AwgNode {
        remark: if remark.is_empty() {
            "AmneziaWG".to_string()
        } else {
            remark
        },
        config,
    })
}

/// Адрес в виде `(ip, длина маски)` — для системного VPN, который принимает их раздельно.
pub fn split_cidr(cidr: &str) -> Option<(String, u8)> {
    let (ip, prefix) = cidr.split_once('/')?;
    Some((ip.trim().to_string(), prefix.trim().parse().ok()?))
}

// ───────────────────────────── Windows: настройка адаптера ─────────────────────────────
//
// На Windows движок `amneziawg-go.exe` сам создаёт адаптер (wintun), а адрес, маршруты и DNS приложение задаёт
// скриптом PowerShell. Сами скрипты и разбор вывода — обычные функции (они проверяются тестами на любой системе),
// а запуск процессов лежит в модуле `win_process` ниже.

/// Имя адаптера AmneziaWG в Windows.
pub const WINDOWS_ADAPTER: &str = "ZexorAWG";

/// Основной маршрут по умолчанию до подъёма туннеля: через него идёт трафик к самому серверу AmneziaWG
/// (иначе пакеты рукопожатия попали бы в собственный туннель).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultRoute {
    pub next_hop: String,
    pub if_index: u32,
}

/// Скрипт PowerShell, который печатает `<шлюз>,<номер интерфейса>` основного маршрута по умолчанию.
pub const DEFAULT_ROUTE_SCRIPT: &str = "$r = Get-NetRoute -DestinationPrefix '0.0.0.0/0' -ErrorAction Stop | Sort-Object RouteMetric | Select-Object -First 1; if (-not $r) { exit 3 }; \"$($r.NextHop),$($r.InterfaceIndex)\"";

/// Разбор вывода [`DEFAULT_ROUTE_SCRIPT`].
pub fn parse_default_route(output: &str) -> Option<DefaultRoute> {
    let line = output.lines().map(str::trim).find(|l| l.contains(','))?;
    let (hop, index) = line.split_once(',')?;
    let next_hop: std::net::Ipv4Addr = hop.trim().parse().ok()?;
    let if_index: u32 = index.trim().parse().ok()?;
    Some(DefaultRoute {
        next_hop: next_hop.to_string(),
        if_index,
    })
}

/// Скрипт PowerShell, который настраивает адаптер `adapter` уже после запуска движка: адрес, MTU, DNS, маршрут до
/// сервера в обход туннеля и маршруты туннеля. Все значения (адреса, DNS) проверяются как IP-адреса — в текст
/// скрипта попадает только то, что разобралось в числа, поэтому из конфига подписки в него ничего подставить нельзя.
/// Изменения только в активном хранилище (`ActiveStore`): после перезагрузки их нет.
pub fn windows_setup_script(
    adapter: &str,
    config: &AwgConfig,
    endpoint: std::net::Ipv4Addr,
    default_route: &DefaultRoute,
) -> Result<String, AwgError> {
    use std::net::{IpAddr, Ipv4Addr};

    let mut addresses = Vec::new();
    for raw in &config.addresses {
        let (ip, prefix) = split_cidr(&with_prefix(raw))
            .ok_or_else(|| AwgError::Config(format!("неверный адрес интерфейса: {raw}")))?;
        if let Ok(ip) = ip.parse::<Ipv4Addr>() {
            if prefix <= 32 {
                addresses.push((ip, prefix));
            }
        }
    }
    if addresses.is_empty() {
        return Err(AwgError::Config(
            "в конфиге нет IPv4-адреса интерфейса".to_string(),
        ));
    }

    let mut dns = Vec::new();
    for raw in &config.dns {
        let ip: IpAddr = raw
            .trim()
            .parse()
            .map_err(|_| AwgError::Config(format!("неверный адрес DNS: {raw}")))?;
        dns.push(ip.to_string());
    }

    // Маршруты туннеля из AllowedIPs: «всё» (0.0.0.0/0) делим на две половины — они точнее маршрута по умолчанию, и
    // он остаётся запасным, если туннель вдруг пропадёт.
    let mut routes: Vec<String> = Vec::new();
    for raw in &config.peer.allowed_ips {
        let Some((ip, prefix)) = split_cidr(&with_prefix(raw)) else {
            continue;
        };
        let Ok(ip) = ip.parse::<Ipv4Addr>() else {
            continue; // IPv6 пока не маршрутизируем
        };
        if prefix > 32 {
            continue;
        }
        if prefix == 0 {
            routes.push("0.0.0.0/1".to_string());
            routes.push("128.0.0.0/1".to_string());
        } else {
            routes.push(format!("{ip}/{prefix}"));
        }
    }
    if routes.is_empty() {
        return Err(AwgError::Config(
            "в конфиге нет IPv4-маршрутов (AllowedIPs)".to_string(),
        ));
    }

    let mut script = String::new();
    script.push_str("$ErrorActionPreference = 'Stop'\n");
    script.push_str(&format!("$alias = '{adapter}'\n"));
    script.push_str(
        "for ($i = 0; $i -lt 60; $i++) { $a = Get-NetAdapter -Name $alias -ErrorAction SilentlyContinue; if ($a) { break }; Start-Sleep -Milliseconds 250 }\n",
    );
    script.push_str("if (-not $a) { throw 'адаптер AmneziaWG не появился' }\n");
    script.push_str("$idx = $a.ifIndex\n");
    for (ip, prefix) in &addresses {
        script.push_str(&format!(
            "New-NetIPAddress -InterfaceIndex $idx -IPAddress '{ip}' -PrefixLength {prefix} -PolicyStore ActiveStore | Out-Null\n"
        ));
    }
    script.push_str(&format!(
        "Set-NetIPInterface -InterfaceIndex $idx -AddressFamily IPv4 -NlMtuBytes {} -InterfaceMetric 1 -ErrorAction SilentlyContinue\n",
        config.tunnel_mtu()
    ));
    if !dns.is_empty() {
        let list = dns
            .iter()
            .map(|d| format!("'{d}'"))
            .collect::<Vec<_>>()
            .join(",");
        script.push_str(&format!(
            "Set-DnsClientServerAddress -InterfaceIndex $idx -ServerAddresses {list}\n"
        ));
    }
    // Сначала маршрут до самого сервера через прежний шлюз — только потом маршруты туннеля.
    script.push_str(&format!(
        "New-NetRoute -DestinationPrefix '{endpoint}/32' -InterfaceIndex {} -NextHop '{}' -RouteMetric 1 -PolicyStore ActiveStore | Out-Null\n",
        default_route.if_index, default_route.next_hop
    ));
    for route in &routes {
        script.push_str(&format!(
            "New-NetRoute -DestinationPrefix '{route}' -InterfaceIndex $idx -NextHop '0.0.0.0' -RouteMetric 1 -PolicyStore ActiveStore | Out-Null\n"
        ));
    }
    Ok(script)
}

#[cfg(unix)]
pub use process::AwgProcess;

#[cfg(unix)]
mod process {
    use std::io::{Read, Write};
    use std::os::unix::io::AsRawFd;
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::os::unix::process::CommandExt;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::time::Duration;

    use super::{AwgConfig, AwgError};

    /// Запущенный `amneziawg-go`. Пока жив — туннель работает; `Drop` его гасит.
    pub struct AwgProcess {
        child: Child,
        socket_path: PathBuf,
    }

    impl AwgProcess {
        /// Запускает движок с готовым TUN-дескриптором и применяет конфиг.
        ///
        /// `tun_fd` должен быть открыт и принадлежать вызывающему: он наследуется дочерним процессом.
        pub fn start(
            binary: &Path,
            tun_fd: i32,
            work_dir: &Path,
            config: &AwgConfig,
        ) -> Result<Self, AwgError> {
            if !binary.exists() {
                return Err(AwgError::Spawn(format!("нет файла {}", binary.display())));
            }
            let endpoint = config.resolve_endpoint()?;
            let uapi = config.to_uapi(endpoint)?;

            std::fs::create_dir_all(work_dir).map_err(|e| AwgError::Spawn(e.to_string()))?;
            // Имя файла сокета движок выводит из настоящего имени TUN-интерфейса и каталога WG_SOCKET_DIR (наш патч
            // сборки): он следит за этим путём, поэтому сокет должен лежать именно там.
            let interface = tun_name(tun_fd)?;
            let socket_path = work_dir.join(format!("{interface}.sock"));
            let _ = std::fs::remove_file(&socket_path);
            let listener = UnixListener::bind(&socket_path)
                .map_err(|e| AwgError::Spawn(format!("сокет: {e}")))?;
            let uapi_fd = listener.as_raw_fd();

            let log_path = work_dir.join("awg.log");
            let (log_out, log_err) = match std::fs::File::create(&log_path) {
                Ok(file) => match file.try_clone() {
                    Ok(clone) => (Stdio::from(file), Stdio::from(clone)),
                    Err(_) => (Stdio::from(file), Stdio::null()),
                },
                Err(_) => (Stdio::null(), Stdio::null()),
            };

            let mut command = Command::new(binary);
            command
                .arg("-f")
                .arg(&interface)
                .env("WG_TUN_FD", tun_fd.to_string())
                .env("WG_UAPI_FD", uapi_fd.to_string())
                .env("WG_SOCKET_DIR", work_dir)
                .env("WG_PROCESS_FOREGROUND", "1")
                .env("LOG_LEVEL", "error")
                .stdin(Stdio::null())
                .stdout(log_out)
                .stderr(log_err);
            // По умолчанию дескрипторы закрываются при запуске дочернего процесса (CLOEXEC) — снимаем флаг.
            // SAFETY: в `pre_exec` вызывается только `fcntl` — функция, безопасная между fork и exec.
            unsafe {
                command.pre_exec(move || {
                    for fd in [tun_fd, uapi_fd] {
                        if libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                            return Err(std::io::Error::last_os_error());
                        }
                    }
                    Ok(())
                });
            }
            let child = command
                .spawn()
                .map_err(|e| AwgError::Spawn(e.to_string()))?;
            let mut process = Self { child, socket_path };

            // Неверный дескриптор или повреждённый бинарник роняют движок сразу — ловим это здесь.
            std::thread::sleep(Duration::from_millis(400));
            if let Ok(Some(status)) = process.child.try_wait() {
                return Err(AwgError::ExitedImmediately(format!(
                    " (код {:?}){}",
                    status.code(),
                    log_tail(&log_path)
                )));
            }

            let mut stream = UnixStream::connect(&process.socket_path)
                .map_err(|e| AwgError::Uapi(format!("подключение к движку: {e}")))?;
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
            stream
                .write_all(uapi.as_bytes())
                .map_err(|e| AwgError::Uapi(e.to_string()))?;
            let mut reply = String::new();
            let mut buffer = [0u8; 256];
            while !reply.contains("\n\n") {
                match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(n) => reply.push_str(&String::from_utf8_lossy(&buffer[..n])),
                    Err(e) => return Err(AwgError::Uapi(format!("нет ответа движка: {e}"))),
                }
            }
            drop(listener);
            if !reply.contains("errno=0") {
                return Err(AwgError::Uapi(format!(
                    "движок отклонил настройки ({}){}",
                    reply.trim(),
                    log_tail(&log_path)
                )));
            }
            Ok(process)
        }

        pub fn is_running(&mut self) -> bool {
            matches!(self.child.try_wait(), Ok(None))
        }

        pub fn stop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            let _ = std::fs::remove_file(&self.socket_path);
        }
    }

    impl Drop for AwgProcess {
        fn drop(&mut self) {
            self.stop();
        }
    }

    /// Имя интерфейса по дескриптору TUN (ioctl TUNGETIFF) — у системного VPN Android оно заранее неизвестно.
    fn tun_name(fd: i32) -> Result<String, AwgError> {
        #[repr(C)]
        struct IfReq {
            name: [u8; 16],
            flags: u16,
            pad: [u8; 22],
        }
        const TUNGETIFF: u64 = 0x8004_54d2;
        let mut req = IfReq {
            name: [0; 16],
            flags: 0,
            pad: [0; 22],
        };
        // SAFETY: fd открыт вызывающим, req — ifreq нужного размера.
        let result = unsafe { libc::ioctl(fd, TUNGETIFF as _, &mut req as *mut IfReq) };
        if result != 0 {
            return Err(AwgError::Spawn(format!(
                "не удалось узнать имя TUN-интерфейса: {}",
                std::io::Error::last_os_error()
            )));
        }
        let end = req
            .name
            .iter()
            .position(|b| *b == 0)
            .unwrap_or(req.name.len());
        Ok(String::from_utf8_lossy(&req.name[..end]).to_string())
    }

    /// Последние строки журнала движка — в сообщение об ошибке.
    fn log_tail(path: &Path) -> String {
        let Ok(text) = std::fs::read_to_string(path) else {
            return String::new();
        };
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        let tail = lines[lines.len().saturating_sub(3)..].join(" | ");
        if tail.is_empty() {
            String::new()
        } else {
            format!("\n{tail}")
        }
    }
}

#[cfg(windows)]
pub use win_process::AwgProcess;

#[cfg(windows)]
mod win_process {
    use std::io::{Read, Write};
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use super::{
        parse_default_route, windows_setup_script, AwgConfig, AwgError, DefaultRoute,
        DEFAULT_ROUTE_SCRIPT, WINDOWS_ADAPTER,
    };
    use crate::xray::process::job::JobObject;

    /// Не показывать окно консоли у дочерних процессов.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// Именованный канал управления движка (UAPI): его создаёт сам `amneziawg-go.exe`.
    fn pipe_path() -> String {
        format!(r"\\.\pipe\ProtectedPrefix\Administrators\AmneziaWG\{WINDOWS_ADAPTER}")
    }

    /// Запущенный `amneziawg-go.exe`. Пока жив — туннель работает; `Drop` его гасит и убирает маршрут до сервера.
    pub struct AwgProcess {
        child: Child,
        _job: Option<JobObject>,
        endpoint: std::net::Ipv4Addr,
    }

    fn powershell(script: &str) -> Result<String, String> {
        let output = Command::new("powershell.exe")
            .args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                script,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("не удалось запустить PowerShell: {e}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        if output.status.success() {
            Ok(stdout)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let text = if stderr.trim().is_empty() {
                stdout
            } else {
                stderr.to_string()
            };
            Err(text.lines().take(3).collect::<Vec<_>>().join(" "))
        }
    }

    fn default_route() -> Result<DefaultRoute, AwgError> {
        let out = powershell(DEFAULT_ROUTE_SCRIPT).map_err(AwgError::Spawn)?;
        parse_default_route(&out).ok_or_else(|| {
            AwgError::Spawn("не удалось определить основной маршрут сети".to_string())
        })
    }

    impl AwgProcess {
        /// Запускает движок, применяет конфиг и настраивает адаптер. Нужны права администратора (адаптер wintun),
        /// `wintun.dll` лежит рядом с `binary`.
        pub fn start(binary: &Path, work_dir: &Path, config: &AwgConfig) -> Result<Self, AwgError> {
            if !binary.is_file() {
                return Err(AwgError::Spawn(format!("нет файла {}", binary.display())));
            }
            let resolved = config.resolve_endpoint()?;
            let endpoint =
                match resolved.ip() {
                    std::net::IpAddr::V4(ip) => ip,
                    std::net::IpAddr::V6(_) => return Err(AwgError::Config(
                        "сервер AmneziaWG с адресом IPv6 в Windows-версии пока не поддерживается"
                            .to_string(),
                    )),
                };
            let uapi = config.to_uapi(resolved)?;
            let route = default_route()?;
            let setup = windows_setup_script(WINDOWS_ADAPTER, config, endpoint, &route)?;

            std::fs::create_dir_all(work_dir).map_err(|e| AwgError::Spawn(e.to_string()))?;
            let log_path = work_dir.join("awg.log");
            let (log_out, log_err) = match std::fs::File::create(&log_path) {
                Ok(file) => match file.try_clone() {
                    Ok(clone) => (Stdio::from(file), Stdio::from(clone)),
                    Err(_) => (Stdio::from(file), Stdio::null()),
                },
                Err(_) => (Stdio::null(), Stdio::null()),
            };

            let mut command = Command::new(binary);
            command
                .arg(WINDOWS_ADAPTER)
                .stdin(Stdio::null())
                .stdout(log_out)
                .stderr(log_err)
                .creation_flags(CREATE_NO_WINDOW);
            // Рядом с exe лежит wintun.dll — движок ищет её в своей папке.
            if let Some(dir) = binary.parent() {
                command.current_dir(dir);
            }
            let child = command
                .spawn()
                .map_err(|e| AwgError::Spawn(e.to_string()))?;
            let job = JobObject::new().ok();
            if let Some(job) = &job {
                let _ = job.assign(&child);
            }
            let mut process = Self {
                child,
                _job: job,
                endpoint,
            };

            if let Err(error) = process.configure(&uapi, &setup, &log_path) {
                process.stop();
                return Err(error);
            }
            Ok(process)
        }

        fn configure(&mut self, uapi: &str, setup: &str, log_path: &Path) -> Result<(), AwgError> {
            // 1. Ждём, пока движок создаст адаптер и канал управления (первый запуск ставит драйвер — до нескольких секунд).
            let deadline = Instant::now() + Duration::from_secs(20);
            let pipe = loop {
                if let Ok(Some(status)) = self.child.try_wait() {
                    return Err(AwgError::ExitedImmediately(format!(
                        " (код {:?}){}",
                        status.code(),
                        log_tail(log_path)
                    )));
                }
                match std::fs::OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(pipe_path())
                {
                    Ok(file) => break file,
                    Err(_) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(250))
                    }
                    Err(e) => {
                        return Err(AwgError::Uapi(format!(
                            "нет канала управления движка: {e}{}",
                            log_tail(log_path)
                        )))
                    }
                }
            };

            // 2. Отправляем настройки. Чтение из канала блокирующее — держим его в отдельном потоке с ограничением времени.
            let (sender, receiver) = mpsc::channel();
            let payload = uapi.to_string();
            std::thread::spawn(move || {
                let mut pipe = pipe;
                let result = (|| -> Result<String, String> {
                    pipe.write_all(payload.as_bytes())
                        .map_err(|e| e.to_string())?;
                    let mut reply = String::new();
                    let mut buffer = [0u8; 256];
                    while !reply.contains("\n\n") {
                        match pipe.read(&mut buffer) {
                            Ok(0) => break,
                            Ok(n) => reply.push_str(&String::from_utf8_lossy(&buffer[..n])),
                            Err(e) => return Err(e.to_string()),
                        }
                    }
                    Ok(reply)
                })();
                let _ = sender.send(result);
            });
            let reply = receiver
                .recv_timeout(Duration::from_secs(10))
                .map_err(|_| AwgError::Uapi(format!("нет ответа движка{}", log_tail(log_path))))?
                .map_err(|e| AwgError::Uapi(format!("нет ответа движка: {e}")))?;
            if !reply.contains("errno=0") {
                return Err(AwgError::Uapi(format!(
                    "движок отклонил настройки ({}){}",
                    reply.trim(),
                    log_tail(log_path)
                )));
            }

            // 3. Адрес, DNS и маршруты адаптера.
            powershell(setup).map_err(|e| AwgError::Uapi(format!("настройка сети: {e}")))?;
            Ok(())
        }

        pub fn is_running(&mut self) -> bool {
            matches!(self.child.try_wait(), Ok(None))
        }

        pub fn stop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
            // Маршруты адаптера исчезают вместе с ним; свой маршрут до сервера убираем сами.
            let _ = Command::new("route")
                .args([
                    "delete",
                    &self.endpoint.to_string(),
                    "mask",
                    "255.255.255.255",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }

    impl Drop for AwgProcess {
        fn drop(&mut self) {
            self.stop();
        }
    }

    /// Последние строки журнала движка — в сообщение об ошибке.
    fn log_tail(path: &Path) -> String {
        let Ok(text) = std::fs::read_to_string(path) else {
            return String::new();
        };
        let lines: Vec<&str> = text
            .lines()
            .filter(|l| !l.trim().is_empty() && !l.starts_with("Warning: this is a test program"))
            .collect();
        let tail = lines[lines.len().saturating_sub(3)..].join(" | ");
        if tail.is_empty() {
            String::new()
        } else {
            format!("\n{tail}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Ключи ненастоящие: 32 нулевых/единичных байта в base64.
    const KEY_A: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";
    const KEY_B: &str = "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE=";

    fn sample() -> String {
        format!(
            "[Interface]\nPrivateKey = {KEY_A}\nAddress = 10.29.29.3/32\nDNS = 1.1.1.1, 8.8.8.8\nJc = 4\nJmin = 50\nJmax = 1000\nS1 = 68\nS2 = 92\nH1 = 1463983389\nH2 = 155702891\nH3 = 1467787988\nH4 = 1138294968\n\n[Peer]\nPublicKey = {KEY_B}\nPresharedKey = {KEY_A}\nAllowedIPs = 0.0.0.0/0, ::/0\nEndpoint = 13.143.183.141:51825\nPersistentKeepalive = 25\n"
        )
    }

    #[test]
    fn parses_interface_peer_and_obfuscation() {
        let cfg = AwgConfig::parse(&sample()).unwrap();
        assert_eq!(cfg.addresses, vec!["10.29.29.3/32"]);
        assert_eq!(cfg.dns, vec!["1.1.1.1", "8.8.8.8"]);
        assert_eq!(cfg.peer.endpoint, "13.143.183.141:51825");
        assert_eq!(cfg.peer.persistent_keepalive, Some(25));
        assert_eq!(cfg.obfuscation.len(), 9);
        assert_eq!(cfg.obfuscation[0], ("jc".to_string(), "4".to_string()));
        assert_eq!(cfg.tunnel_mtu(), 1280);
    }

    #[test]
    fn uapi_text_has_hex_keys_and_obfuscation_in_order() {
        let cfg = AwgConfig::parse(&sample()).unwrap();
        let uapi = cfg
            .to_uapi("13.143.183.141:51825".parse().unwrap())
            .unwrap();
        assert!(uapi.starts_with("set=1\nprivate_key=0000"));
        assert!(uapi.contains("\njc=4\njmin=50\njmax=1000\ns1=68\ns2=92\nh1=1463983389\n"));
        assert!(uapi.contains("\npublic_key=0101"));
        assert!(uapi.contains("\nendpoint=13.143.183.141:51825\n"));
        assert!(uapi.contains("\npersistent_keepalive_interval=25\n"));
        assert!(uapi.contains("\nallowed_ip=0.0.0.0/0\nallowed_ip=::/0\n"));
        assert!(uapi.ends_with("\n\n"));
    }

    #[test]
    fn address_without_mask_gets_one() {
        let text = sample().replace("10.29.29.3/32", "10.29.29.3, fd00::3");
        let cfg = AwgConfig::parse(&text).unwrap();
        assert_eq!(cfg.addresses, vec!["10.29.29.3/32", "fd00::3/128"]);
    }

    #[test]
    fn broken_configs_are_rejected_with_a_reason() {
        assert!(AwgConfig::parse("").is_err());
        assert!(AwgConfig::parse(&sample().replace(KEY_A, "не-ключ")).is_err());
        assert!(
            AwgConfig::parse(&sample().replace("Endpoint = 13.143.183.141:51825\n", "")).is_err()
        );
        assert!(AwgConfig::parse(&sample().replace("Address = 10.29.29.3/32\n", "")).is_err());
    }

    #[test]
    fn only_the_first_peer_is_used() {
        let text = format!(
            "{}\n[Peer]\nPublicKey = {KEY_A}\nEndpoint = 1.2.3.4:5\nAllowedIPs = 10.0.0.0/8\n",
            sample()
        );
        let cfg = AwgConfig::parse(&text).unwrap();
        assert_eq!(cfg.peer.endpoint, "13.143.183.141:51825");
    }

    #[test]
    fn awg_uri_roundtrip() {
        let token = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sample().as_bytes());
        let node = parse_awg_uri(&format!("awg://{token}#Czech%20Republic")).unwrap();
        assert_eq!(node.remark, "Czech Republic");
        assert_eq!(node.config.peer.endpoint, "13.143.183.141:51825");
        assert!(parse_awg_uri("awg://!!!").is_err());
        assert!(parse_awg_uri("vless://x").is_err());
    }

    #[test]
    fn cidr_split() {
        assert_eq!(
            split_cidr("10.0.0.2/32"),
            Some(("10.0.0.2".to_string(), 32))
        );
        assert_eq!(split_cidr("::/0"), Some(("::".to_string(), 0)));
        assert_eq!(split_cidr("10.0.0.2"), None);
    }

    /// Ручная проверка с настоящим движком и сервером (нужен root и /dev/net/tun):
    /// `AWG_TEST_BIN=/путь/amneziawg-go AWG_TEST_CONF=/путь/test.conf cargo test -p zexor-vpn-core -- --ignored real_tunnel`
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore]
    fn real_tunnel_handshake() {
        use std::os::unix::io::IntoRawFd;
        use std::process::Command;

        let binary = std::env::var("AWG_TEST_BIN").expect("AWG_TEST_BIN");
        let conf = std::fs::read_to_string(std::env::var("AWG_TEST_CONF").expect("AWG_TEST_CONF"))
            .unwrap();
        let config = AwgConfig::parse(&conf).unwrap();

        // TUN-интерфейс вместо системного VPN Android: тот же дескриптор, что получает движок на телефоне.
        let tun = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/net/tun")
            .unwrap();
        let fd = tun.into_raw_fd();
        #[repr(C)]
        struct IfReq {
            name: [u8; 16],
            flags: u16,
            pad: [u8; 22],
        }
        let mut req = IfReq {
            name: [0; 16],
            flags: (libc::IFF_TUN | libc::IFF_NO_PI) as u16,
            pad: [0; 22],
        };
        req.name[..7].copy_from_slice(b"awgtest");
        // SAFETY: fd открыт выше, req — корректный ifreq нужного размера.
        assert_eq!(
            unsafe { libc::ioctl(fd, 0x4004_54ca, &mut req) },
            0,
            "TUNSETIFF"
        );

        let dir = std::env::temp_dir().join("zexor-awg-test");
        let mut process =
            AwgProcess::start(std::path::Path::new(&binary), fd, &dir, &config).unwrap();

        let run = |args: &[&str]| Command::new("ip").args(args).status().unwrap().success();
        assert!(run(&[
            "addr",
            "add",
            &config.addresses[0],
            "dev",
            "awgtest"
        ]));
        assert!(run(&["link", "set", "awgtest", "up"]));
        // Адрес клиента /32 — связанного маршрута нет, поэтому маршрут к серверу внутри туннеля добавляем вручную
        // (в приложении его создаёт системный VPN). Адрес можно сменить переменной AWG_TEST_PING.
        let target = std::env::var("AWG_TEST_PING").unwrap_or_else(|_| "10.29.29.1".to_string());
        run(&["route", "add", &format!("{target}/32"), "dev", "awgtest"]);
        let ping = Command::new("ping")
            .args(["-c", "3", "-W", "2", "-I", "awgtest", &target])
            .output()
            .unwrap();
        let ok = ping.status.success();
        process.stop();
        let _ = Command::new("ip").args(["link", "del", "awgtest"]).status();
        assert!(
            ok,
            "сервер не ответил через туннель: {}",
            String::from_utf8_lossy(&ping.stdout)
        );
    }

    fn sample_config() -> AwgConfig {
        AwgConfig::parse(
            "[Interface]\nPrivateKey = kMAgcvFXOGzOKOyUDMT6y8o6Jn0kWqVYM7bWT6fTkU4=\nAddress = 10.29.64.2/32\nDNS = 1.1.1.1, 8.8.8.8\nMTU = 1280\nJc = 4\n\n[Peer]\nPublicKey = wN0povGFu0PwWPgPvEjI9UJuZyPauxylzb6x4sAyalc=\nEndpoint = 13.143.183.141:51825\nAllowedIPs = 0.0.0.0/0, ::/0\nPersistentKeepalive = 25\n",
        )
        .unwrap()
    }

    #[test]
    fn default_route_output_is_parsed() {
        let route = parse_default_route("\r\n192.168.1.1,12\r\n").unwrap();
        assert_eq!(route.next_hop, "192.168.1.1");
        assert_eq!(route.if_index, 12);
        assert!(parse_default_route("").is_none());
        assert!(parse_default_route("не маршрут,x").is_none());
        // Маршрут «на канале» (без шлюза) — тоже валидный.
        assert_eq!(
            parse_default_route("0.0.0.0,7").unwrap().next_hop,
            "0.0.0.0"
        );
    }

    #[test]
    fn windows_script_sets_address_dns_bypass_and_split_routes() {
        let route = DefaultRoute {
            next_hop: "192.168.1.1".to_string(),
            if_index: 12,
        };
        let script = windows_setup_script(
            WINDOWS_ADAPTER,
            &sample_config(),
            "13.143.183.141".parse().unwrap(),
            &route,
        )
        .unwrap();
        assert!(script.contains("-IPAddress '10.29.64.2' -PrefixLength 32"));
        assert!(script.contains("-NlMtuBytes 1280"));
        assert!(script.contains("-ServerAddresses '1.1.1.1','8.8.8.8'"));
        // Маршрут до сервера идёт раньше маршрутов туннеля.
        let bypass = script
            .find("'13.143.183.141/32' -InterfaceIndex 12 -NextHop '192.168.1.1'")
            .unwrap();
        let first_split = script.find("'0.0.0.0/1'").unwrap();
        assert!(bypass < first_split);
        assert!(script.contains("'128.0.0.0/1'"));
        // IPv6 пока не маршрутизируется, активное хранилище — чтобы после перезагрузки ничего не осталось.
        assert!(!script.contains("::/"));
        assert_eq!(
            script.matches("-PolicyStore ActiveStore").count(),
            1 + 1 + 2
        );
    }

    #[test]
    fn windows_script_rejects_values_that_are_not_ip_addresses() {
        let route = DefaultRoute {
            next_hop: "192.168.1.1".to_string(),
            if_index: 12,
        };
        let mut config = sample_config();
        config.dns = vec!["1.1.1.1'; calc; '".to_string()];
        assert!(windows_setup_script(
            WINDOWS_ADAPTER,
            &config,
            "13.143.183.141".parse().unwrap(),
            &route
        )
        .is_err());

        let mut config = sample_config();
        config.addresses = vec!["fd00::2/128".to_string()];
        assert!(windows_setup_script(
            WINDOWS_ADAPTER,
            &config,
            "13.143.183.141".parse().unwrap(),
            &route
        )
        .is_err());
    }
}
