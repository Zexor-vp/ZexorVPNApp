import { useRef, useState, type FormEvent } from 'react';
import Button from '../components/Button';
import LanguageCard from '../components/LanguageCard';
import PageShell from '../components/PageShell';
import QrCode from '../components/QrCode';
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
import { useT } from '../i18n';

const BOT_URL = 'https://t.me/Zexorvpnbot';

const POLL_INTERVAL_MS = 2000;
const LOGIN_TIMEOUT_MS = 5 * 60 * 1000;

// telegram — бот открывается в браузере этого ПК; telegram-qr — только QR-код для телефона (на ПК без VPN Telegram не открывается);
// cabinet — вход через страницу личного кабинета (браузер + тот же адрес QR-кодом для другого устройства).
type Pending = 'telegram' | 'telegram-qr' | 'cabinet' | 'google' | null;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

interface Props {
  onSuccess: (session: SessionInfo) => void;
}

export default function Login({ onSuccess }: Props) {
  const t = useT();
  const [email, setEmail] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [pending, setPending] = useState<Pending>(null);
  const [qrUrl, setQrUrl] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const cancelled = useRef(false);

  // Опрашивает сервер, пока пользователь подтверждает вход в боте/браузере.
  async function waitForLogin(poll: () => Promise<SessionInfo | null>) {
    const deadline = Date.now() + LOGIN_TIMEOUT_MS;
    while (!cancelled.current) {
      const session = await poll();
      if (session) return session;
      if (Date.now() > deadline) {
        throw new Error(t('Время ожидания входа истекло — попробуйте ещё раз.'));
      }
      await sleep(POLL_INTERVAL_MS);
    }
    return null;
  }

  async function handleExternalLogin(kind: Exclude<Pending, null>) {
    setError(null);
    setQrUrl(null);
    setCopied(false);
    cancelled.current = false;
    setPending(kind);
    try {
      let session: SessionInfo | null;
      if (kind === 'telegram' || kind === 'telegram-qr') {
        const { token, url } = await startTelegramLogin(kind === 'telegram');
        setQrUrl(url);
        session = await waitForLogin(() => pollTelegramLogin(token));
      } else if (kind === 'cabinet') {
        const pairState = await startBrowserLogin(undefined, true);
        setQrUrl(`${CABINET_URL}/adblock/connect?state=${pairState}&source=desktop`);
        session = await waitForLogin(() => pollBrowserLogin(pairState));
      } else {
        const pairState = await startBrowserLogin('google');
        session = await waitForLogin(() => pollBrowserLogin(pairState));
      }
      if (session) onSuccess(session);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setPending(null);
      setQrUrl(null);
    }
  }

  function cancelExternalLogin() {
    cancelled.current = true;
    setPending(null);
    setQrUrl(null);
  }

  async function copyLink() {
    if (!qrUrl) return;
    try {
      await navigator.clipboard.writeText(qrUrl);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2500);
    } catch {
      // буфер обмена недоступен — ссылку всё равно можно отсканировать с экрана
    }
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
        <h1>{t('Вход')}</h1>
        <p>{t('Войдите или зарегистрируйтесь, чтобы получить доступ к VPN Zexor')}</p>
      </div>

      <form className="card" onSubmit={handleSubmit}>
        {pending ? (
          <div className="login-waiting">
            <span className="spinner" aria-hidden />
            <p>
              {pending === 'telegram'
                ? t('Откройте бота в Telegram и нажмите «Start» — вход подтвердится автоматически.')
                : pending === 'telegram-qr'
                  ? t('Отсканируйте QR-код телефоном, где работает Telegram, и нажмите «Start» в боте — вход подтвердится здесь автоматически.')
                  : pending === 'cabinet'
                    ? t('Войдите в личный кабинет в открывшемся браузере или отсканируйте QR-код телефоном — приложение войдёт автоматически.')
                    : t('Завершите вход через Google в открывшемся браузере — приложение войдёт автоматически.')}
            </p>
            {qrUrl && (
              <div className="login-qr">
                <QrCode value={qrUrl} size={188} />
                <Button type="button" variant="secondary" onClick={() => void copyLink()}>
                  {copied ? t('Ссылка скопирована') : t('Скопировать ссылку')}
                </Button>
              </div>
            )}
            <Button type="button" variant="ghost" onClick={cancelExternalLogin}>
              {t('Отмена')}
            </Button>
          </div>
        ) : (
          <>
            <div className="login-social">
              <Button type="button" variant="secondary" onClick={() => handleExternalLogin('telegram')}>
                {t('Войти через Telegram')}
              </Button>
              <Button type="button" variant="secondary" onClick={() => handleExternalLogin('telegram-qr')}>
                {t('Войти по QR-коду')}
              </Button>
              <Button type="button" variant="secondary" onClick={() => handleExternalLogin('cabinet')}>
                {t('Войти через личный кабинет')}
              </Button>
              <Button type="button" variant="secondary" onClick={() => handleExternalLogin('google')}>
                {t('Войти через Google')}
              </Button>
            </div>
            <p className="muted login-hint">{t('Telegram не открывается без VPN? Войдите по QR-коду: отсканируйте его телефоном — на нём Telegram работает.')}</p>
            <div className="login-divider">
              <span>{t('или по email')}</span>
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
          <span>{t('Пароль')}</span>
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
          {t('Войти')}
        </Button>
      </form>

      <section className="card">
        <span className="label">{t('Нет аккаунта?')}</span>
        <p className="muted" style={{ margin: 0 }}>
          {t('Аккаунт создаётся за минуту — в Telegram-боте или на сайте. После регистрации вернитесь сюда и войдите.')}
        </p>
        <div className="login-social">
          <Button type="button" variant="secondary" onClick={() => void openExternal(BOT_URL)}>
            {t('Зарегистрироваться в Telegram')}
          </Button>
          <Button type="button" variant="secondary" onClick={() => void openExternal(CABINET_URL)}>
            {t('Зарегистрироваться на сайте')}
          </Button>
        </div>
      </section>

      <LanguageCard />
    </PageShell>
  );
}
