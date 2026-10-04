import { useCallback, useEffect, useRef, useState, type FormEvent } from 'react';
import { listen } from '@tauri-apps/api/event';
import { currentLocale, useT } from '../i18n';
import Button from '../components/Button';
import Modal from '../components/Modal';
import PageShell from '../components/PageShell';
import ProgressBar from '../components/ProgressBar';
import ProtocolMenu from '../components/ProtocolMenu';
import ModeSlider from '../components/ModeSlider';
import { isMobile } from '../lib/platform';
import ServerPicker, { type PingValue } from '../components/ServerPicker';
import { BoltIcon, PlusIcon, RefreshIcon, TrashIcon } from '../components/Icons';
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
import { crashReport } from '../lib/commands';

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
  return Number.isNaN(date.getTime()) ? '' : date.toLocaleDateString(currentLocale());
}

interface Props {
  /** Меняется раз в час — по смене перезагружаем подписку, серверы и пинг. */
  refreshKey: number;
  onOpenSubscription: () => void;
  onOpenRouting: () => void;
  /** Без аккаунта: только свои подписки (кнопка «+»), без доступа к сервису Zexor. */
  guest?: boolean;
}

export default function Home({ refreshKey, onOpenSubscription, onOpenRouting, guest = false }: Props) {
  const t = useT();
  const [sources, setSources] = useState<SourceSummary[]>([]);
  // Гость начинает без подписки: подписка аккаунта ему недоступна, свою он добавляет сам.
  const [sourceId, setSourceId] = useState<string>(guest ? '' : ACCOUNT_SOURCE);
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
  const [crash, setCrash] = useState('');
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Android: если прошлый запуск завершился аварийно, показываем причину — по ней можно понять, что случилось.
  useEffect(() => {
    if (isMobile) void crashReport().then(setCrash).catch(() => undefined);
  }, []);
  const [addOpen, setAddOpen] = useState(false);
  const [hintHidden, setHintHidden] = useState(() => {
    try {
      return window.localStorage.getItem(HINT_KEY) === '1';
    } catch {
      return false;
    }
  });
  const loadSeq = useRef(0);

  const isAccount = !guest && sourceId === ACCOUNT_SOURCE;

  // Список подписок: гостю подписка аккаунта не показывается.
  const visibleSources = useCallback(
    (list: SourceSummary[]) => (guest ? list.filter((s) => s.id !== ACCOUNT_SOURCE) : list),
    [guest],
  );

  // Данные аккаунта гость не запрашивает: без сессии сервер ответил бы «нужен вход».
  const subscription = useAsync(() => (guest ? Promise.resolve(null) : getSubscription()), [guest]);
  const protocol = useAsync(() => (guest ? Promise.resolve(null) : getProtocol()), [guest]);
  const devices = useAsync(() => (guest ? Promise.resolve(null) : getDevices()), [guest]);

  // Порядок серверов случайный: при каждом заходе на главную список перемешивается заново (как в Happ), а пока
  // страница открыта, он не прыгает при фоновых обновлениях — у каждого сервера свой случайный ключ на всё посещение.
  const shuffleKeys = useRef(new Map<string, number>());
  const shuffled = useCallback(<T extends { remark: string }>(list: T[]): T[] => {
    const keys = shuffleKeys.current;
    for (const item of list) {
      if (!keys.has(item.remark)) keys.set(item.remark, Math.random());
    }
    return [...list].sort((a, b) => (keys.get(a.remark) ?? 0) - (keys.get(b.remark) ?? 0));
  }, []);

  const loadNodes = useCallback(async (id: string) => {
    const mine = ++loadSeq.current;
    setNodesError(null);
    if (!id) {
      setNodes([]);
      setSelectedNode('');
      return;
    }
    try {
      const list = shuffled(await listNodes(id));
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
  }, [shuffled]);

  const refreshPings = useCallback(async () => {
    if (!sourceId) return;
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
    void listSources()
      .then((list) => {
        const shown = visibleSources(list);
        setSources(shown);
        // Гость: сразу выбираем первую свою подписку, если она уже есть.
        if (guest) setSourceId((current) => current || shown[0]?.id || '');
      })
      .catch(() => undefined);
    void appSettings().then(setSettings).catch(() => undefined);
    void connectionStatus()
      .then((current) => {
        setStatus(current);
        if (current.connected && current.source_id && !(guest && current.source_id === ACCOUNT_SOURCE)) {
          setSourceId(current.source_id);
        }
        if (current.node_remark && !current.auto) setSelectedNode(current.node_remark);
      })
      .catch(() => undefined);
  }, []);

  // Обновление подписки: срок, трафик, устройства, протокол и список серверов. Запускается раз в час,
  // кнопкой «Обновить» и по команде от сервера (см. событие ниже) — без перезапуска приложения.
  async function refreshAll() {
    setRefreshing(true);
    setError(null);
    try {
      await Promise.all([
        subscription.reload(),
        protocol.reload(),
        devices.reload(),
        listSources()
          .then((list) => setSources(visibleSources(list)))
          .catch(() => undefined),
        loadNodes(sourceId).then(() => refreshPings()),
      ]);
    } finally {
      setRefreshing(false);
    }
  }
  const refreshRef = useRef(refreshAll);
  refreshRef.current = refreshAll;

  useEffect(() => {
    if (refreshKey === 0) return;
    void refreshRef.current();
  }, [refreshKey]);

  // Быстрое включение с плитки/виджета не удалось (нет входа, нет разрешения на VPN...) — показываем причину.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    listen<string>('quick-error', (event) => setError(t(String(event.payload))))
      .then((off) => {
        if (cancelled) off();
        else unlisten = off;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Админ нажал «обновить подписки в приложениях» — Rust сообщает об этом событием.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    listen('subscription-changed', () => void refreshRef.current())
      .then((off) => {
        if (cancelled) off();
        else unlisten = off;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

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
    setAutoInfo(t('Подбираем сервер…'));
    const result = await autoConnect(sourceId);
    setStatus({ connected: true, node_remark: result.remark, source_id: sourceId, auto: true });
    setAutoInfo(
      result.ms === null
        ? t('Авто: сервер выбирается постоянно по доступности и скорости')
        : t('Авто выбрал: {server} · {ms} мс', { server: result.remark, ms: result.ms }),
    );
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
            setError(t('Нет доступных серверов в подписке.'));
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
    const name = sources.find((s) => s.id === sourceId)?.name ?? t('подписку');
    if (!window.confirm(t('Удалить «{name}» из приложения?', { name }))) return;
    try {
      await removeSource(sourceId);
      const rest = visibleSources(await listSources());
      setSources(rest);
      setSourceId(guest ? (rest[0]?.id ?? '') : ACCOUNT_SOURCE);
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
    setSources(visibleSources(await listSources()));
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
        <>
          <button
            className={`icon-btn ${refreshing ? 'icon-btn-spin' : ''}`}
            aria-label={t('Обновить')}
            title={t('Обновить подписку и список серверов')}
            disabled={refreshing}
            onClick={() => void refreshAll()}
          >
            <RefreshIcon />
          </button>
          <button
            className="icon-btn"
            aria-label={t('Добавить подписку')}
            title={t('Добавить свою подписку')}
            onClick={() => setAddOpen(true)}
          >
            <PlusIcon />
          </button>
        </>
      }
    >
      {crash && (
        <div className="hint-bar" role="alert" style={{ alignItems: 'flex-start' }}>
          <span style={{ overflowWrap: 'anywhere', whiteSpace: 'pre-wrap', fontSize: '0.8rem' }}>
            {t('Приложение аварийно завершилось в прошлый раз. Сделайте скриншот этого сообщения и отправьте в поддержку:')}
            {'\n'}
            {crash}
          </span>
          <button className="hint-close" aria-label={t('Скрыть подсказку')} onClick={() => setCrash('')}>
            ✕
          </button>
        </div>
      )}

      {!guest && sources.length <= 1 && !hintHidden && (
        <div className="hint-bar" role="note">
          <span>
            {t('Нажмите + в правом верхнем углу, чтобы добавить другую подписку')}
          </span>
          <button className="hint-close" aria-label={t('Скрыть подсказку')} onClick={hideHint}>
            ✕
          </button>
        </div>
      )}

      {sources.length > 1 && (
        <select
          className="node-select"
          aria-label={t('Подписка')}
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

      {guest && sources.length === 0 ? (
        <section className="card card-glow">
          <span className="label">{t('Своя подписка')}</span>
          <p style={{ margin: 0 }}>{t('Добавьте ссылку подписки любого сервиса — её серверы появятся здесь, и можно подключаться.')}</p>
          <Button onClick={() => setAddOpen(true)}>{t('Добавить подписку')}</Button>
          <p className="muted" style={{ margin: 0 }}>
            {t('Хотите VPN Zexor? Откройте вкладку «Вход», войдите или зарегистрируйтесь — и сервис Zexor станет доступен.')}
          </p>
          <Button variant="secondary" onClick={onOpenSubscription}>
            {t('Войти или зарегистрироваться')}
          </Button>
        </section>
      ) : (
      <section className="card card-glow card-raised">
        <div className="row">
          {isMobile ? (
            <span />
          ) : (
            <ModeSlider value={tunnelMode} disabled={busy || !settings} onChange={(mode) => void handleModeChange(mode)} />
          )}
          <div className="row" style={{ gap: '0.5rem' }}>
            {canSwitchProtocol && <ProtocolMenu value={activeProtocol} disabled={busy} onChange={handleProtocolChange} />}
            {!isAccount && (
              <button
                className="icon-btn"
                aria-label={t('Удалить подписку')}
                title={t('Удалить подписку')}
                onClick={handleRemoveSource}
              >
                <TrashIcon />
              </button>
            )}
          </div>
        </div>
        {canSwitchProtocol && activeProtocol === 'wireguard' && <p className="warn-text">{t('WireGuard не работает в России и Иране')}</p>}

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
            title={t('Клиент сам выбирает лучший сервер')}
            onClick={() => void handleAutoToggle()}
          >
            <BoltIcon /> {t('Авто')}
          </button>
        </div>
        <Button
          variant={status.connected ? 'danger' : 'primary'}
          loading={busy}
          disabled={!status.connected && !autoOn && !selectedNode}
          onClick={handleToggleConnection}
          className="connect-btn"
        >
          {status.connected ? t('Отключиться') : t('Подключиться')}
        </Button>

        {autoInfo && (status.connected || busy) && <p className="muted" style={{ margin: 0 }}>{autoInfo}</p>}
        {(error || nodesError) && <p className="form-error">{error ?? nodesError}</p>}
      </section>
      )}

      {isAccount && (
        <section className="card">
          {sub ? (
            <>
              <div className="row">
                <div>
                  <span className="label">{t('Расход трафика')}</span>
                  <div className="row" style={{ justifyContent: 'flex-start', marginTop: '0.4rem' }}>
                    {sub.is_trial && <span className="chip">{t('Пробный период')}</span>}
                    {sub.is_grace_period && <span className="chip chip-warn">{t('Грейс')}</span>}
                    {sub.is_expired && <span className="chip chip-danger">{t('Истекла')}</span>}
                  </div>
                </div>
                <div className="big-number">
                  {sub.traffic_limit_gb > 0 ? Math.round(sub.traffic_used_percent) : '∞'}
                  {sub.traffic_limit_gb > 0 && <small>%</small>}
                </div>
              </div>
              {sub.traffic_limit_gb > 0 && <ProgressBar percent={sub.traffic_used_percent} />}
              <p className="muted" style={{ margin: 0 }}>
                {sub.traffic_limit_gb > 0
                  ? t('{used} ГБ из {limit} ГБ', { used: sub.traffic_used_gb.toFixed(1), limit: sub.traffic_limit_gb })
                  : t('{used} ГБ · безлимит', { used: sub.traffic_used_gb.toFixed(1) })}
              </p>
              <div className="tile-grid">
                <div className="tile">
                  <span className="label">{t('Тариф')}</span>
                  <span className="tile-value">{sub.tariff_name ?? t('Подписка')}</span>
                  <span className="muted">{t('до {date}', { date: formatDate(sub.end_date) })}</span>
                </div>
                <div className={`tile ${sub.days_left <= 3 ? 'tile-warn' : ''}`}>
                  <span className="label">{t('Осталось')}</span>
                  <span className="tile-value">{sub.days_left > 0 ? t('{n} дн.', { n: sub.days_left }) : sub.time_left_display || t('0 дн.')}</span>
                  <span className="muted">
                    {t('устройств: {count}', {
                      count: devices.data ? t('{used} из {limit}', { used: devices.data.total, limit: sub.device_limit }) : sub.device_limit,
                    })}
                  </span>
                </div>
              </div>
              <Button variant="secondary" onClick={onOpenSubscription}>
                {t('Управление подпиской')}
              </Button>
            </>
          ) : subscription.loading ? (
            <div className="empty">
              <span className="spinner spinner-lg" aria-hidden />
            </div>
          ) : subscription.error ? (
            // Та же ошибка уже показана в карточке подключения — второй раз её не повторяем.
            subscription.error === (error ?? nodesError) ? (
              <p className="muted">{t('Данные подписки появятся, когда восстановится связь.')}</p>
            ) : (
              <p className="form-error">{subscription.error}</p>
            )
          ) : (
            !hasSubscription && (
              <>
                <p className="empty">{t('У аккаунта нет активной подписки.')}</p>
                <Button onClick={() => void openExternal(`${CABINET_URL}/subscription`)}>{t('Оформить подписку')}</Button>
              </>
            )
          )}
        </section>
      )}

      {!isMobile && (
      <section className="card">
        <div className="row">
          <span className="label">{t('Маршрутизация')}</span>
          {settings && settings.routing_apps.length > 0 && (
            <span className="chip">
              {settings.routing_mode === 'only' ? t('Только') : t('Кроме')} {settings.routing_apps.length}
            </span>
          )}
        </div>
        <p className="muted" style={{ margin: 0 }}>
          {t('Выберите, какие приложения идут через VPN, а какие — напрямую, в обход него.')}
        </p>
        <Button variant="secondary" onClick={onOpenRouting}>
          {t('Настроить маршрутизацию')}
        </Button>
      </section>
      )}

      {adminPrompt && (
        <Modal title={t('Нужны права администратора')} onClose={() => setAdminPrompt(false)}>
          <p className="muted" style={{ margin: 0 }}>
            {t('Режиму TUN нужно создать виртуальный сетевой адаптер — для этого Windows требует права администратора. Перезапустить приложение с правами администратора? VPN отключится и включится заново после запуска.')}
          </p>
          <div className="modal-actions">
            <Button type="button" variant="ghost" onClick={() => setAdminPrompt(false)}>
              {t('Отмена')}
            </Button>
            <Button type="button" onClick={() => void handleRestartAsAdmin()}>
              {t('Перезапустить')}
            </Button>
          </div>
        </Modal>
      )}

      {addOpen && <AddSourceModal onClose={() => setAddOpen(false)} onAdded={handleSourceAdded} />}
    </PageShell>
  );
}

function AddSourceModal({ onClose, onAdded }: { onClose: () => void; onAdded: (s: SourceSummary) => void }) {
  const t = useT();
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
    <Modal title={t('Добавить подписку')} onClose={onClose}>
      <form onSubmit={submit} style={{ display: 'flex', flexDirection: 'column', gap: '0.75rem' }}>
        <p className="muted" style={{ margin: 0 }}>
          {t('Вставьте ссылку подписки другого сервиса или конфиг AmneziaWG (.conf) — серверы появятся в этом приложении рядом с вашими.')}
        </p>
        <input
          className="text-input"
          placeholder={t('Название (необязательно)')}
          value={name}
          maxLength={40}
          onChange={(e) => setName(e.target.value)}
        />
        <textarea
          className="text-input text-area"
          placeholder={t('https://… или текст конфига AmneziaWG')}
          rows={3}
          value={url}
          autoFocus
          onChange={(e) => setUrl(e.target.value)}
        />
        <label className="link-btn" style={{ alignSelf: 'flex-start', cursor: 'pointer' }}>
          {t('Выбрать файл .conf')}
          <input
            type="file"
            accept=".conf,.txt,text/plain"
            hidden
            onChange={(e) => {
              const file = e.target.files?.[0];
              e.target.value = '';
              if (!file) return;
              void file.text().then((text) => {
                setUrl(text);
                if (!name.trim()) setName(file.name.replace(/\.[^.]+$/, '').slice(0, 40));
              });
            }}
          />
        </label>
        {error && <p className="form-error">{error}</p>}
        <div className="modal-actions">
          <Button type="button" variant="ghost" onClick={onClose}>
            {t('Отмена')}
          </Button>
          <Button type="submit" loading={loading} disabled={!url.trim()}>
            {t('Добавить')}
          </Button>
        </div>
      </form>
    </Modal>
  );
}
