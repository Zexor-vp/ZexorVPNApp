import ProgressBar from './ProgressBar';
import type { UpdatePhase } from '../hooks/useAutoUpdate';
import { useT } from '../i18n';

/** Закрывает окно на время автообновления: приложение перезапустится само. */
export default function UpdateOverlay({ phase }: { phase: UpdatePhase }) {
  const t = useT();
  if (phase.kind === 'idle') return null;
  return (
    <div className="modal-backdrop" role="alertdialog" aria-live="polite" aria-label={t('Обновление приложения')}>
      <div className="modal" style={{ alignItems: 'stretch' }}>
        <h2>{t('Обновляем Zexor VPN')}</h2>
        <p className="muted" style={{ margin: 0 }}>
          {t('Версия {version}.', { version: phase.version })}{' '}
          {phase.kind === 'downloading'
            ? t('Скачиваем обновление…')
            : t('Устанавливаем, приложение перезапустится само.')}
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
