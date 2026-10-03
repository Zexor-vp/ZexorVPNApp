import { useEffect, useState, type FormEvent } from 'react';
import { getVersion } from '@tauri-apps/api/app';
import { check as checkForUpdate, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import Button from '../components/Button';
import LanguageCard from '../components/LanguageCard';
import PageShell from '../components/PageShell';
import Toggle from '../components/Toggle';
import { isMobile } from '../lib/platform';
import { reportAuthLoss, useAsync } from '../hooks/useAsync';
import {
  formatMoney,
  getMe,
  getReferral,
  requestEmailChange,
  verifyEmailChange,
} from '../lib/cabinet';
import { appSettings, checkApkUpdate, errorMessage, logout, openExternal, setTelemetry } from '../lib/commands';
import { useT } from '../i18n';

type UpdateState =
  | { kind: 'idle' }
  | { kind: 'checking' }
  | { kind: 'up-to-date' }
  | { kind: 'available'; update: Update }
  | { kind: 'apk'; version: string; url: string }
  | { kind: 'installing' }
  | { kind: 'error'; message: string };

interface Props {
  onLoggedOut: () => void;
}

export default function ProfilePage({ onLoggedOut }: Props) {
  const t = useT();
  const me = useAsync(() => getMe(), []);
  const referral = useAsync(() => getReferral(), []);
  const [telemetry, setTelemetryState] = useState<boolean | null>(null);

  useEffect(() => {
    appSettings()
      .then((current) => setTelemetryState(current.telemetry))
      .catch(() => undefined);
  }, []);

  const [notice, setNotice] = useState<{ tone: 'ok' | 'error'; text: string } | null>(null);
  const [copied, setCopied] = useState(false);
  const [version, setVersion] = useState('');
  const [updateState, setUpdateState] = useState<UpdateState>({ kind: 'idle' });

  useEffect(() => {
    void getVersion()
      .then(setVersion)
      .catch(() => undefined);
  }, []);

  async function handleCopyCode(code: string) {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1800);
    } catch {
      setNotice({ tone: 'error', text: t('Не удалось скопировать код.') });
    }
  }

  async function handleCheckUpdate() {
    setUpdateState({ kind: 'checking' });
    try {
      if (isMobile) {
        const apk = await checkApkUpdate();
        setUpdateState(apk ? { kind: 'apk', ...apk } : { kind: 'up-to-date' });
        return;
      }
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
  const displayName = user?.first_name || user?.username || user?.email || t('Профиль');

  return (
    <PageShell>
      <div className="page-title">
        <h1>{t('Профиль')}</h1>
        <p>{t('Аккаунт, баланс, рефералы и настройки приложения')}</p>
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
        <span className="label">{t('Реферальная программа')}</span>
        {referral.data ? (
          <>
            <div className="copy-box">
              <span>{referral.data.referral_code}</span>
              <Button variant="secondary" onClick={() => void handleCopyCode(referral.data!.referral_code)}>
                {copied ? t('Скопировано') : t('Копировать')}
              </Button>
            </div>
            <div className="tile-grid">
              <div className="tile">
                <span className="label">{t('Рефералов')}</span>
                <span className="tile-value">
                  {referral.data.total_referrals}
                  <span className="muted"> · {t('активных {n}', { n: referral.data.active_referrals })}</span>
                </span>
              </div>
              <div className="tile">
                <span className="label">{t('Заработано')}</span>
                <span className="tile-value">
                  {formatMoney(referral.data.total_earnings_kopeks, user?.currency ?? 'RUB')}
                </span>
              </div>
            </div>
            <span className="muted">{t('Друг указывает ваш код при регистрации — вы получаете {percent}% с каждого его пополнения.', { percent: referral.data.commission_percent })}</span>
          </>
        ) : referral.loading ? (
          <div className="empty">
            <span className="spinner" aria-hidden />
          </div>
        ) : (
          <p className="muted">{referral.error ?? t('Реферальная программа недоступна.')}</p>
        )}
      </section>

      <section className="card">
        <span className="label">{t('Приложение')}</span>
        <div className="row">
          <span>
            {t('Обновления')}
            {version ? ` · v${version}` : ''}
          </span>
          <Button
            variant="secondary"
            loading={updateState.kind === 'checking' || updateState.kind === 'installing'}
            onClick={handleCheckUpdate}
          >
            {t('Проверить')}
          </Button>
        </div>
        {updateState.kind === 'up-to-date' && <p className="muted">{t('Установлена последняя версия.')}</p>}
        {updateState.kind === 'apk' && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: '0.6rem' }}>
            <p style={{ margin: 0 }}>{t('Доступна версия {version}.', { version: updateState.version })}</p>
            <Button onClick={() => void openExternal(updateState.url).catch((err) => setUpdateState({ kind: 'error', message: errorMessage(err) }))}>
              {t('Скачать обновление')}
            </Button>
          </div>
        )}
        {updateState.kind === 'error' && <p className="form-error">{updateState.message}</p>}
        {updateState.kind === 'available' && (
          <div style={{ display: 'flex', flexDirection: 'column', gap: '0.6rem' }}>
            <p style={{ margin: 0 }}>{t('Доступна версия {version}.', { version: updateState.update.version })}</p>
            <Button onClick={() => void handleInstallUpdate(updateState.update)}>{t('Установить и перезапустить')}</Button>
          </div>
        )}
        <Toggle
          label={t('Статистика доступности серверов')}
          hint={t('Раз в полчаса приложение отправляет время отклика серверов Zexor и название вашей сети (оператора) — без IP-адреса и без привязки к аккаунту. Это помогает быстрее находить блокировки. Если приложение даёт сбой, оно также отправляет технический отчёт: версию, модель устройства и текст ошибки — тоже без IP-адреса, почты и привязки к аккаунту.')}
          checked={telemetry ?? true}
          disabled={telemetry === null}
          onChange={(enabled) => {
            setTelemetryState(enabled);
            void setTelemetry(enabled).catch(() => setTelemetryState(!enabled));
          }}
        />
        {!isMobile && (
        <p className="muted" style={{ margin: 0 }}>
          {t('Закрытие окна не отключает VPN — приложение остаётся в трее. Выйти полностью можно из меню значка в трее.')}
        </p>
        )}
      </section>

      <section className="card">
        <Button variant="ghost" onClick={() => void handleLogout()}>
          {t('Выйти из аккаунта')}
        </Button>
      </section>

      <LanguageCard />
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
  const t = useT();
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
        <div style={{ minWidth: 0 }}>
          <span className="label">{t('Почта')}</span>
          <div className="tile-value" style={{ marginTop: '0.3rem', fontSize: '1rem', overflowWrap: 'anywhere' }}>
            {email ?? t('не указана')}
          </div>
          {email && !verified && <span className="chip chip-warn">{t('не подтверждена')}</span>}
        </div>
        {step === 'view' && (
          <Button variant="secondary" style={{ flexShrink: 0 }} onClick={() => setStep('enter')}>
            {email ? t('Изменить') : t('Добавить')}
          </Button>
        )}
      </div>
      {step !== 'view' && (
        <form onSubmit={submit} style={{ display: 'flex', flexDirection: 'column', gap: '0.6rem' }}>
          {step === 'enter' ? (
            <input
              className="text-input"
              type="email"
              placeholder={t('Новый адрес почты')}
              value={value}
              autoFocus
              onChange={(e) => setValue(e.target.value)}
            />
          ) : (
            <>
              <span className="muted">{t('Мы отправили 6-значный код на {email}.', { email: value })}</span>
              <input
                className="text-input"
                inputMode="numeric"
                maxLength={6}
                placeholder={t('Код из письма')}
                value={code}
                autoFocus
                onChange={(e) => setCode(e.target.value.replace(/\D/g, ''))}
              />
            </>
          )}
          {error && <p className="form-error">{error}</p>}
          <div className="modal-actions">
            <Button type="button" variant="ghost" onClick={() => setStep('view')}>
              {t('Отмена')}
            </Button>
            <Button type="submit" loading={loading} disabled={step === 'enter' ? !value.includes('@') : code.length !== 6}>
              {step === 'enter' ? t('Отправить код') : t('Подтвердить')}
            </Button>
          </div>
        </form>
      )}
    </section>
  );
}
