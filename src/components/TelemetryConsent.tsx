import { useEffect, useState } from 'react';
import Button from './Button';
import Modal from './Modal';
import { useT } from '../i18n';
import { appSettings, setTelemetry } from '../lib/commands';

/** Вопрос о согласии при первом запуске (и один раз после обновления у тех, кого раньше не спрашивали). */
export default function TelemetryConsent() {
  const t = useT();
  const [open, setOpen] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    void appSettings()
      .then((settings) => setOpen(!settings.telemetry_decided))
      .catch(() => undefined);
  }, []);

  if (!open) return null;

  async function answer(enabled: boolean) {
    setSaving(true);
    try {
      await setTelemetry(enabled);
      setOpen(false);
    } catch {
      setSaving(false);
    }
  }

  return (
    <Modal title={t('Помогите сделать Zexor лучше')} onClose={() => undefined}>
      <p>{t('Разрешить приложению отправлять анонимную техническую информацию?')}</p>
      <ul style={{ margin: '0.25rem 0', paddingLeft: '1.2rem' }}>
        <li>{t('время отклика серверов Zexor из вашей сети и название оператора;')}</li>
        <li>{t('отчёты о сбоях: версия приложения, модель устройства и текст ошибки.')}</li>
      </ul>
      <p className="muted">
        {t('Без IP-адреса, почты и привязки к аккаунту. Это помогает быстрее находить блокировки и исправлять вылеты. Решение можно изменить в любой момент в профиле.')}
      </p>
      <div className="modal-actions">
        <Button variant="ghost" disabled={saving} onClick={() => void answer(false)}>
          {t('Не разрешать')}
        </Button>
        <Button loading={saving} onClick={() => void answer(true)}>
          {t('Разрешить')}
        </Button>
      </div>
    </Modal>
  );
}
