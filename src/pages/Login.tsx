import { useRef, useState, type FormEvent } from 'react';
import Button from '../components/Button';
import PageShell from '../components/PageShell';
import {
  errorMessage,
  login,
  pollBrowserLogin,
  pollTelegramLogin,
  startBrowserLogin,
  startTelegramLogin,
  openExternal,
  type SessionInfo,
} from '../lib/commands';
import { CABINET_URL } from '../lib/cabinet';

const BOT_URL = 'https://t.me/Zexorvpnbot';

const POLL_INTERVAL_MS = 2000;
const LOGIN_TIMEOUT_MS = 5 * 60 * 1000;

type Pending = 'telegram' | 'google' | null;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

interface Props {
  onSuccess: (session: SessionInfo) => void;
}

export default function Login({ onSuccess }: Props) {
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [pending, setPending] = useState<Pending>(null);
  const cancelled = useRef(false);

  // Опрашивает сервер, пока пользователь подтверждает вход в боте/браузере.
  async function waitForLogin(poll: () => Promise<SessionInfo | null>) {
    const deadline = Date.now() + LOGIN_TIMEOUT_MS;
    while (!cancelled.current) {
      const session = await poll();
      if (session) return session;
      if (Date.now() > deadline) {
        throw new Error('Время ожидания входа истекло — попробуйте ещё раз.');
      }
      await sleep(POLL_INTERVAL_MS);
    }
    return null;
  }

  async function handleExternalLogin(kind: 'telegram' | 'google') {
    setError(null);
    cancelled.current = false;
    setPending(kind);
    try {
      let session: SessionInfo | null;
      if (kind === 'telegram') {
        const { token } = await startTelegramLogin();
        session = await waitForLogin(() => pollTelegramLogin(token));
      } else {
        const pairState = await startBrowserLogin('google');
        session = await waitForLogin(() => pollBrowserLogin(pairState));
      }
      if (session) onSuccess(session);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setPending(null);
    }
  }

  function cancelExternalLogin() {
    cancelled.current = true;
    setPending(null);
  }

  async function handleSubmit(e: FormEvent) {
    e.preventDefault();
    setError(null);
    setLoading(true);
    try {
      const session = await login(email.trim(), password);
      onSuccess(session);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }

  return (
    <PageShell>
      <div className="page-title">
        <h1>Вход</h1>
        <p>Войдите или зарегистрируйтесь, чтобы получить доступ к VPN Zexor</p>
      </div>

      <form className="card" onSubmit={handleSubmit}>
        {pending ? (
          <div className="login-waiting">
            <span className="spinner" aria-hidden />
            <p>
              {pending === 'telegram'
                ? 'Откройте бота в Telegram и нажмите «Start» — вход подтвердится автоматически.'
                : 'Завершите вход через Google в открывшемся браузере — приложение войдёт автоматически.'}
            </p>
            <Button type="button" variant="ghost" onClick={cancelExternalLogin}>
              Отмена
            </Button>
          </div>
        ) : (
          <>
            <div className="login-social">
              <Button type="button" variant="secondary" onClick={() => handleExternalLogin('telegram')}>
                Войти через Telegram
              </Button>
              <Button type="button" variant="secondary" onClick={() => handleExternalLogin('google')}>
                Войти через Google
              </Button>
            </div>
            <div className="login-divider">
              <span>или по email</span>
            </div>
          </>
        )}

        <label className="field">
          <span>Email</span>
          <input
            type="email"
            required
            autoFocus
            autoComplete="email"
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            placeholder="you@example.com"
          />
        </label>

        <label className="field">
          <span>Пароль</span>
          <input
            type="password"
            required
            autoComplete="current-password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            placeholder="••••••••"
          />
        </label>

        {error && <p className="form-error">{error}</p>}

        <Button type="submit" loading={loading} disabled={pending !== null} className="login-submit">
          Войти
        </Button>
      </form>

      <section className="card">
        <span className="label">Нет аккаунта?</span>
        <p className="muted" style={{ margin: 0 }}>
          Аккаунт создаётся за минуту — в Telegram-боте или на сайте. После регистрации вернитесь сюда и войдите.
        </p>
        <div className="login-social">
          <Button type="button" variant="secondary" onClick={() => void openExternal(BOT_URL)}>
            Зарегистрироваться в Telegram
          </Button>
          <Button type="button" variant="secondary" onClick={() => void openExternal(CABINET_URL)}>
            Зарегистрироваться на сайте
          </Button>
        </div>
      </section>
    </PageShell>
  );
}
