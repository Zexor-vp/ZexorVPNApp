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
  is_reality: boolean;
}

interface NodesResponse {
  nodes: NodeSummary[];
}

export const listNodes = async (): Promise<NodeSummary[]> =>
  (await invoke<NodesResponse>('list_nodes')).nodes;

export const connect = (nodeRemark: string): Promise<void> => invoke('connect', { nodeRemark });

export const disconnect = (): Promise<void> => invoke('disconnect');

export interface ConnectionStatus {
  connected: boolean;
  node_remark: string | null;
}

export const connectionStatus = (): Promise<ConnectionStatus> => invoke('connection_status');

export interface AdblockState {
  enabled: boolean;
}

export const adblockSettings = (): Promise<AdblockState> => invoke('adblock_settings');

export const setAdblockEnabled = (enabled: boolean): Promise<void> =>
  invoke('set_adblock_enabled', { enabled });
