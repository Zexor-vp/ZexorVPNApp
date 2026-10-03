// Типизированные обёртки над Tauri invoke() — единственное место, которое
// знает точные имена команд и форму их аргументов/ответов.
import { invoke } from '@tauri-apps/api/core';

export interface SessionInfo {
  email: string | null;
}

type CommandErrorKind =
  | 'NeedsLogin'
  | 'InvalidCredentials'
  | 'Network'
  | 'NoSubscription'
  | 'NeedsElevation'
  | 'Other';

interface CommandError {
  kind: CommandErrorKind;
  message: string;
}

function isCommandError(err: unknown): err is CommandError {
  return typeof err === 'object' && err !== null && 'kind' in err && 'message' in err;
}

/** Превращает ошибку команды в текст для пользователя. */
export function errorMessage(err: unknown): string {
  if (isCommandError(err)) {
    switch (err.kind) {
      case 'InvalidCredentials':
        return 'Неверный email или пароль.';
      case 'Network':
        return 'Нет связи с сервером. Проверьте интернет-соединение.';
      case 'NeedsLogin':
        return 'Сессия истекла — войдите снова.';
      case 'NoSubscription':
        return 'У аккаунта нет активной подписки.';
      default:
        return err.message;
    }
  }
  if (err instanceof Error) return err.message;
  return String(err);
}

/** true, если для операции нужны права администратора (режим TUN). */
export function needsElevation(err: unknown): boolean {
  return isCommandError(err) && err.kind === 'NeedsElevation';
}

/** true, если сервер отклонил операцию именно из-за истёкшей сессии. */
export function needsLogin(err: unknown): boolean {
  return isCommandError(err) && err.kind === 'NeedsLogin';
}

export const login = (email: string, password: string): Promise<SessionInfo> =>
  invoke('login', { email, password });

export const startTelegramLogin = (): Promise<{ token: string }> => invoke('start_telegram_login');

/** `null` — пользователь ещё не подтвердил вход в боте. */
export const pollTelegramLogin = (token: string): Promise<SessionInfo | null> =>
  invoke('poll_telegram_login', { token });

/** Открывает веб-страницу входа в браузере и возвращает `state` для поллинга. */
export const startBrowserLogin = (provider?: string): Promise<string> =>
  invoke('start_browser_login', { provider: provider ?? null });

/** `null` — пользователь ещё не завершил вход в браузере. */
export const pollBrowserLogin = (pairState: string): Promise<SessionInfo | null> =>
  invoke('poll_browser_login', { pairState });

export const logout = (): Promise<void> => invoke('logout');

export const currentSession = (): Promise<SessionInfo | null> => invoke('current_session');

export interface NodeSummary {
  remark: string;
  /** `vless` или `wireguard`. */
  protocol: 'vless' | 'wireguard' | string;
  is_reality: boolean;
}

interface NodesResponse {
  nodes: NodeSummary[];
}

/** Узлы выбранной подписки (`account` — подписка аккаунта). */
export const listNodes = async (sourceId?: string): Promise<NodeSummary[]> =>
  (await invoke<NodesResponse>('list_nodes', { sourceId: sourceId ?? null })).nodes;

export const connect = (nodeRemark: string, sourceId?: string): Promise<void> =>
  invoke('connect', { nodeRemark, sourceId: sourceId ?? null });

export const disconnect = (): Promise<void> => invoke('disconnect');

export interface ConnectionStatus {
  connected: boolean;
  node_remark: string | null;
  source_id: string | null;
  /** Подключение идёт в авто-режиме (балансировщик или самый быстрый сервер). */
  auto: boolean;
}

export const connectionStatus = (): Promise<ConnectionStatus> => invoke('connection_status');

export interface SourceSummary {
  id: string;
  name: string;
  removable: boolean;
}

export const ACCOUNT_SOURCE = 'account';

export const listSources = (): Promise<SourceSummary[]> => invoke('list_sources');

export const addSource = (name: string, url: string): Promise<SourceSummary> =>
  invoke('add_source', { name, url });

export const removeSource = (id: string): Promise<void> => invoke('remove_source', { id });

export interface AdblockState {
  enabled: boolean;
  use_remote: boolean;
  custom_block: string[];
  custom_allow: string[];
  bundled_count: number;
  remote_count: number;
  /** Unix-время последнего обновления дополнительного списка. */
  remote_updated_at: number | null;
  /** Сколько доменов блокируется прямо сейчас. */
  active_count: number;
}

export type AdblockListKind = 'block' | 'allow';

export const adblockSettings = (): Promise<AdblockState> => invoke('adblock_settings');

export const setAdblockEnabled = (enabled: boolean): Promise<AdblockState> =>
  invoke('set_adblock_enabled', { enabled });

export const setAdblockRemote = (enabled: boolean): Promise<AdblockState> =>
  invoke('set_adblock_remote', { enabled });

export const addAdblockDomain = (kind: AdblockListKind, domain: string): Promise<AdblockState> =>
  invoke('add_adblock_domain', { kind, domain });

export const removeAdblockDomain = (kind: AdblockListKind, domain: string): Promise<AdblockState> =>
  invoke('remove_adblock_domain', { kind, domain });

export const refreshAdblockRemote = (): Promise<AdblockState> => invoke('refresh_adblock_remote');

export interface SessionStats {
  /** Сколько рекламных соединений заблокировано с запуска приложения. */
  blocked: number;
  /** Идёт ли подсчёт прямо сейчас (VPN подключён). */
  active: boolean;
}

export const adblockSessionStats = (): Promise<SessionStats> => invoke('adblock_session_stats');

export const resetAdblockSessionStats = (): Promise<SessionStats> => invoke('reset_adblock_session_stats');

export interface NodePing {
  remark: string;
  /** Задержка в мс; `null` — сервер не отвечает. */
  ms: number | null;
}

/** Быстрый TCP-пинг всех серверов подписки. */
export const pingNodes = (sourceId?: string): Promise<NodePing[]> =>
  invoke('ping_nodes', { sourceId: sourceId ?? null });

export interface NodeTest {
  ok: boolean;
  ms: number | null;
  error: string | null;
}

/** Реальная проверка: запрос через туннель до этого сервера. */
export const testNode = (nodeRemark: string, sourceId?: string): Promise<NodeTest> =>
  invoke('test_node', { nodeRemark, sourceId: sourceId ?? null });

export interface AutoResult {
  /** `AUTO` у балансировщика или имя выбранного сервера. */
  remark: string;
  /** Задержка выбранного сервера; у балансировщика не определена. */
  ms: number | null;
}

/**
 * Авто-режим. На подписке аккаунта работает балансировщик (сам следит за серверами и включает
 * YouTube-серверы); на чужой подписке выбирается самый быстрый рабочий сервер.
 */
export const autoConnect = (sourceId?: string): Promise<AutoResult> =>
  invoke('auto_connect', { sourceId: sourceId ?? null });

/** Запрос к Cabinet API: токен подставляет Rust, фронтенд его не видит. */
export const cabinetRequest = <T = unknown>(
  method: 'GET' | 'POST' | 'PUT' | 'PATCH' | 'DELETE',
  path: string,
  body?: unknown,
): Promise<T> => invoke<T>('cabinet_request', { method, path, body: body ?? null });

/** Открывает страницу кабинета/Telegram в браузере (разрешены только свои домены). */
export const openExternal = (url: string): Promise<void> => invoke('open_external', { url });

/** Открывает страницу оплаты (`payment_url` от бэкенда) в браузере. */
export const openPaymentUrl = (url: string): Promise<void> => invoke('open_payment_url', { url });

export type TunnelMode = 'proxy' | 'tun';
export type RoutingMode = 'exclude' | 'only';

export interface AppSettings {
  /** Авто-выбор сервера включён. */
  auto: boolean;
  tunnel_mode: TunnelMode;
  routing_mode: RoutingMode;
  routing_apps: string[];
  /** Приложение запущено с правами администратора (нужны для TUN). */
  elevated: boolean;
  /** Отправка анонимных замеров доступности серверов Zexor. */
  telemetry: boolean;
}

export const appSettings = (): Promise<AppSettings> => invoke('app_settings');

export const setAuto = (enabled: boolean): Promise<AppSettings> => invoke('set_auto', { enabled });

/** TUN без прав администратора вернёт ошибку `NeedsElevation` — тогда предложите `restartAsAdmin`. */
export const setTunnelMode = (mode: TunnelMode): Promise<AppSettings> =>
  invoke('set_tunnel_mode', { mode });

/** Перезапускает приложение от имени администратора (окно UAC) с режимом TUN. */
export const restartAsAdmin = (): Promise<void> => invoke('restart_as_admin');

export const setRoutingMode = (mode: RoutingMode): Promise<AppSettings> =>
  invoke('set_routing_mode', { mode });

export const addRoutingApp = (name: string): Promise<AppSettings> => invoke('add_routing_app', { name });

export const removeRoutingApp = (name: string): Promise<AppSettings> =>
  invoke('remove_routing_app', { name });

/** Запущенные приложения пользователя — для выбора из списка. */
export const listRunningApps = (): Promise<string[]> => invoke('list_running_apps');

export const setTelemetry = (enabled: boolean): Promise<AppSettings> => invoke('set_telemetry', { enabled });
