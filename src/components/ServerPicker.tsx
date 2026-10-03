import { useCallback, useEffect, useRef, useState } from 'react';
import { ChevronDownIcon, RefreshIcon } from './Icons';
import { useMediaQuery } from '../hooks/useMediaQuery';
import { useOutsideClose } from '../hooks/useOutsideClose';
import { useT } from '../i18n';
import { ServerName } from './Flag';
import type { NodeSummary } from '../lib/commands';

/** `undefined` — ещё меряем, `null` — не отвечает, число — миллисекунды. */
export type PingValue = number | null | undefined;

export function PingBadge({ ms }: { ms: PingValue }) {
  const t = useT();
  if (ms === undefined) return <span className="ping ping-pending">…</span>;
  if (ms === null) return <span className="ping ping-down">{t('недоступен')}</span>;
  const tone = ms < 150 ? 'ping-good' : ms < 300 ? 'ping-mid' : 'ping-bad';
  return <span className={`ping ${tone}`}>{t('{ms} мс', { ms })}</span>;
}

interface Props {
  nodes: NodeSummary[];
  selected: string;
  pings: Record<string, PingValue>;
  pinging: boolean;
  disabled?: boolean;
  /** Авто-выбор сервера: список заблокирован и притушен, вместо сервера — надпись. */
  auto?: boolean;
  onSelect: (value: string) => void;
  onRefreshPings: () => void;
}

export default function ServerPicker({ nodes, selected, pings, pinging, disabled, auto, onSelect, onRefreshPings }: Props) {
  const t = useT();
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useOutsideClose(ref, open, close);

  const current = nodes.find((n) => n.remark === selected);
  // В полноэкранном режиме список серверов всегда раскрыт: места хватает, а выпадающий список только перекрывал бы карточки.
  const expanded = useMediaQuery('(min-width: 900px)');

  // Авто включили при открытом списке — закрываем его.
  useEffect(() => {
    if (auto) setOpen(false);
  }, [auto]);

  function toggle() {
    if (!open) onRefreshPings(); // пинг обновляем при каждом раскрытии списка
    setOpen((v) => !v);
  }

  if (expanded) {
    return (
      <div className={`server-list ${auto ? 'server-list-auto' : ''}`} role="listbox" aria-label={t('Серверы')}>
        <div className="row">
          <span className="label">{auto ? t('Авто выбор сервера') : t('Сервер')}</span>
          <button type="button" className="link-btn" disabled={pinging || auto} onClick={onRefreshPings}>
            <RefreshIcon /> {pinging ? t('Измеряем…') : t('Обновить пинг')}
          </button>
        </div>
        {nodes.length === 0 && <span className="muted">{t('Нет серверов')}</span>}
        {nodes.map((node) => {
          const active = !auto && node.remark === selected;
          return (
            <button
              key={node.remark}
              type="button"
              role="option"
              aria-selected={active}
              disabled={disabled || auto}
              className={`dropdown-item ${active ? 'dropdown-item-active' : ''}`}
              onClick={() => onSelect(node.remark)}
            >
              <span className="dropdown-item-main">
                <strong>
                  <ServerName remark={node.remark} />
                </strong>
                <span className="muted">{node.protocol === 'wireguard' ? 'WireGuard' : node.is_reality ? 'VLESS · REALITY' : 'VLESS'}</span>
              </span>
              <PingBadge ms={pings[node.remark]} />
            </button>
          );
        })}
      </div>
    );
  }

  return (
    <div className="dropdown" ref={ref}>
      <button
        type="button"
        className={`dropdown-trigger ${auto ? 'dropdown-trigger-auto' : ''}`}
        aria-haspopup="listbox"
        aria-expanded={open}
        disabled={disabled || auto || nodes.length === 0}
        onClick={toggle}
      >
        <span>
          <span className="label">{t('Сервер')}</span>
          <span className="dropdown-value">
            {auto ? (
              <span className="auto-caption">{t('Авто выбор сервера').toUpperCase()}</span>
            ) : nodes.length === 0 ? (
              t('Нет серверов')
            ) : (
              <>
                {current ? <ServerName remark={current.remark} /> : '—'} {current && <PingBadge ms={pings[current.remark]} />}
              </>
            )}
          </span>
        </span>
        <ChevronDownIcon />
      </button>

      {open && !auto && (
        <div className="dropdown-panel" role="listbox">
          {nodes.map((node) => (
            <button
              key={node.remark}
              type="button"
              role="option"
              aria-selected={node.remark === selected}
              className={`dropdown-item ${node.remark === selected ? 'dropdown-item-active' : ''}`}
              onClick={() => {
                setOpen(false);
                onSelect(node.remark);
              }}
            >
              <span className="dropdown-item-main">
                <strong>
                  <ServerName remark={node.remark} />
                </strong>
                <span className="muted">{node.protocol === 'wireguard' ? 'WireGuard' : node.is_reality ? 'VLESS · REALITY' : 'VLESS'}</span>
              </span>
              <PingBadge ms={pings[node.remark]} />
            </button>
          ))}

          <button type="button" className="dropdown-footer" disabled={pinging} onClick={onRefreshPings}>
            <RefreshIcon /> {pinging ? t('Измеряем…') : t('Обновить пинг')}
          </button>
        </div>
      )}
    </div>
  );
}
