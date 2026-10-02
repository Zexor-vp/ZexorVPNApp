import { useMemo, useState, type ReactNode } from 'react';
import Button from './Button';
import Modal from './Modal';
import TopUpModal from './TopUpModal';
import { reportAuthLoss } from '../hooks/useAsync';
import {
  balanceFromOptions,
  formatMoney,
  previewTariffSwitch,
  purchaseTariff,
  renew,
  switchTariff,
  type PurchaseOptions,
  type SubscriptionData,
  type SwitchPreview,
  type TariffOption,
  type TariffPeriod,
} from '../lib/cabinet';
import { errorMessage } from '../lib/commands';

interface Props {
  options: PurchaseOptions;
  subscription: SubscriptionData | null;
  /** Покупка прошла — страница перечитает данные. */
  onDone: (message: string) => void;
}

/** Общий диалог подтверждения: цена, баланс и пополнение, если денег не хватает. */
function ConfirmDialog({
  title,
  body,
  currency,
  neededMinor,
  working,
  error,
  confirmLabel = 'Подтвердить',
  onConfirm,
  onClose,
  onToppedUp,
}: {
  title: string;
  body: ReactNode;
  currency: string;
  neededMinor: number;
  working: boolean;
  error: string | null;
  confirmLabel?: string;
  onConfirm: () => void;
  onClose: () => void;
  onToppedUp: () => void;
}) {
  const [topUp, setTopUp] = useState(false);
  return (
    <>
      <Modal title={title} onClose={() => !working && onClose()}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: '0.6rem' }}>{body}</div>
        {neededMinor > 0 && <p className="form-error">Не хватает {formatMoney(neededMinor, currency)} на балансе.</p>}
        {error && <p className="form-error">{error}</p>}
        <div className="modal-actions">
          <Button variant="ghost" disabled={working} onClick={onClose}>
            Отмена
          </Button>
          {neededMinor > 0 ? (
            <Button onClick={() => setTopUp(true)}>Пополнить на {formatMoney(neededMinor, currency)}</Button>
          ) : (
            <Button loading={working} onClick={onConfirm}>
              {confirmLabel}
            </Button>
          )}
        </div>
      </Modal>
      {topUp && (
        <TopUpModal
          currency={currency}
          neededMinor={neededMinor}
          onClose={() => setTopUp(false)}
          onPaid={() => {
            setTopUp(false);
            onToppedUp();
          }}
        />
      )}
    </>
  );
}

function PeriodGrid({ tariff, onPick }: { tariff: TariffOption; onPick: (period: TariffPeriod) => void }) {
  return (
    <div className="option-grid">
      {tariff.periods.map((period) => (
        <button key={period.days} className="option" onClick={() => onPick(period)}>
          <strong>{period.label}</strong>
          <span className="muted">{period.price_label}</span>
          {period.original_price_label && <span className="muted old-price">{period.original_price_label}</span>}
          {period.discount_percent ? <span className="chip">−{period.discount_percent}%</span> : null}
        </button>
      ))}
    </div>
  );
}

const sellable = (t: TariffOption) => t.is_available && !t.is_daily && t.periods.length > 0;

type Step =
  | { kind: 'tariffs' }
  | { kind: 'periods'; tariff: TariffOption }
  | { kind: 'confirm'; tariff: TariffOption; period: TariffPeriod }
  | { kind: 'switch'; tariff: TariffOption; preview: SwitchPreview };

/**
 * Один блок «Продление и тариф»: сначала выбирается тариф, дальше по ситуации:
 *  - текущий тариф → выбор срока → продление;
 *  - другой тариф при действующей платной подписке → смена: остаток текущего тарифа засчитывается,
 *    платится только разница за оставшиеся дни (считает бэкенд по формуле бота), цену показываем после выбора;
 *  - подписки нет, она закончилась или пробная → выбор срока → покупка по полной цене.
 * Не хватает денег — в подтверждении сразу предлагается пополнение со страницей оплаты.
 */
export default function RenewalSection({ options, subscription, onDone }: Props) {
  const [step, setStep] = useState<Step>({ kind: 'tariffs' });
  const [loadingId, setLoadingId] = useState<number | null>(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const currency = options.currency;
  const balance = balanceFromOptions(options);
  const tariffs = useMemo(() => (options.tariffs ?? []).filter(sellable), [options.tariffs]);
  const canSwitch = options.has_subscription && !options.subscription_is_expired && subscription !== null && !subscription.is_trial;
  const daysLeft = subscription?.days_left ?? 0;

  async function pickTariff(tariff: TariffOption) {
    setError(null);
    if (!tariff.is_current && canSwitch) {
      setLoadingId(tariff.id);
      try {
        const preview = await previewTariffSwitch(tariff.id, tariff.periods[0].days);
        setStep({ kind: 'switch', tariff, preview });
      } catch (err) {
        if (!reportAuthLoss(err)) setError(errorMessage(err));
      } finally {
        setLoadingId(null);
      }
      return;
    }
    setStep({ kind: 'periods', tariff });
  }

  async function confirm() {
    if (step.kind !== 'confirm' && step.kind !== 'switch') return;
    setWorking(true);
    setError(null);
    try {
      if (step.kind === 'switch') {
        await switchTariff(step.tariff.id, step.tariff.periods[0].days);
        onDone(`Тариф изменён на «${step.tariff.name}».`);
      } else if (step.tariff.is_current) {
        await renew(step.period.days);
        onDone('Подписка продлена.');
      } else {
        await purchaseTariff(step.tariff.id, step.period.days);
        onDone(`Тариф «${step.tariff.name}» оформлен.`);
      }
      setStep({ kind: 'tariffs' });
    } catch (err) {
      if (!reportAuthLoss(err)) setError(errorMessage(err));
    } finally {
      setWorking(false);
    }
  }

  const needed =
    step.kind === 'confirm' ? Math.max(0, step.period.price_kopeks - balance) : step.kind === 'switch' ? step.preview.missing_amount_kopeks : 0;

  const back = () => setStep(step.kind === 'confirm' ? { kind: 'periods', tariff: step.tariff } : { kind: 'tariffs' });

  return (
    <section className="card">
      <div className="row">
        <span className="label">{step.kind === 'tariffs' ? 'Продление и тариф' : 'Выбор тарифа'}</span>
        {step.kind !== 'tariffs' && (
          <button className="link-btn" onClick={back}>
            ← Назад
          </button>
        )}
      </div>

      {error && step.kind !== 'confirm' && step.kind !== 'switch' && <p className="form-error">{error}</p>}

      {step.kind === 'tariffs' && (
        <>
          {canSwitch && (
            <p className="muted" style={{ margin: 0 }}>
              При смене тарифа учитывается остаток текущего: у вас {daysLeft > 0 ? `${daysLeft} дн.` : 'ещё есть время'} подписки, срок не
              теряется, а платится только разница. Точную сумму покажем после выбора тарифа.
            </p>
          )}
          <div className="list">
            {tariffs.length === 0 && <p className="muted">Тарифы сейчас недоступны.</p>}
            {tariffs.map((tariff) => (
              <button key={tariff.id} className="tariff-card" disabled={loadingId !== null} onClick={() => void pickTariff(tariff)}>
                <span className="tariff-head">
                  <strong>{tariff.name}</strong>
                  {tariff.is_current && <span className="chip">Текущий</span>}
                </span>
                <span className="muted">
                  {tariff.traffic_limit_label} · устройств: {tariff.device_limit}
                  {tariff.servers_count > 0 ? ` · серверов: ${tariff.servers_count}` : ''}
                </span>
                {tariff.description && <span className="muted tariff-desc">{tariff.description}</span>}
                <span className="tariff-cta">
                  {loadingId === tariff.id
                    ? 'Считаем…'
                    : tariff.is_current
                      ? 'Продлить'
                      : canSwitch
                        ? 'Перейти на этот тариф'
                        : `Выбрать · от ${tariff.periods[0].price_label}`}
                </span>
              </button>
            ))}
          </div>
        </>
      )}

      {step.kind === 'periods' && (
        <>
          <p style={{ margin: 0 }}>
            <strong>{step.tariff.name}</strong>
            <span className="muted"> — выберите срок</span>
          </p>
          <PeriodGrid
            tariff={step.tariff}
            onPick={(period) => {
              setError(null);
              setStep({ kind: 'confirm', tariff: step.tariff, period });
            }}
          />
        </>
      )}

      {(step.kind === 'confirm' || step.kind === 'switch') && (
        <ConfirmDialog
          title={step.kind === 'switch' ? 'Сменить тариф' : step.tariff.is_current ? 'Продлить подписку' : 'Оформить подписку'}
          currency={currency}
          neededMinor={needed}
          working={working}
          error={error}
          onConfirm={() => void confirm()}
          onClose={back}
          onToppedUp={() => onDone('Баланс пополнен — теперь можно подтвердить покупку.')}
          body={
            step.kind === 'switch' ? (
              <>
                <p style={{ margin: 0 }}>Перейти на тариф «{step.tariff.name}».</p>
                <p className="muted" style={{ margin: 0 }}>
                  Остаток текущего тарифа ({step.preview.remaining_days} дн.) засчитан. Срок подписки сохранится.
                </p>
                <p style={{ margin: 0 }}>
                  {step.preview.upgrade_cost_kopeks > 0 ? (
                    <>
                      К оплате: <strong>{step.preview.upgrade_cost_label}</strong>. Сумма спишется с баланса.
                    </>
                  ) : (
                    <strong>Доплата не нужна.</strong>
                  )}
                </p>
              </>
            ) : (
              <p style={{ margin: 0 }}>
                {step.tariff.is_current ? 'Продлить' : 'Оформить'} тариф «{step.tariff.name}» на {step.period.label.toLowerCase()} за{' '}
                <strong>{step.period.price_label}</strong>. Сумма спишется с баланса ({formatMoney(balance, currency)}).
              </p>
            )
          }
        />
      )}
    </section>
  );
}
