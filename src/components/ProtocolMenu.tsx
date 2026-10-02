import { useCallback, useRef, useState } from 'react';
import { CheckIcon, ChevronDownIcon } from './Icons';
import { useOutsideClose } from '../hooks/useOutsideClose';

export type ProtocolValue = 'vless' | 'wireguard';

const OPTIONS: { value: ProtocolValue; short: string; title: string; hint: string; warn?: boolean }[] = [
  { value: 'vless', short: 'VLESS', title: 'VLESS (REALITY)', hint: 'Работает почти везде, в том числе в России и Иране' },
  { value: 'wireguard', short: 'WireGuard', title: 'WireGuard', hint: 'Простой и быстрый, но не работает в России и Иране', warn: true },
];

interface Props {
  value: ProtocolValue;
  onChange: (value: ProtocolValue) => void;
  disabled?: boolean;
}

/** Компактная кнопка-«пилюля» с всплывающим окном выбора протокола. */
export default function ProtocolMenu({ value, onChange, disabled }: Props) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useOutsideClose(ref, open, close);

  const current = OPTIONS.find((o) => o.value === value) ?? OPTIONS[0];

  return (
    <div className="dropdown dropdown-compact" ref={ref}>
      <button
        type="button"
        className="pill-btn"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={`Протокол: ${current.title}`}
        disabled={disabled}
        onClick={() => setOpen((v) => !v)}
      >
        {current.short}
        <ChevronDownIcon width={14} height={14} />
      </button>
      {open && (
        <div className="dropdown-panel dropdown-panel-compact" role="listbox">
          <span className="label" style={{ padding: '0.3rem 0.75rem 0.15rem', display: 'block' }}>
            Протокол
          </span>
          {OPTIONS.map((option) => (
            <button
              key={option.value}
              type="button"
              role="option"
              aria-selected={option.value === value}
              className={`dropdown-item ${option.value === value ? 'dropdown-item-active' : ''}`}
              onClick={() => {
                setOpen(false);
                if (option.value !== value) onChange(option.value);
              }}
            >
              <span className="dropdown-item-main">
                <strong>{option.title}</strong>
                <span className={option.warn ? 'warn-text' : 'muted'}>{option.hint}</span>
              </span>
              {option.value === value && <CheckIcon />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
