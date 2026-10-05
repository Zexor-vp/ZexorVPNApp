import { useEffect, useState } from 'react';
import Button from '../components/Button';
import Modal from '../components/Modal';
import PageShell from '../components/PageShell';
import ProgressBar from '../components/ProgressBar';
import Toggle from '../components/Toggle';
import { TrashIcon } from '../components/Icons';
import RenewalSection from '../components/RenewalSection';
import TopUpModal from '../components/TopUpModal';
import { reportAuthLoss, useAsync } from '../hooks/useAsync';
import {
  CABINET_URL,
  deleteDevice,
  formatMoney,
  getDevicePrice,
  getDevices,
  getMe,
  getPurchaseOptions,
  getSubscription,
  purchaseDevices,
  setAutopay,
  type DevicePrice,
} from '../lib/cabinet';
import { errorMessage, openExternal, rawErrorMessage } from '../lib/commands';
import { currentLocale, useT } from '../i18n';

type Confirm =
  | { kind: 'devices'; count: number; price: DevicePrice }
  | { kind: 'delete-device'; hwid: string; name: string };

export default function SubscriptionPage() {
  const t = useT();
  const subscription = useAsync(() => getSubscription(), []);
  const me = useAsync(() => getMe(), []);
  const purchase = useAsync(() => getPurchaseOptions(), []);
  const [topUpOpen, setTopUpOpen] = useState(false);
  const devices = useAsync(() => getDevices(), []);

  const [extraDevices, setExtraDevices] = useState(1);
  const [devicePrice, setDevicePrice] = useState<DevicePrice | null>(null);
  const [confirm, setConfirm] = useState<Confirm | null>(null);
  const [working, setWorking] = useState(false);
  const [message, setMessage] = useState<{ tone: 'ok' | 'error'; text: string; topUp?: boolean } | null>(null);

  const currency = me.data?.currency ?? 'RUB';
  const sub = subscription.data?.subscription ?? null;

  useEffect(() => {
    let cancelled = false;
    setDevicePrice(null);
    getDevicePrice(extraDevices)
      .then((price) => !cancelled && setDevicePrice(price))
      .catch(() => !cancelled && setDevicePrice(null));
    return () => {
      cancelled = true;
    };
  }, [extraDevices]);

  // Запросили больше, чем можно докупить: сервер отвечает «можно максимум N» — возвращаем счётчик в допустимые границы.
  useEffect(() => {
    if (devicePrice?.available === false && devicePrice.can_add != null && extraDevices > devicePrice.can_add) {
      setExtraDevices(Math.max(1, devicePrice.can_add));
    }
  }, [devicePrice, extraDevices]);

  async function refreshAll() {
    await Promise.all([subscription.reload(), purchase.reload(), devices.reload(), me.reload()]);
  }

  async function runConfirmed() {
    if (!confirm) return;
    setWorking(true);
    setMessage(null);
    try {
      switch (confirm.kind) {
        case 'devices':
          await purchaseDevices(confirm.count);
          setExtraDevices(1);
          setMessage({ tone: 'ok', text: t('Устройства добавлены.') });
          break;
        case 'delete-device':
          await deleteDevice(confirm.hwid);
          setMessage({ tone: 'ok', text: t('Устройство удалено.') });
          break;
      }
      setConfirm(null);
      await refreshAll();
    } catch (err) {
      if (reportAuthLoss(err)) return;
      const text = errorMessage(err);
      setConfirm(null);
      setMessage({ tone: 'error', text, topUp: /средств|balance|funds/i.test(rawErrorMessage(err)) });
    } finally {
      setWorking(false);
    }
  }

  async function handleAutopay(enabled: boolean) {
    try {
      await setAutopay(enabled);
      await subscription.reload();
    } catch (err) {
      if (!reportAuthLoss(err)) setMessage({ tone: 'error', text: errorMessage(err) });
    }
  }

  const confirmText = (() => {
    if (!confirm) return '';
    switch (confirm.kind) {
      case 'devices':
        return t('Добавить устройств: {count} за {price}?', { count: confirm.count, price: confirm.price.total_price_label ?? '' });
      case 'delete-device':
        return t('Удалить устройство «{name}»? Оно освободит место в лимите.', { name: confirm.name });
    }
  })();

  return (
    <PageShell>
      <div className="page-title">
        <h1>{t('Подписка')}</h1>
        <p>{t('Тариф, продление и дополнительные опции')}</p>
      </div>

      {message && (
        <div className="card">
          <p className={message.tone === 'ok' ? 'form-ok' : 'form-error'}>{message.text}</p>
          {message.topUp && <Button onClick={() => setTopUpOpen(true)}>{t('Пополнить баланс')}</Button>}
        </div>
      )}

      <section className="card card-glow">
        {sub ? (
          <>
            <div className="row">
              <div>
                <span className="label">{t('Тариф')}</span>
                <div className="tile-value" style={{ marginTop: '0.3rem' }}>
                  {sub.tariff_name ?? t('Подписка')}
                </div>
              </div>
              <span className={`chip ${sub.is_expired ? 'chip-danger' : sub.is_grace_period ? 'chip-warn' : ''}`}>
                {sub.is_expired ? t('Истекла') : sub.is_grace_period ? t('Грейс') : sub.is_trial ? t('Пробный период') : t('Активна')}
              </span>
            </div>
            <p className="muted" style={{ margin: 0 }}>
              {t('Действует до {date} · осталось {left}', {
                date: new Date(sub.end_date).toLocaleDateString(currentLocale()),
                left: sub.days_left > 0 ? t('{n} дн.', { n: sub.days_left }) : sub.time_left_display || t('0 дн.'),
              })}
            </p>
            {sub.traffic_limit_gb > 0 && (
              <>
                <ProgressBar percent={sub.traffic_used_percent} />
                <p className="muted" style={{ margin: 0 }}>
                  {t('Трафик: {used} из {limit} ГБ', { used: sub.traffic_used_gb.toFixed(1), limit: sub.traffic_limit_gb })}
                </p>
              </>
            )}
            {!sub.is_daily && (
              <Toggle
                label={t('Автопродление')}
                hint={t('Спишем с баланса за несколько дней до окончания')}
                checked={sub.autopay_enabled}
                onChange={handleAutopay}
              />
            )}
          </>
        ) : subscription.loading ? (
          <div className="empty">
            <span className="spinner spinner-lg" aria-hidden />
          </div>
        ) : (
          <>
            <p className="empty">{subscription.error ?? t('У аккаунта нет активной подписки.')}</p>
            <Button onClick={() => void openExternal(`${CABINET_URL}/subscription`)}>{t('Оформить в кабинете')}</Button>
          </>
        )}
      </section>

      {me.data && (
        <section className="card">
          <div className="row">
            <div>
              <span className="label">{t('Баланс')}</span>
              <div className="tile-value" style={{ marginTop: '0.3rem' }}>
                {formatMoney(
                  currency === 'EUR' ? me.data.balance_eur_cents : currency === 'USD' ? me.data.balance_usd_cents : me.data.balance_kopeks,
                  currency,
                )}
              </div>
            </div>
            <Button variant="secondary" onClick={() => setTopUpOpen(true)}>
              {t('Пополнить')}
            </Button>
          </div>
        </section>
      )}

      {purchase.data && purchase.data.sales_mode === 'tariffs' && (
        <RenewalSection
          options={purchase.data}
          subscription={sub}
          onDone={(text) => {
            setMessage({ tone: 'ok', text });
            void refreshAll();
          }}
        />
      )}

      {sub && (
        <section className="card">
          <div className="row">
            <span className="label">{t('Устройства')}</span>
            <span className="muted">
              {devices.data
                ? t('{used} из {limit}', { used: devices.data.total, limit: devices.data.device_limit })
                : t('лимит {limit}', { limit: sub.device_limit })}
            </span>
          </div>
          {devices.data && devices.data.devices.length > 0 && (
            <div className="list">
              {devices.data.devices.map((device) => {
                const name = device.local_name || device.device_model || device.platform || device.hwid.slice(0, 8);
                return (
                  <div key={device.hwid} className="list-item">
                    <span>
                      {name}
                      {device.platform && device.local_name && <span className="muted"> · {device.platform}</span>}
                    </span>
                    <button
                      className="icon-btn"
                      aria-label={t('Удалить устройство {name}', { name })}
                      onClick={() => setConfirm({ kind: 'delete-device', hwid: device.hwid, name })}
                    >
                      <TrashIcon />
                    </button>
                  </div>
                );
              })}
            </div>
          )}
          {devicePrice?.available === false ? (
            <p className="muted" style={{ margin: 0 }}>
              {devicePrice.reason ?? t('Докупка устройств недоступна.')}
            </p>
          ) : (
            <div className="row">
              <div className="stepper">
                <button
                  aria-label={t('Меньше')}
                  disabled={extraDevices <= 1}
                  onClick={() => setExtraDevices((n) => Math.max(1, n - 1))}
                >
                  −
                </button>
                <span>{extraDevices}</span>
                <button
                  aria-label={t('Больше')}
                  disabled={extraDevices >= (devicePrice?.can_add ?? 10)}
                  onClick={() => setExtraDevices((n) => Math.min(devicePrice?.can_add ?? 10, n + 1))}
                >
                  +
                </button>
              </div>
              <Button
                variant="secondary"
                disabled={!devicePrice?.available}
                onClick={() => devicePrice && setConfirm({ kind: 'devices', count: extraDevices, price: devicePrice })}
              >
                {t('Докупить')}{devicePrice?.total_price_label ? ` · ${devicePrice.total_price_label}` : ''}
              </Button>
            </div>
          )}
          {devicePrice?.available !== false && devicePrice?.can_add != null && (
            <p className="muted" style={{ margin: 0 }}>
              {t('Можно докупить не больше {n}', { n: devicePrice.can_add })}
            </p>
          )}
        </section>
      )}

      {topUpOpen && (
        <TopUpModal
          currency={currency}
          onClose={() => setTopUpOpen(false)}
          onPaid={() => {
            setTopUpOpen(false);
            setMessage({ tone: 'ok', text: t('Баланс пополнен.') });
            void refreshAll();
          }}
        />
      )}

      {confirm && (
        <Modal title={t('Подтвердите')} onClose={() => !working && setConfirm(null)}>
          <p style={{ margin: 0 }}>{confirmText}</p>
          <div className="modal-actions">
            <Button variant="ghost" disabled={working} onClick={() => setConfirm(null)}>
              {t('Отмена')}
            </Button>
            <Button loading={working} onClick={() => void runConfirmed()}>
              {t('Подтвердить')}
            </Button>
          </div>
        </Modal>
      )}
    </PageShell>
  );
}
