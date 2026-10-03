import { useEffect, useState, type FormEvent } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import Button from '../components/Button';
import PageShell from '../components/PageShell';
import Segmented from '../components/Segmented';
import { TrashIcon } from '../components/Icons';
import {
  addRoutingApp,
  appSettings,
  errorMessage,
  listRunningApps,
  removeRoutingApp,
  setRoutingMode,
  type AppSettings,
  type RoutingMode,
} from '../lib/commands';
import { useT } from '../i18n';

interface Props {
  onBack: () => void;
}

const MODE_HINT: Record<RoutingMode, string> = {
  exclude: 'Весь трафик идёт через VPN. Приложения из списка ходят напрямую, минуя VPN.', // i18n-key
  only: 'Через VPN идут только приложения из списка. Всё остальное ходит напрямую.', // i18n-key
};

export default function RoutingPage({ onBack }: Props) {
  const t = useT();
  const [state, setState] = useState<AppSettings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [value, setValue] = useState('');
  const [adding, setAdding] = useState(false);
  const [running, setRunning] = useState<string[] | null>(null);
  const [loadingRunning, setLoadingRunning] = useState(false);

  useEffect(() => {
    appSettings()
      .then(setState)
      .catch((err) => setError(errorMessage(err)));
  }, []);

  // Любое изменение сразу применяется (при активном подключении Rust переподключит туннель).
  async function apply(action: () => Promise<AppSettings>) {
    setError(null);
    try {
      setState(await action());
    } catch (err) {
      setError(errorMessage(err));
      throw err;
    }
  }

  async function add(name: string) {
    if (!name.trim()) return;
    setAdding(true);
    try {
      await apply(() => addRoutingApp(name));
      setValue('');
    } catch {
      // ошибка уже показана
    } finally {
      setAdding(false);
    }
  }

  function submit(event: FormEvent) {
    event.preventDefault();
    void add(value);
  }

  async function browse() {
    try {
      const picked = await open({
        multiple: false,
        directory: false,
        filters: [{ name: t('Программы'), extensions: ['exe'] }],
      });
      if (typeof picked === 'string') await add(picked);
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  async function showRunning() {
    setLoadingRunning(true);
    try {
      setRunning(await listRunningApps());
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoadingRunning(false);
    }
  }

  const apps = state?.routing_apps ?? [];
  const mode = state?.routing_mode ?? 'exclude';
  const suggestions = (running ?? []).filter((name) => !apps.some((a) => a.toLowerCase() === name.toLowerCase()));

  return (
    <PageShell
      actions={
        <button className="link-btn back-btn" onClick={onBack}>
          {t('← Назад')}
        </button>
      }
    >
      <div className="page-title">
        <h1>{t('Маршрутизация')}</h1>
        <p>{t('Какие приложения идут через VPN, а какие — напрямую')}</p>
      </div>

      {error && (
        <div className="card">
          <p className="form-error">{error}</p>
        </div>
      )}

      <section className="card card-glow">
        <Segmented<RoutingMode>
          value={mode}
          disabled={!state}
          options={[
            { value: 'exclude', label: t('Кроме выбранных') },
            { value: 'only', label: t('Только выбранные') },
          ]}
          onChange={(next) => void apply(() => setRoutingMode(next)).catch(() => undefined)}
        />
        <p className="muted" style={{ margin: 0 }}>
          {t(MODE_HINT[mode])}
        </p>
        {state && state.tunnel_mode === 'proxy' && (
          <p className="muted" style={{ margin: 0 }}>
            {t('В режиме Proxy правила действуют на программы, которые используют системный прокси (браузеры и большинство приложений). Для игр, лаунчеров и остального включите режим TUN на главном экране.')}
          </p>
        )}
      </section>

      <section className="card">
        <span className="label">{mode === 'exclude' ? t('Напрямую, без VPN') : t('Только через VPN')}</span>
        <form className="row" onSubmit={submit} style={{ gap: '0.5rem' }}>
          <input
            className="text-input"
            placeholder="chrome.exe"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            aria-label={t('Имя приложения')}
          />
          <Button type="submit" variant="secondary" loading={adding} disabled={!value.trim()}>
            {t('Добавить')}
          </Button>
        </form>
        <div className="row" style={{ gap: '0.5rem' }}>
          <Button variant="ghost" onClick={() => void browse()}>
            {t('Выбрать файл…')}
          </Button>
          <Button variant="ghost" loading={loadingRunning} onClick={() => void showRunning()}>
            {t('Запущенные программы')}
          </Button>
        </div>

        {running && (
          <div className="chip-cloud" aria-label={t('Запущенные программы')}>
            {suggestions.length === 0 && <span className="muted">{t('Нет новых программ для добавления')}</span>}
            {suggestions.map((name) => (
              <button key={name} type="button" className="chip chip-button" onClick={() => void add(name)}>
                + {name}
              </button>
            ))}
          </div>
        )}

        {apps.length > 0 ? (
          <div className="list">
            {apps.map((app) => (
              <div key={app} className="list-item">
                <span>{app}</span>
                <button
                  className="icon-btn"
                  aria-label={t('Убрать {name}', { name: app })}
                  onClick={() => void apply(() => removeRoutingApp(app)).catch(() => undefined)}
                >
                  <TrashIcon />
                </button>
              </div>
            ))}
          </div>
        ) : (
          <p className="muted" style={{ margin: 0 }}>
            {mode === 'only'
              ? t('Список пуст — пока через VPN идёт всё. Добавьте приложения, чтобы ограничить VPN только ими.')
              : t('Список пуст — все приложения идут через VPN.')}
          </p>
        )}
      </section>

      <p className="muted" style={{ margin: '0 0.25rem' }}>
        {t('Если VPN подключён, при изменении списка он переподключится на пару секунд, чтобы правила вступили в силу.')}
      </p>
    </PageShell>
  );
}
