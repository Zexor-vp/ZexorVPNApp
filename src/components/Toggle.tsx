interface Props {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  label: string;
  hint?: string;
}

export default function Toggle({ checked, onChange, disabled = false, label, hint }: Props) {
  return (
    <div className="toggle-row">
      <div className="toggle-text">
        <span>{label}</span>
        {hint && <small>{hint}</small>}
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={label}
        disabled={disabled}
        className={`toggle ${checked ? 'toggle-on' : ''}`}
        onClick={() => onChange(!checked)}
      >
        <span className="toggle-knob" />
      </button>
    </div>
  );
}
