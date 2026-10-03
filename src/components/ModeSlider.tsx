import { useT } from '../i18n';
import type { TunnelMode } from '../lib/commands';

interface Props {
  value: TunnelMode;
  /** Идёт подключение или смена режима. */
  disabled?: boolean;
  onChange: (mode: TunnelMode) => void;
}

/** Ползунок переключения Proxy ⇄ TUN. */
export default function ModeSlider({ value, disabled, onChange }: Props) {
  const t = useT();
  return (
    <div
      className={`mode-slider ${value === 'tun' ? 'mode-slider-tun' : ''}`}
      role="radiogroup"
      aria-label={t('Режим работы')}
    >
      <span className="mode-thumb" aria-hidden />
      {(['proxy', 'tun'] as const).map((mode) => (
        <button
          key={mode}
          type="button"
          role="radio"
          aria-checked={value === mode}
          disabled={disabled}
          className={`mode-option ${value === mode ? 'mode-option-active' : ''}`}
          onClick={() => value !== mode && onChange(mode)}
        >
          {mode === 'proxy' ? 'Proxy' : 'TUN'}
        </button>
      ))}
    </div>
  );
}
