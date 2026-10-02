interface Option<T extends string> {
  value: T;
  label: string;
}

interface Props<T extends string> {
  value: T;
  options: Option<T>[];
  onChange: (value: T) => void;
  disabled?: boolean;
}

export default function Segmented<T extends string>({ value, options, onChange, disabled }: Props<T>) {
  return (
    <div className="segmented" role="radiogroup">
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          role="radio"
          aria-checked={value === option.value}
          disabled={disabled}
          className={`segment ${value === option.value ? 'segment-active' : ''}`}
          onClick={() => value !== option.value && onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}
