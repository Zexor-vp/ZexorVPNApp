import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react';
import Button from '../components/Button';
import Modal from '../components/Modal';
import PageShell from '../components/PageShell';
import ProgressBar from '../components/ProgressBar';
import ProtocolMenu from '../components/ProtocolMenu';
import ModeSlider from '../components/ModeSlider';
import ServerPicker, { type PingValue } from '../components/ServerPicker';
import { BoltIcon, PlusIcon, TrashIcon } from '../components/Icons';
import { reportAuthLoss, useAsync } from '../hooks/useAsync';
import { getDevices, getProtocol, getSubscription, setProtocol } from '../lib/cabinet';
import {
  ACCOUNT_SOURCE,
  addSource,
  appSettings,
  autoConnect,
  connect,
  connectionStatus,
  disconnect,
  errorMessage,
  listNodes,
  listSources,
  needsElevation,
  openExternal,
  pingNodes,
  removeSource,
  restartAsAdmin,
  setAuto,
  setTunnelMode,
  type AppSettings,
  type ConnectionStatus,
  type TunnelMode,
  type NodeSummary,
  type SourceSummary,
} from '../lib/commands';
import { CABINET_URL } from '../lib/cabinet';

const STATUS_POLL_MS = 2000;

const selectionKey = (sourceId: string) => `zexor.server.${sourceId}`;
const HINT_KEY = 'zexor.hint.add-subscription';

function readSelection(sourceId: string): string | null {
  try {
    return window.localStorage.getItem(selectionKey(sourceId));
  } catch {
    return null;
  }
}

function writeSelection(sourceId: string, value: string) {
  try {
    window.localStorage.setItem(selectionKey(sourceId), value);
  } catch {
    // Запомнить выбор — удобство, а не необходимость.
  }
}

function formatDate(iso: string): string {
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? '' : date.toLocaleDateString('ru-RU');
}

interface Props {
  /** Меняется раз в час — по смене перезагружаем подписку, серверы и пинг. */
  refreshKey: number;
  onOpenSubscription: () => void;
  onOpenRouting: () => void;
}

export default function Home({ refreshKey, onOpenSubscription, onOpenRouting }: Props) {
  const [sources, setSources] = useState<SourceSummary[]>([]);
  const [sourceId, setSourceId] = useState<string>(ACCOUNT_SOURCE);
  const [nodes, setNodes] = useState<NodeSummary[]>([]);
  const [nodesError, setNodesError] = useState<string | null>(null);
  const [selectedNode, setSelectedNode] = useState('');
  const [pings, setPings] = useState<Record<string, PingValue>>({});
  const [pinging, setPinging] = useState(false);
  const [autoInfo, setAutoInfo] = useState<string | null>(null);
  const pingSeq = useRef(0);
  const [status, setStatus] = useState<ConnectionStatus>({
    connected: false,
    node_remark: null,
    source_id: null,
    auto: false,
  });
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [adminPrompt, setAdminPrompt] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [hintHidden, setHintHidden] = useState(() => {
    try {
      return window.localStorage.getItem(HINT_KEY) === '1';
    } catch {
      return false;
    }
  });
  const loadSeq = useRef(0);

  const isAccount = sourceId === ACCOUNT_SOURCE;

  const subscription = useAsync(() => getSubscription(), []);
  const protocol = useAsync(() => getProtocol(), []);
  const devices = useAsync(() => getDevices(), []);

  const loadNodes = useCallback(async (id: string) => {
    const mine = ++loadSeq.current;
    setNodesError(null);
    try {
      const list = await listNodes(id);
      if (mine !== loadSeq.current) return;
      setNodes(list);
      const saved = readSelection(id);
      setSelectedNode((current) => {
        if (list.some((n) => n.remark === current)) return current;
        if (saved && list.some((n) => n.remark === saved)) return saved;
        return list[0]?.remark ?? '';
      });
    } catch (err) {
      if (mine !== loadSeq.current) return;
      if (reportAuthLoss(err)) return;
      setNodes([]);
      setSelectedNode('');
      setNodesError(errorMessage(err));
    }
  }, []);

  const refreshPings = useCallback(async () => {
    const mine = ++pingSeq.current;
    setPinging(true);
    setPings((prev) => Object.fromEntries(Object.keys(prev).map((k) => [k, undefined])));
    try {
      const result = await pingNodes(sourceId);
      if (mine !== pingSeq.current) return;
      setPings(Object.fromEntries(result.map((r) => [r.remark, r.ms])));
    } catch (err) {
      if (mine === pingSeq.current && !reportAuthLoss(err)) setPings({});
    } finally {
      if (mine === pingSeq.current) setPinging(false);
    }
  }, [sourceId]);

  // Список подписок и текущее состояние подключения.
  useEffect(() => {
    void listSources().then(setSources).catch(() => undefined);
    void appSettings().then(setSettings).catch(() => undefined);
    void connectionStatus()
      .then((current) => {
        setStatus(current);
        if (current.connected && current.source_id) setSourceId(current.source_id);
        if (current.node_remark && !current.auto) setSelectedNode(current.node_remark);
      })
      .catch(() => undefined);
  }, []);

  // Почасовое обновление подписки: срок, трафик, устройства, протокол и список серверов.
  useEffect(() => {
    if (refreshKey === 0) return;
    void subscription.reload();
    void protocol.reload();
    void devices.reload();
    void listSources()
      .then(setSources)
      .catch(() => undefined);
    void loadNodes(sourceId).then(() => refreshPings());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [refreshKey]);

  useEffect(() => {
    setAutoInfo(null);
    void loadNodes(sourceId).then(() => refreshPings());
  }, [sourceId, loadNodes, refreshPings]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      connectionStatus()
        .then(setStatus)
        .catch(() => undefined);
    }, STATUS_POLL_MS);
    return () => window.clearInterval(timer);
  }, []);

  async function connectAuto() {
    setAutoInfo('Подбираем сервер…');
    const result = await autoConnect(sourceId);
    setStatus({ connected: true, node_remark: result.remark, source_id: sourceId, auto: true });
    setAutoInfo(result.ms === null ? 'Авто: сервер выбирается постоянно по доступности и скорости' : `Авто выбрал: ${result.remark} · ${result.ms} мс`);
  }

  async function handleToggleConnection() {
    setError(null);
    setBusy(true);
    try {
      if (status.connected) {
        await disconnect();
        setStatus({ connected: false, node_remark: null, source_id: null, auto: false });
        setAutoInfo(null);
      } else {
        if (autoOn) {
          await connectAuto();
        } else {
          if (!selectedNode) {
            setError('Нет доступных серверов в подписке.');
            return;
          }
          await connect(selectedNode, sourceId);
          setStatus({ connected: true, node_remark: selectedNode, source_id: sourceId, auto: false });
          setAutoInfo(null);
        }
      }
    } catch (err) {
      if (needsElevation(err)) setAdminPrompt(true);
      else if (!reportAuthLoss(err)) setError(errorMessage(err));
      setStatus(await connectionStatus().catch(() => status));
    } finally {
      setBusy(false);
    }
  }

  // Смена сервера на лету: Rust сам гасит прежний туннель перед новым подключением.
  async function handleNodeChange(value: string) {
    setSelectedNode(value);
    writeSelection(sourceId, value);
    setAutoInfo(null);
    if (!status.connected) return;
    setError(null);
    setBusy(true);
    try {
      await connect(value, sourceId);
      setStatus({ connected: true, node_remark: value, source_id: sourceId, auto: false });
    } catch (err) {
      if (!reportAuthLoss(err)) setError(errorMessage(err));
      setStatus(await connectionStatus().catch(() => status));
    } finally {
      setBusy(false);
    }
  }

  async function handleProtocolChange(next: 'vless' | 'wireguard') {
    setError(null);
    setBusy(true);
    try {
      if (status.connected) {
        await disconnect();
        setStatus({ connected: false, node_remark: null, source_id: null, auto: false });
      }
      await setProtocol(next);
      await Promise.all([protocol.reload(), loadNodes(sourceId)]);
    } catch (err) {
      if (!reportAuthLoss(err)) setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function handleAutoToggle() {
    if (!settings) return;
    const next = !settings.auto;
    setError(null);
    setBusy(true);
    try {
      setSettings(await setAuto(next));
      setStatus(await connectionStatus());
      if (!next) setAutoInfo(null);
    } catch (err) {
      if (needsElevation(err)) setAdminPrompt(true);
      else if (!reportAuthLoss(err)) setError(errorMessage(err));
      setSettings(await appSettings().catch(() => settings));
      setStatus(await connectionStatus().catch(() => status));
    } finally {
      setBusy(false);
    }
  }

  async function handleModeChange(mode: TunnelMode) {
    setError(null);
    setBusy(true);
    try {
      setSettings(await setTunnelMode(mode));
      setStatus(await connectionStatus());
    } catch (err) {
      if (needsElevation(err)) setAdminPrompt(true);
      else if (!reportAuthLoss(err)) setError(errorMessage(err));
      setStatus(await connectionStatus().catch(() => status));
    } finally {
      setBusy(false);
    }
  }

  async function handleRestartAsAdmin() {
    setAdminPrompt(false);
    try {
      await restartAsAdmin();
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  async function handleRemoveSource() {
    if (isAccount) return;
    const name = sources.find((s) => s.id === sourceId)?.name ?? 'подписку';
    if (!window.confirm(`Удалить «${name}» из приложения?`)) return;
    try {
      await removeSource(sourceId);
      setSources(await listSources());
      setSourceId(ACCOUNT_SOURCE);
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  function hideHint() {
    setHintHidden(true);
    try {
      window.localStorage.setItem(HINT_KEY, '1');
    } catch {
      // подсказка просто покажется снова после перезапуска
    }
  }

  async function handleSourceAdded(added: SourceSummary) {
    setAddOpen(false);
    setSources(await listSources());
    setSourceId(added.id);
  }

  const autoOn = settings?.auto ?? false;
  const tunnelMode = settings?.tunnel_mode ?? 'proxy';
  const sub = subscription.data?.subscription ?? null;
  const hasSubscription = subscription.data?.has_subscription ?? false;
  const protocolInfo = protocol.data;
  const canSwitchProtocol = isAccount && !!protocolInfo?.protocol_switch_available;
  const activeProtocol = (protocolInfo?.active_protocol ?? 'vless') as 'vless' | 'wireguard';

  return (
    <PageShell
      actions={
        <button className="icon-btn" aria-label="Добавить подписку" title="Добавить свою подписку" onClick={() => setAddOpen(true)}>
          <PlusIcon />
        </button>
      }
    >
      {sources.length <= 1 && !hintHidden && (
        <div className="hint-bar" role="note">
          <span>
            Нажмите <strong>+</strong> в правом верхнем углу, чтобы добавить другую подписку
          </span>
          <button className="hint-close" aria-label="Скрыть подсказку" onClick={hideHint}>
            ✕
          </button>
        </div>
      )}

      {sources.length > 1 && (
        <select
          className="node-select"
          aria-label="Подписка"
          value={sourceId}
          disabled={busy}
          onChange={(e) => setSourceId(e.target.value)}
        >
          {sources.map((source) => (
            <option key={source.id} value={source.id}>
              {source.name}
            </option>
          ))}
        </select>
      )}

      <section className="card card-glow card-raised">
        <div className="row">
          <ModeSlider value={tunnelMode} disabled={busy || !settings} onChange={(mode) => void handleModeChange(mode)} />
          <div className="row" style={{ gap: '0.5rem' }}>
            {canSwitchProtocol && <ProtocolMenu value={activeProtocol} disabled={busy} onChange={handleProtocolChange} />}
            {!isAccount && (
              <button className="icon-btn" aria-label="Удалить подписку" title="Удалить подписку" onClick={handleRemoveSource}>
                <TrashIcon />
              </button>
            )}
          </div>
        </div>
        {canSwitchProtocol && activeProtocol === 'wireguard' && <p className="warn-text">WireGuard не работает в России и Иране</p>}

        <div className="picker-row">
          <div className="picker-main">
            <ServerPicker
              nodes={nodes}
              selected={selectedNode}
              pings={pings}
              pinging={pinging}
              disabled={busy}
              auto={autoOn}
              onSelect={(value) => void handleNodeChange(value)}
              onRefreshPings={() => void refreshPings()}
            />
          </div>
          <button
            type="button"
            className={`auto-btn ${autoOn ? 'auto-btn-on' : ''}`}
            aria-pressed={autoOn}
            disabled={busy || !settings}
            title="Клиент сам выбирает лучший сервер"
            onClick={() => void handleAutoToggle()}
          >
            <BoltIcon /> Авто
          </button>
        </div>
        <Button
          variant={status.connected ? 'danger' : 'primary'}
          loading={busy}
          disabled={!status.connected && !autoOn && !selectedNode}
          onClick={handleToggleConnection}
          className="connect-btn"
        >
          {status.connected ? 'Отключиться' : 'Подключиться'}
        </Button>

        {autoInfo && (status.connected || busy) && <p className="muted" style={{ margin: 0 }}>{autoInfo}</p>}
        {(error || nodesError) && <p className="form-error">{error ?? nodesError}</p>}
      </section>

      <section className="card">
        <div className="row">
          <span className="label">Маршрутизация</span>
          {settings && settings.routing_apps.length > 0 && (
            <span className="chip">{settings.routing_mode === 'only' ? 'Только ' : 'Кроме '}{settings.routing_apps.length}</span>
          )}
        </div>
        <p className="muted" style={{ margin: 0 }}>
          Выберите, какие приложения идут через VPN, а какие — напрямую, в обход него.
        </p>
        <Button variant="secondary" onClick={onOpenRouting}>
          Настроить маршрутизацию
        </Button>
      </section>

      {isAccount && (
        <section className="card">
          {sub ? (
            <>
              <div className="row">
                <div>
                  <span className="label">Расход трафика</span>
                  <div className="row" style={{ justifyContent: 'flex-start', marginTop: '0.4rem' }}>
                    {sub.is_trial && <span className="chip">Пробный период</span>}
                    {sub.is_grace_period && <span className="chip chip-warn">Грейс</span>}
                    {sub.is_expired && <span className="chip chip-danger">Истекла</span>}
                  </div>
                </div>
                <div className="big-number">
                  {sub.traffic_limit_gb > 0 ? Math.round(sub.traffic_used_percent) : '∞'}
                  {sub.traffic_limit_gb > 0 && <small>%</small>}
                </div>
              </div>
              {sub.traffic_limit_gb > 0 && <ProgressBar percent={sub.traffic_used_percent} />}
              <p className="muted" style={{ margin: 0 }}>
                {sub.traffic_used_gb.toFixed(1)} ГБ{sub.traffic_limit_gb > 0 ? ` из ${sub.traffic_limit_gb} ГБ` : ' · безлимит'}
              </p>
              <div className="tile-grid">
                <div className="tile">
                  <span className="label">Тариф</span>
                  <span className="tile-value">{sub.tariff_name ?? 'Подписка'}</span>
                  <span className="muted">до {formatDate(sub.end_date)}</span>
                </div>
                <div className={`tile ${sub.days_left <= 3 ? 'tile-warn' : ''}`}>
                  <span className="label">Осталось</span>
                  <span className="tile-value">{sub.days_left > 0 ? `${sub.days_left} дн.` : sub.time_left_display || '0 дн.'}</span>
                  <span className="muted">
                    устройств: {devices.data ? `${devices.data.total} из ${sub.device_limit}` : sub.device_limit}
                  </span>
                </div>
              </div>
              <Button variant="secondary" onClick={onOpenSubscription}>
                Управление подпиской
              </Button>
            </>
          ) : subscription.loading ? (
            <div className="empty">
              <span className="spinner spinner-lg" aria-hidden />
            </div>
          ) : subscription.error ? (
            <p className="form-error">{subscription.error}</p>
          ) : (
            !hasSubscription && (
              <>
                <p className="empty">У аккаунта нет активной подписки.</p>
                <Button onClick={() => void openExternal(`${CABINET_URL}/subscription`)}>Оформить подписку</Button>
              </>
            )
          )}
        </section>
      )}

      {adminPrompt && (
        <Modal title="Нужны права администратора" onClose={() => setAdminPrompt(false)}>
          <p className="muted" style={{ margin: 0 }}>
            Режиму TUN нужно создать виртуальный сетевой адаптер — для этого Windows требует права администратора. Перезапустить
            приложение с правами администратора? VPN отключится и включится заново после запуска.
          </p>
          <div className="modal-actions">
            <Button type="button" variant="ghost" onClick={() => setAdminPrompt(false)}>
              Отмена
            </Button>
            <Button type="button" onClick={() => void handleRestartAsAdmin()}>
              Перезапустить
            </Button>
          </div>
        </Modal>
      )}

      {addOpen && <AddSourceModal onClose={() => setAddOpen(false)} onAdded={handleSourceAdded} />}
    </PageShell>
  );
}

function AddSourceModal({ onClose, onAdded }: { onClose: () => void; onAdded: (s: SourceSummary) => void }) {
  const [name, setName] = useState('');
  const [url, setUrl] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setError(null);
    setLoading(true);
    try {
      onAdded(await addSource(name, url));
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }

  return (
    <Modal title="Добавить подписку" onClose={onClose}>
      <form onSubmit={submit} style={{ display: 'flex', flexDirection: 'column', gap: '0.75rem' }}>
        <p className="muted" style={{ margin: 0 }}>
          Вставьте ссылку подписки другого сервиса — её серверы появятся в этом приложении рядом с вашими.
        </p>
        <input
          className="text-input"
          placeholder="Название (необязательно)"
          value={name}
          maxLength={40}
          onChange={(e) => setName(e.target.value)}
        />
        <input
          className="text-input"
          placeholder="https://…"
          value={url}
          autoFocus
          onChange={(e) => setUrl(e.target.value)}
        />
        {error && <p className="form-error">{error}</p>}
        <div className="modal-actions">
          <Button type="button" variant="ghost" onClick={onClose}>
            Отмена
          </Button>
          <Button type="submit" loading={loading} disabled={!url.trim()}>
            Добавить
          </Button>
        </div>
      </form>
    </Modal>
  );
}
