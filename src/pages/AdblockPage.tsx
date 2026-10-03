import { useEffect, useState, type FormEvent } from 'react';
import Button from '../components/Button';
import PageShell from '../components/PageShell';
import Toggle from '../components/Toggle';
import { TrashIcon } from '../components/Icons';
import {
  addAdblockDomain,
  adblockSessionStats,
  adblockSettings,
  errorMessage,
  removeAdblockDomain,
  resetAdblockSessionStats,
  setAdblockEnabled,
  type AdblockListKind,
  type AdblockState,
  type SessionStats,
} from '../lib/commands';
import { useT } from '../i18n';

const STATS_POLL_MS = 2000;

export default function AdblockPage() {
  const t = useT();
  const [state, setState] = useState<AdblockState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [stats, setStats] = useState<SessionStats>({ blocked: 0, active: false });

  useEffect(() => {
    adblockSettings()
      .then(setState)
      .catch((err) => setError(errorMessage(err)));
  }, []);

  // Счётчик заблокированной рекламы: опрашиваем, пока открыта страница.
  useEffect(() => {
    let cancelled = false;
    const tick = () =>
      adblockSessionStats()
        .then((next) => !cancelled && setStats(next))
        .catch(() => undefined);
    void tick();
    const timer = window.setInterval(tick, STATS_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, []);

  // Любое изменение сразу применяется (при активном подключении Rust переподключит туннель).
  async function apply(action: () => Promise<AdblockState>) {
    setError(null);
    try {
      setState(await action());
    } catch (err) {
      setError(errorMessage(err));
      throw err;
    }
  }

  return (
    <PageShell>
      <div className="page-title">
        <h1>MyBlock</h1>
        <p>{t('Блокировщик рекламы: работает локально и не зависит от сервера')}</p>
      </div>

      {error && (
        <div className="card">
          <p className="form-error">{error}</p>
        </div>
      )}

      <section className="card card-glow">
        <Toggle
          label={t('Блокировать рекламу')}
          hint={t('Рекламные домены отсекаются прямо в приложении, трафик до них не доходит')}
          checked={state?.enabled ?? true}
          disabled={!state}
          onChange={(enabled) => void apply(() => setAdblockEnabled(enabled)).catch(() => undefined)}
        />
        <div className="stat-box">
          <span className="label">{t('Заблокировано за сессию')}</span>
          <span className="big-number">{stats.blocked.toLocaleString('ru-RU')}</span>
          <div className="row">
            <span className="muted">
              {stats.active ? t('Считаем рекламные запросы, пока VPN подключён') : t('Подключитесь к VPN — тогда начнём считать')}
            </span>
            <button className="link-btn" onClick={() => void resetAdblockSessionStats().then(setStats)}>
              {t('Сбросить')}
            </button>
          </div>
        </div>
      </section>

      <DomainList
        title={t('Всегда блокировать')}
        hint={t('Добавьте домен, который не попал в списки, например ads.example.com')}
        kind="block"
        items={state?.custom_block ?? []}
        onAdd={(domain) => apply(() => addAdblockDomain('block', domain))}
        onRemove={(domain) => apply(() => removeAdblockDomain('block', domain)).catch(() => undefined)}
      />

      <DomainList
        title={t('Никогда не блокировать')}
        hint={t('Если сайт сломался из-за блокировщика, добавьте его сюда — он будет работать как обычно')}
        kind="allow"
        items={state?.custom_allow ?? []}
        onAdd={(domain) => apply(() => addAdblockDomain('allow', domain))}
        onRemove={(domain) => apply(() => removeAdblockDomain('allow', domain)).catch(() => undefined)}
      />

      <p className="muted" style={{ margin: '0 0.25rem' }}>
        {t('Если VPN подключён, при изменении настроек он переподключится на пару секунд, чтобы правила вступили в силу.')}
      </p>
    </PageShell>
  );
}

interface ListProps {
  title: string;
  hint: string;
  kind: AdblockListKind;
  items: string[];
  onAdd: (domain: string) => Promise<void>;
  onRemove: (domain: string) => void;
}

function DomainList({ title, hint, kind, items, onAdd, onRemove }: ListProps) {
  const t = useT();
  const [value, setValue] = useState('');
  const [adding, setAdding] = useState(false);
  const [localError, setLocalError] = useState<string | null>(null);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!value.trim()) return;
    setAdding(true);
    setLocalError(null);
    try {
      await onAdd(value);
      setValue('');
    } catch (err) {
      setLocalError(errorMessage(err));
    } finally {
      setAdding(false);
    }
  }

  return (
    <section className="card" data-kind={kind}>
      <span className="label">{title}</span>
      <p className="muted" style={{ margin: 0 }}>
        {hint}
      </p>
      <form className="row" onSubmit={submit} style={{ gap: '0.5rem' }}>
        <input
          className="text-input"
          placeholder="example.com"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          aria-label={title}
        />
        <Button type="submit" variant="secondary" loading={adding} disabled={!value.trim()}>
          {t('Добавить')}
        </Button>
      </form>
      {localError && <p className="form-error">{localError}</p>}
      {items.length > 0 && (
        <div className="list">
          {items.map((domain) => (
            <div key={domain} className="list-item">
              <span>{domain}</span>
              <button className="icon-btn" aria-label={t('Убрать {name}', { name: domain })} onClick={() => onRemove(domain)}>
                <TrashIcon />
              </button>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
