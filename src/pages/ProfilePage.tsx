import { useEffect, useState, type FormEvent } from 'react';
import { getVersion } from '@tauri-apps/api/app';
import { check as checkForUpdate, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import Button from '../components/Button';
import PageShell from '../components/PageShell';
import Segmented from '../components/Segmented';
import { reportAuthLoss, useAsync } from '../hooks/useAsync';
import {
  formatMoney,
  getMe,
  getReferral,
  requestEmailChange,
  setCurrency,
  verifyEmailChange,
} from '../lib/cabinet';
import { errorMessage, logout } from '../lib/commands';

type UpdateState =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'up-to-date' }
  | { kind: 'available'; update: Update }
  | { kind: 'installing' }
  | { kind: 'error'; message: string };

interface Props {
  onLoggedOut: () => void;
}

export default function ProfilePage({ onLoggedOut }: Props) {
  const me = useAsync(() => getMe(), []);
  const referral = useAsync(() => getReferral(), []);

  const [notice, setNotice] = useState<{ tone: 'ok' | 'error'; text: string } | null>(null);
  const [copied, setCopied] = useState(false);
  const [version, setVersion] = useState('');
  const [updateState, setUpdateState] = useState<UpdateState>({ kind: 'idle' });

  useEffect(() => {
    void getVersion()
      .then(setVersion)
      .catch(() => undefined);
  }, []);

  async function handleCurrency(next: string) {
    setNotice(null);
    try {
      await setCurrency(next);
      await me.reload();
      setNotice({ tone: 'ok', text: 'Валюта изменена.' });
    } catch (err) {
      if (!reportAuthLoss(err)) setNotice({ tone: 'error', text: errorMessage(err) });
    }
  }

  async function handleCopyCode(code: string) {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch {
      setNotice({ tone: 'error', text: 'Не удалось скопировать код.' });
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

  async function handleLogout() {
    try {
      await logout();
    } finally {
      onLoggedOut();
    }
  }

  const user = me.data;
  const displayName = user?.first_name || user?.username || user?.email || 'Профиль';

  return (
    <PageShell>
      <div className="page-title">
        <h1>Профиль</h1>
        <p>Аккаунт, баланс, рефералы и настройки приложения</p>
      </div>

      {notice && (
        <div className="card">
          <p className={notice.tone === 'ok' ? 'form-ok' : 'form-error'}>{notice.text}</p>
        </div>
      )}

      <section className="card card-glow">
        {user ? (
          <>
            <div className="row" style={{ justifyContent: 'flex-start', gap: '0.9rem' }}>
              <span className="avatar">{displayName.slice(0, 1).toUpperCase()}</span>
              <div>
                <div className="tile-value">{displayName}</div>
                <span className="muted">ID {user.id}</span>
              </div>
            </div>
            <div style={{ display: 'flex', flexDirection: 'column', gap: '0.5rem' }}>
              <span className="label">Валюта</span>
              <Segmented
                value={user.currency}
                onChange={handleCurrency}
                options={[
                  { value: 'RUB', label: '₽ RUB' },
                  { value: 'EUR', label: '€ EUR' },
                  { value: 'USD', label: '$ USD' },
                ]}
              />
              <span className="muted">Сменить валюту можно, пока на балансе в текущей нет средств.</span>
            </div>
          </>
        ) : me.loading ? (
          <div className="empty">
            <span className="spinner spinner-lg" aria-hidden />
          </div>
        ) : (
          <p className="form-error">{me.error}</p>
        )}
      </section>

      {user && <EmailCard email={user.email} verified={user.email_verified} onChanged={() => void me.reload()} />}

      <section className="card">
        <span className="label">Реферальная программа</span>
        {referral.data ? (
          <>
            <div className="copy-box">
              <span>{referral.data.referral_code}</span>
              <Button variant="secondary" onClick={() => void handleCopyCode(referral.data!.referral_code)}>
                {copied ? 'Скопировано' : 'Копировать'}
              </Button>
            </div>
            <div className="tile-grid">
              <div className="tile">
                <span className="label">Рефералов</span>
                <span className="tile-value">
                  {referral.data.total_referrals}
                  <span className="muted"> · активных {referral.data.active_referrals}</span>
                </span>
              </div>
              <div className="tile">
                <span className="label">Заработано</span>
                <span className="tile-value">
                  {formatMoney(referral.data.total_earnings_kopeks, user?.currency ?? 'RUB')}
                </span>
                <span className="muted">комиссия {referral.data.commission_percent}%</span>
              </div>
            </div>
            <span className="muted">Друг указывает ваш код при регистрации — вы получаете процент с его пополнений.</span>
          </>
        ) : referral.loading ? (
          <div className="empty">
            <span className="spinner" aria-hidden />
          </div>
        ) : (
          <p className="muted">{referral.error ?? 'Реферальная программа недоступна.'}</p>
        )}
      </section>

      <section className="card">
        <span className="label">Приложение</span>
        <div className="row">
          <span>Обновления{version ? ` · v${version}` : ''}</span>
          <Button
            variant="secondary"
            loading={updateState.kind === 'checking' || updateState.kind === 'installing'}
            onClick={handleCheckUpdate}
          >
            Проверить
          </Button>
        </div>
        {updateState.kind === 'up-to-date' && <p className="muted">Установлена последняя версия.</p>}
        {updateState.kind === 'error' && <p className="form-error">{updateState.message}</p>}
        {updateState.kind === 'available' && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: '0.6rem' }}>
            <p style={{ margin: 0 }}>Доступна версия {updateState.update.version}.</p>
            <Button onClick={() => void handleInstallUpdate(updateState.update)}>Установить и перезапустить</Button>
          </div>
        )}
        <p className="muted" style={{ margin: 0 }}>
          Закрытие окна не отключает VPN — приложение остаётся в трее. Выйти полностью можно из меню значка в трее.
        </p>
      </section>

      <section className="card">
        <Button variant="ghost" onClick={() => void handleLogout()}>
          Выйти из аккаунта
        </Button>
      </section>
    </PageShell>
  );
}

function EmailCard({
  email,
  verified,
  onChanged,
}: {
  email: string | null;
  verified: boolean;
  onChanged: () => void;
}) {
  const [step, setStep] = useState<'view' | 'enter' | 'code'>('view');
  const [value, setValue] = useState('');
  const [code, setCode] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  async function submit(event: FormEvent) {
    event.preventDefault();
    setError(null);
    setLoading(true);
    try {
      if (step === 'enter') {
        await requestEmailChange(value.trim());
        setStep('code');
      } else {
        await verifyEmailChange(code.trim());
        setStep('view');
        setValue('');
        setCode('');
        onChanged();
      }
    } catch (err) {
      if (!reportAuthLoss(err)) setError(errorMessage(err));
    } finally {
      setLoading(false);
    }
  }

  return (
    <section className="card">
      <div className="row">
        <div>
          <span className="label">Почта</span>
          <div className="tile-value" style={{ marginTop: '0.3rem', fontSize: '1rem' }}>
            {email ?? 'не указана'}
          </div>
          {email && !verified && <span className="chip chip-warn">не подтверждена</span>}
        </div>
        {step === 'view' && (
          <Button variant="secondary" onClick={() => setStep('enter')}>
            {email ? 'Изменить' : 'Добавить'}
          </Button>
        )}
      </div>
      {step !== 'view' && (
        <form onSubmit={submit} style={{ display: 'flex', flexDirection: 'column', gap: '0.6rem' }}>
          {step === 'enter' ? (
            <input
              className="text-input"
              type="email"
              placeholder="Новый адрес почты"
              value={value}
              autoFocus
              onChange={(e) => setValue(e.target.value)}
            />
          ) : (
            <>
              <span className="muted">Мы отправили 6-значный код на {value}.</span>
              <input
                className="text-input"
                inputMode="numeric"
                maxLength={6}
                placeholder="Код из письма"
                value={code}
                autoFocus
                onChange={(e) => setCode(e.target.value.replace(/\D/g, ''))}
              />
            </>
          )}
          {error && <p className="form-error">{error}</p>}
          <div className="modal-actions">
            <Button type="button" variant="ghost" onClick={() => setStep('view')}>
              Отмена
            </Button>
            <Button type="submit" loading={loading} disabled={step === 'enter' ? !value.includes('@') : code.length !== 6}>
              {step === 'enter' ? 'Отправить код' : 'Подтвердить'}
            </Button>
          </div>
        </form>
      )}
    </section>
  );
}
