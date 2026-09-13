import { useCallback, useEffect, useRef, useState } from 'react';
import { check as checkForUpdate, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import Button from '../components/Button';
import StatusPill from '../components/StatusPill';
import Toggle from '../components/Toggle';
import {
  adblockSettings,
  connect,
  connectionStatus,
  disconnect,
  errorMessage,
  listNodes,
  logout,
  needsLogin,
  setAdblockEnabled,
  type ConnectionStatus,
  type NodeSummary,
  type SessionInfo,
} from '../lib/commands';

const STATUS_POLL_MS = 2000;

type UpdateState =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'up-to-date' }
  | { kind: 'available'; update: Update }
  | { kind: 'installing' }
  | { kind: 'error'; message: string };

interface Props {
  session: SessionInfo;
  onLoggedOut: () => void;
}

export default function Dashboard({ session, onLoggedOut }: Props) {
  const [nodes, setNodes] = useState<NodeSummary[]>([]);
  const [selectedNode, setSelectedNode] = useState<string>('');
  const [status, setStatus] = useState<ConnectionStatus>({ connected: false, node_remark: null });
  const [adblockEnabled, setAdblockEnabledState] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [updateState, setUpdateState] = useState<UpdateState>({ kind: 'idle' });
  const pollRef = useRef<number | undefined>(undefined);

  const handleAuthLoss = useCallback(
    (err: unknown) => {
      if (needsLogin(err)) {
        onLoggedOut();
        return true;
      }
      return false;
    },
    [onLoggedOut],
  );

  useEffect(() => {
    let cancelled = false;

    async function load() {
      try {
        const [nodeList, adblock, currentStatus] = await Promise.all([
          listNodes(),
          adblockSettings(),
          connectionStatus(),
        ]);
        if (cancelled) return;
        setNodes(nodeList);
        setAdblockEnabledState(adblock.enabled);
        setStatus(currentStatus);
        setSelectedNode(currentStatus.node_remark ?? nodeList[0]?.remark ?? '');
      } catch (err) {
        if (cancelled) return;
        if (!handleAuthLoss(err)) setError(errorMessage(err));
      }
    }

    load();
    return () => {
      cancelled = true;
    };
  }, [handleAuthLoss]);

  useEffect(() => {
    pollRef.current = window.setInterval(async () => {
      try {
        const next = await connectionStatus();
        setStatus(next);
      } catch {
        // Опрос статуса не критичен — просто попробуем на следующем тике.
      }
    }, STATUS_POLL_MS);
    return () => window.clearInterval(pollRef.current);
  }, []);

  async function handleToggleConnection() {
    setError(null);
    setBusy(true);
    try {
      if (status.connected) {
        await disconnect();
        setStatus({ connected: false, node_remark: null });
      } else {
        if (!selectedNode) {
          setError('Нет доступных серверов в подписке.');
          return;
        }
        await connect(selectedNode);
        setStatus({ connected: true, node_remark: selectedNode });
      }
    } catch (err) {
      if (!handleAuthLoss(err)) setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function handleAdblockToggle(next: boolean) {
    const previous = adblockEnabled;
    setAdblockEnabledState(next);
    try {
      await setAdblockEnabled(next);
    } catch (err) {
      setAdblockEnabledState(previous);
      if (!handleAuthLoss(err)) setError(errorMessage(err));
    }
  }

  async function handleLogout() {
    try {
      await logout();
    } finally {
      onLoggedOut();
    }
  }

  async function handleCheckUpdate() {
    setUpdateState({ kind: 'checking' });
    try {
      const update = await checkForUpdate();
      setUpdateState(update ? { kind: 'available', update } : { kind: 'up-to-date' });
    } catch (err) {
      setUpdateState({ kind: 'error', message: errorMessage(err) });
    }
  }

  async function handleInstallUpdate(update: Update) {
    setUpdateState({ kind: 'installing' });
    try {
      await update.downloadAndInstall();
      await relaunch();
    } catch (err) {
      setUpdateState({ kind: 'error', message: errorMessage(err) });
    }
  }

  return (
    <main className="app-shell dashboard-shell">
      <header className="dashboard-header">
        <div className="brand">
          <span className="brand-dot" />
          <h1>Zexor VPN</h1>
        </div>
        <button className="link-btn" onClick={handleLogout}>
          {session.email ?? 'Выйти'}
        </button>
      </header>

      <section className="card connection-card">
        <StatusPill connected={status.connected} pending={busy} />

        <select
          className="node-select"
          value={selectedNode}
          disabled={status.connected || busy || nodes.length === 0}
          onChange={(e) => setSelectedNode(e.target.value)}
        >
          {nodes.length === 0 && <option value="">Нет серверов</option>}
          {nodes.map((node) => (
            <option key={node.remark} value={node.remark}>
              {node.remark}
              {node.is_reality ? ' · REALITY' : ''}
            </option>
          ))}
        </select>

        <Button
          variant={status.connected ? 'danger' : 'primary'}
          loading={busy}
          disabled={!status.connected && !selectedNode}
          onClick={handleToggleConnection}
          className="connect-btn"
        >
          {status.connected ? 'Отключиться' : 'Подключиться'}
        </Button>

        {error && <p className="form-error">{error}</p>}
      </section>

      <section className="card settings-card">
        <Toggle
          label="Блокировщик рекламы"
          hint="Работает локально, не зависит от сервера"
          checked={adblockEnabled}
          onChange={handleAdblockToggle}
        />
      </section>

      <section className="card update-card">
        <div className="update-row">
          <span>Обновления</span>
          <Button
            variant="secondary"
            loading={updateState.kind === 'checking' || updateState.kind === 'installing'}
            onClick={handleCheckUpdate}
          >
            Проверить обновления
          </Button>
        </div>
        {updateState.kind === 'up-to-date' && <p className="update-note">Установлена последняя версия.</p>}
        {updateState.kind === 'error' && <p className="form-error">{updateState.message}</p>}
        {updateState.kind === 'available' && (
          <div className="update-available">
            <p>Доступна версия {updateState.update.version}.</p>
            <Button variant="primary" onClick={() => handleInstallUpdate(updateState.update)}>
              Установить и перезапустить
            </Button>
          </div>
        )}
      </section>
    </main>
  );
}
