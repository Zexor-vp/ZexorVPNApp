import ProgressBar from './ProgressBar';
import type { UpdatePhase } from '../hooks/useAutoUpdate';

/** Закрывает окно на время автообновления: приложение перезапустится само. */
export default function UpdateOverlay({ phase }: { phase: UpdatePhase }) {
  if (phase.kind === 'idle') return null;
  return (
    <div className="modal-backdrop" role="alertdialog" aria-live="polite" aria-label="Обновление приложения">
      <div className="modal" style={{ alignItems: 'stretch' }}>
        <h2>Обновляем Zexor VPN</h2>
        <p className="muted" style={{ margin: 0 }}>
          Версия {phase.version}.{' '}
          {phase.kind === 'downloading' ? 'Скачиваем обновление…' : 'Устанавливаем, приложение перезапустится само.'}
        </p>
        {phase.kind === 'downloading' && phase.percent !== null ? (
          <ProgressBar percent={phase.percent} />
        ) : (
          <div className="empty">
            <span className="spinner spinner-lg" aria-hidden />
          </div>
        )}
      </div>
    </div>
  );
}
