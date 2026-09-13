interface Props {
  connected: boolean;
  pending: boolean;
}

export default function StatusPill({ connected, pending }: Props) {
  const label = pending ? 'Подключение…' : connected ? 'Защищено' : 'Отключено';
  const tone = pending ? 'pending' : connected ? 'on' : 'off';
  return (
    <span className={`status-pill status-pill-${tone}`}>
      <span className="status-dot" />
      {label}
    </span>
  );
}
