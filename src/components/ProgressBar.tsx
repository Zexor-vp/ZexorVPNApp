interface Props {
  /** 0–100 */
  percent: number;
}

export default function ProgressBar({ percent }: Props) {
  const clamped = Math.max(0, Math.min(100, percent));
  const tone = clamped >= 95 ? 'progress-fill-danger' : clamped >= 80 ? 'progress-fill-warn' : '';
  return (
    <div>
      <div className="progress" role="progressbar" aria-valuenow={Math.round(clamped)} aria-valuemin={0} aria-valuemax={100}>
        <div className={`progress-fill ${tone}`} style={{ width: `${clamped}%` }} />
        {[25, 50, 75].map((tick) => (
          <span key={tick} className="progress-tick" style={{ left: `${tick}%` }} />
        ))}
      </div>
    </div>
  );
}
