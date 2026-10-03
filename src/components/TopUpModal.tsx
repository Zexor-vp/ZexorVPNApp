import { useEffect, useRef, useState, type FormEvent } from 'react';
import Button from './Button';
import Modal from './Modal';
import {
  balanceFor,
  createTopUp,
  formatMoney,
  getMe,
  getPaymentMethods,
  type PaymentMethod,
} from '../lib/cabinet';
import { errorMessage, openPaymentUrl } from '../lib/commands';
import { reportAuthLoss } from '../hooks/useAsync';
import { useT } from '../i18n';

const BALANCE_POLL_MS = 5000;
const BALANCE_POLL_MAX_MS = 10 * 60 * 1000;

interface Props {
  currency: string;
  /** Сколько не хватает (копейки/центы): подставится в сумму. */
  neededMinor?: number;
  onClose: () => void;
  /** Баланс вырос — оплата прошла. */
  onPaid: () => void;
}

/** Пополнение: выбрали способ и сумму — страница оплаты сразу открывается в браузере. */
export default function TopUpModal({ currency, neededMinor, onClose, onPaid }: Props) {
  const t = useT();
  const [methods, setMethods] = useState<PaymentMethod[] | null>(null);
  const [methodId, setMethodId] = useState('');
  const [optionId, setOptionId] = useState('');
  const [amount, setAmount] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [waiting, setWaiting] = useState(false);
  const startBalance = useRef<number | null>(null);

  const method = methods?.find((m) => m.id === methodId) ?? null;

  useEffect(() => {
    getPaymentMethods()
      .then((list) => {
        const available = list.filter((m) => m.is_available);
        setMethods(available);
        const first = available[0];
        if (first) {
          setMethodId(first.id);
          const minMajor = Math.ceil(first.min_amount_kopeks / 100);
          const quickMajor = first.quick_amounts[0] ? first.quick_amounts[0] / 100 : minMajor;
          const wanted = neededMinor ? Math.ceil(neededMinor / 100) : quickMajor;
          setAmount(String(Math.max(minMajor, Math.round(wanted))));
        }
      })
      .catch((err) => {
        if (!reportAuthLoss(err)) setError(errorMessage(err));
      });
  }, [neededMinor]);

  // После открытия страницы оплаты ждём, пока вырастет баланс.
  useEffect(() => {
    if (!waiting) return;
    let cancelled = false;
    const startedAt = Date.now();
    const timer = window.setInterval(async () => {
      if (Date.now() - startedAt > BALANCE_POLL_MAX_MS) {
        window.clearInterval(timer);
        return;
      }
      try {
        const me = await getMe();
        const now = balanceFor(me);
        if (startBalance.current === null) startBalance.current = now;
        else if (now > startBalance.current && !cancelled) {
          window.clearInterval(timer);
          onPaid();
        }
      } catch {
        // следующая попытка через пять секунд
      }
    }, BALANCE_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [waiting, onPaid]);

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!method) return;
    const major = Number(amount.replace(',', '.'));
    if (!Number.isFinite(major) || major <= 0) {
      setError(t('Введите сумму.'));
      return;
    }
    const minor = Math.round(major * 100);
    if (minor < method.min_amount_kopeks || minor > method.max_amount_kopeks) {
      setError(
        t('Сумма для этого способа: от {min} до {max}.', {
          min: formatMoney(method.min_amount_kopeks, currency),
          max: formatMoney(method.max_amount_kopeks, currency),
        }),
      );
      return;
    }
    setBusy(true);
    setError(null);
    try {
      startBalance.current = balanceFor(await getMe());
      const result = await createTopUp(minor, method.id, optionId || undefined);
      await openPaymentUrl(result.payment_url);
      setWaiting(true);
    } catch (err) {
      if (!reportAuthLoss(err)) setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  if (waiting) {
    return (
      <Modal title={t('Оплата в браузере')} onClose={onClose}>
        <p style={{ margin: 0 }}>
          {t('Страница оплаты открыта в браузере. Оплатите — баланс обновится здесь сам, это занимает до минуты.')}
        </p>
        <div className="empty">
          <span className="spinner spinner-lg" aria-hidden />
        </div>
        <div className="modal-actions">
          <Button variant="ghost" onClick={onClose}>
            {t('Закрыть')}
          </Button>
        </div>
      </Modal>
    );
  }

  const options = method?.options?.filter((o) => o.id) ?? [];

  return (
    <Modal title={t('Пополнить баланс')} onClose={onClose}>
      {!methods && !error && (
        <div className="empty">
          <span className="spinner" aria-hidden />
        </div>
      )}
      {methods && methods.length === 0 && <p className="muted">{t('Способы оплаты сейчас недоступны. Пополните баланс в кабинете или у бота.')}</p>}
      {methods && methods.length > 0 && (
        <form onSubmit={submit} style={{ display: 'flex', flexDirection: 'column', gap: '0.75rem' }}>
          {neededMinor ? (
            <p className="muted" style={{ margin: 0 }}>
              {t('Не хватает {amount}.', { amount: formatMoney(neededMinor, currency) })}
            </p>
          ) : null}
          {methods.length > 1 && (
            <div className="option-grid">
              {methods.map((m) => (
                <button
                  type="button"
                  key={m.id}
                  className={`option ${m.id === methodId ? 'option-selected' : ''}`}
                  onClick={() => {
                    setMethodId(m.id);
                    setOptionId('');
                  }}
                >
                  <strong>{m.name}</strong>
                  {m.description && <span className="muted">{m.description}</span>}
                </button>
              ))}
            </div>
          )}
          {options.length > 0 && (
            <select className="node-select" value={optionId} onChange={(e) => setOptionId(e.target.value)}>
              <option value="">{t('Способ по умолчанию')}</option>
              {options.map((o) => (
                <option key={String(o.id)} value={String(o.id)}>
                  {String(o.name ?? o.id)}
                </option>
              ))}
            </select>
          )}
          <input
            className="text-input"
            inputMode="decimal"
            aria-label={t('Сумма')}
            placeholder={t('Сумма')}
            value={amount}
            onChange={(e) => setAmount(e.target.value.replace(/[^\d.,]/g, ''))}
          />
          {method && method.quick_amounts.length > 0 && (
            <div className="row" style={{ flexWrap: 'wrap', justifyContent: 'flex-start', gap: '0.4rem' }}>
              {method.quick_amounts.slice(0, 5).map((quick) => (
                <button type="button" key={quick} className="pill-btn" onClick={() => setAmount(String(Math.round(quick / 100)))}>
                  {formatMoney(quick, currency)}
                </button>
              ))}
            </div>
          )}
          {error && <p className="form-error">{error}</p>}
          <div className="modal-actions">
            <Button type="button" variant="ghost" onClick={onClose}>
              {t('Отмена')}
            </Button>
            <Button type="submit" loading={busy} disabled={!method}>
              {t('Перейти к оплате')}
            </Button>
          </div>
        </form>
      )}
      {error && !methods && <p className="form-error">{error}</p>}
    </Modal>
  );
}
