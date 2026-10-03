// Подмена Tauri-команд для визуальной проверки интерфейса в обычном браузере (не входит в сборку приложения).
const sub = {
  id: 1, status: 'active', is_trial: false, end_date: '2026-10-13T07:22:00Z', days_left: 10, hours_left: 4,
  time_left_display: '10д 4ч', traffic_limit_gb: 500, traffic_used_gb: 37.4, traffic_used_percent: 7.5,
  device_limit: 2, autopay_enabled: true, autopay_days_before: 3, is_active: true, is_expired: false,
  is_limited: false, is_daily: false, tariff_name: 'ЭКО', is_grace_period: false,
};
let protocol = 'vless';
const nodes = () =>
  protocol === 'wireguard'
    ? [['🇩🇪 Germany', 'wireguard'], ['🇺🇸 United States', 'wireguard'], ['🇨🇿 Czech Republic', 'wireguard']]
    : [['🇨🇿 Чехия', 'vless'], ['🇸🇪 Швеция', 'vless'], ['🇳🇱 Нидерланды', 'vless'], ['🇷🇺 Москва', 'vless']];
// Демо стартует как гость (без аккаунта); вход любыми данными открывает полное приложение. `?account` — сразу с аккаунтом.
let loggedIn = location.search.includes('account');
let connected: string | null = null;
let connectedAuto = false;
const settings = { auto: false, tunnel_mode: 'proxy', routing_mode: 'exclude', routing_apps: ['steam.exe'] as string[], elevated: false, telemetry: true };
const accountSource = { id: 'account', name: 'Zexor', removable: false };
const ownSources: { id: string; name: string; removable: boolean }[] = [];
let blockedCount = 128;
// В демо у обращения №7 есть непрочитанный ответ поддержки: видно красный значок и точку на вкладке.
try { if (!localStorage.getItem('zexor.support.seen')) localStorage.setItem('zexor.support.seen', JSON.stringify({ 7: 1 })); } catch { /* демо */ }
const tickets: any[] = [{ id: 7, title: 'Не подключается Чехия', status: 'answered', priority: 'normal', created_at: '2026-10-01T10:00:00Z', updated_at: '2026-10-02T09:30:00Z', closed_at: null, is_reply_blocked: false, messages: [{ id: 1, message_text: 'Здравствуйте, Чехия пишет n/a', is_from_admin: false, has_media: false, created_at: '2026-10-01T10:00:00Z' }, { id: 2, message_text: 'Добрый день! Обновите подписку и выберите Чехию заново — мы поменяли адрес.', is_from_admin: true, has_media: false, created_at: '2026-10-02T09:30:00Z' }] }];
const adblock = { enabled: true, use_remote: true, custom_block: ['ads.example.com'], custom_allow: ['good.example.org'], bundled_count: 34, remote_count: 61, remote_updated_at: Math.floor(Date.now() / 1000) - 7200, active_count: 0 };
const adblockView = () => ({ ...adblock, active_count: adblock.enabled ? 34 + (adblock.use_remote ? 61 : 0) + adblock.custom_block.length : 0 });
export async function invoke(cmd: string, args: any = {}): Promise<any> {
  await new Promise((r) => setTimeout(r, 120));
  switch (cmd) {
    case 'current_session': return loggedIn ? { email: 'user@example.com' } : null;
    case 'login': loggedIn = true; return { email: args.email || 'user@example.com' };
    case 'logout': loggedIn = false; return null;
    case 'list_sources': return loggedIn ? [accountSource, ...ownSources] : [accountSource, ...ownSources];
    case 'add_source': { const added = { id: 'x' + (ownSources.length + 1), name: args.name || 'Другой сервис', removable: true }; ownSources.push(added); return added; }
    case 'remove_source': { const i = ownSources.findIndex((x) => x.id === args.id); if (i >= 0) ownSources.splice(i, 1); return null; }
    case 'list_nodes': return { nodes: nodes().map(([remark, p]) => ({ remark, protocol: p, is_reality: p === 'vless' })) };
    case 'connection_status': return { connected: !!connected, node_remark: connected, source_id: connected ? 'account' : null, auto: connectedAuto };
    case 'connect': connected = args.nodeRemark; connectedAuto = false; return null;
    case 'disconnect': connected = null; connectedAuto = false; return null;
    case 'app_settings': return settings;
    case 'set_auto': settings.auto = args.enabled; if (args.enabled && connected && !connectedAuto) { connected = 'AUTO'; connectedAuto = true; } return settings;
    case 'set_tunnel_mode': if (args.mode === 'tun' && !settings.elevated) throw { kind: 'NeedsElevation', message: 'режиму TUN нужны права администратора' }; settings.tunnel_mode = args.mode; return settings;
    case 'restart_as_admin': settings.elevated = true; settings.tunnel_mode = 'tun'; return null;
    case 'set_native_labels': return null;
    case 'set_telemetry': settings.telemetry = args.enabled; return settings;
    case 'set_routing_mode': settings.routing_mode = args.mode; return settings;
    case 'add_routing_app': { const name = String(args.name).split(/[\\/]/).pop() as string; if (settings.routing_apps.includes(name)) throw { kind: 'Other', message: 'это приложение уже в списке' }; settings.routing_apps.push(name); return settings; }
    case 'remove_routing_app': settings.routing_apps = settings.routing_apps.filter((a) => a !== args.name); return settings;
    case 'list_running_apps': return ['chrome.exe', 'Discord.exe', 'steam.exe', 'Telegram.exe', 'Spotify.exe', 'Code.exe'];
    case 'adblock_settings': return adblockView();
    case 'set_adblock_enabled': adblock.enabled = args.enabled; return adblockView();
    case 'set_adblock_remote': adblock.use_remote = args.enabled; return adblockView();
    case 'add_adblock_domain': { const list = args.kind === 'block' ? adblock.custom_block : adblock.custom_allow; if (list.includes(args.domain)) throw { kind: 'Other', message: 'этот домен уже в списке' }; list.push(args.domain); return adblockView(); }
    case 'remove_adblock_domain': { const key = args.kind === 'block' ? 'custom_block' : 'custom_allow'; (adblock as any)[key] = (adblock as any)[key].filter((d: string) => d !== args.domain); return adblockView(); }
    case 'refresh_adblock_remote': adblock.remote_updated_at = Math.floor(Date.now() / 1000); return adblockView();
    case 'open_payment_url': return null;
    case 'adblock_session_stats': if (connected) blockedCount += 3; return { blocked: blockedCount, active: !!connected };
    case 'reset_adblock_session_stats': blockedCount = 0; return { blocked: 0, active: !!connected };
    case 'ping_nodes': return nodes().map(([remark], i) => ({ remark, ms: remark.includes('Москва') ? null : 28 + i * 87 }));
    case 'test_node': return args.nodeRemark.includes('Швеция') ? { ok: false, ms: null, error: 'сервер не ответил через туннель' } : { ok: true, ms: 142, error: null };
    case 'auto_connect': connected = 'AUTO'; connectedAuto = true; return { remark: 'AUTO', ms: null };
    case 'cabinet_request': {
      const p: string = args.path;
      if (p.endsWith('/subscription/info')) return { has_subscription: true, subscription: sub };
      if (p.endsWith('/subscription/protocol')) { if (args.method === 'POST') protocol = args.body.protocol; return { active_protocol: protocol, protocol_switch_available: true }; }
      if (p.includes('purchase-options')) return { sales_mode: 'tariffs', current_tariff_id: 5, currency: 'RUB', balance_kopeks: 12500, balance_usd_cents: 0, balance_eur_cents: 0, balance_label: '125 ₽', subscription_is_expired: false, has_subscription: true, tariffs: [
        { id: 5, name: 'ЭКО', description: 'Для одного-двух устройств, без излишеств', traffic_limit_label: '500 ГБ', device_limit: 2, servers_count: 6, is_current: true, is_available: true, is_daily: false, periods: [{ days: 30, months: 1, label: '1 месяц', price_kopeks: 19900, price_label: '199 ₽' }, { days: 90, months: 3, label: '3 месяца', price_kopeks: 49900, price_label: '499 ₽', discount_percent: 15, original_price_label: '597 ₽' }] },
        { id: 7, name: 'ПРЕМИУМ', description: 'Больше устройств и безлимитный трафик', traffic_limit_label: '♾️ Безлимит', device_limit: 5, servers_count: 10, is_current: false, is_available: true, is_daily: false, periods: [{ days: 30, months: 1, label: '1 месяц', price_kopeks: 39900, price_label: '399 ₽' }, { days: 365, months: 12, label: '1 год', price_kopeks: 349900, price_label: '3499 ₽', discount_percent: 27 }] },
      ] };
      if (p.endsWith('/tariff/switch/preview')) return { can_switch: true, new_tariff_name: 'ПРЕМИУМ', remaining_days: 10, upgrade_cost_kopeks: 30000, upgrade_cost_label: '300 ₽', has_enough_balance: false, missing_amount_kopeks: 17500, missing_amount_label: '175 ₽', is_upgrade: true };
      if (p.endsWith('/balance/payment-methods')) return [{ id: 'yookassa', name: 'Карта / СБП', description: 'ЮKassa', min_amount_kopeks: 10000, max_amount_kopeks: 10000000, is_available: true, options: null, quick_amounts: [10000, 30000, 50000] }];
      if (p.endsWith('/balance/topup')) return { payment_id: 'p1', payment_url: 'https://pay.example.com/checkout/p1', amount_kopeks: args.body.amount_kopeks, status: 'pending' };
      if (p.includes('renewal-options')) return [{ period_days: 30, price_kopeks: 19900, price_label: '199 ₽', discount_percent: 0 }, { period_days: 90, price_kopeks: 49900, price_label: '499 ₽', discount_percent: 15 }, { period_days: 365, price_kopeks: 169900, price_label: '1699 ₽', discount_percent: 30 }];
      if (p.endsWith('/subscription/devices')) return { devices: [{ hwid: 'abcdef123456', platform: 'Windows', device_model: 'Desktop', local_name: 'Мой ПК' }], total: 1, device_limit: 2 };
      if (p.includes('devices/price')) return { available: true, total_price_label: '99 ₽', can_add: 5 };
      if (p.includes('traffic-packages')) return [{ gb: 50, price_kopeks: 9900, is_unlimited: false, discount_percent: 0 }, { gb: 200, price_kopeks: 29900, is_unlimited: false, discount_percent: 0 }];
      if (p.endsWith('/auth/me')) return { id: 862, username: 'ivan', first_name: 'Иван', email: 'user@example.com', email_verified: true, balance_kopeks: 12500, balance_usd_cents: 0, balance_eur_cents: 0, currency: 'RUB', referral_code: 'ABC123', auth_type: 'telegram' };
      if (p.includes('/info/faq')) return [{ id: 1, title: 'Как подключиться?', content: '<p>Выберите сервер на главной и нажмите «Подключиться».</p>', order: 1 }, { id: 2, title: 'Сколько устройств можно подключить?', content: '<p>Столько, сколько указано в тарифе. Докупить можно на вкладке «Подписка».</p>', order: 2 }];
      if (p.includes('/tickets') && args.method === 'GET' && /tickets\?/.test(p)) return { items: tickets.map((t) => ({ ...t, messages_count: t.messages.length, last_message: t.messages[t.messages.length - 1] })), total: tickets.length };
      if (/\/tickets\/\d+\/messages$/.test(p)) { const t = tickets[0]; const m = { id: t.messages.length + 1, message_text: args.body.message, is_from_admin: false, has_media: false, created_at: new Date().toISOString() }; t.messages.push(m); return m; }
      if (/\/tickets\/\d+$/.test(p)) return tickets[0];
      if (p.endsWith('/tickets') && args.method === 'POST') { const t = { id: 8, title: args.body.title, status: 'open', priority: 'normal', created_at: new Date().toISOString(), updated_at: new Date().toISOString(), closed_at: null, is_reply_blocked: false, messages: [{ id: 1, message_text: args.body.message, is_from_admin: false, has_media: false, created_at: new Date().toISOString() }] }; tickets.unshift(t); return t; }
      if (p.endsWith('/referral')) return { referral_code: 'ABC123', total_referrals: 4, active_referrals: 2, total_earnings_kopeks: 38000, commission_percent: 10, referral_balance_kopeks: 0 };
      return null;
    }
    default: return null;
  }
}
