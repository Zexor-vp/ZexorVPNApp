import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App';
import TelemetryConsent from './components/TelemetryConsent';
import { I18nProvider } from './i18n';
import './styles/globals.css';
import { invoke } from '@tauri-apps/api/core';

// Контекстное меню (долгое нажатие / правая кнопка: «копировать», «сохранить картинку») отключено везде, кроме полей ввода.
window.addEventListener('contextmenu', (event) => {
  const target = event.target as HTMLElement | null;
  if (target?.closest('input, textarea, [contenteditable="true"]')) return;
  event.preventDefault();
});

// Необработанные ошибки интерфейса уходят на сервер (в админ-панели — «Ошибки приложений»).
const reportUiError = (message: string) => {
  if (!message || /ResizeObserver loop/i.test(message)) return;
  void invoke('report_app_error', { kind: 'js', message: message.slice(0, 3500) }).catch(() => undefined);
};
window.addEventListener('error', (event) => {
  const where = event.filename ? ` (${event.filename.split('/').pop()}:${event.lineno})` : '';
  reportUiError(`${event.message}${where}${event.error?.stack ? `\n${event.error.stack}` : ''}`);
});
window.addEventListener('unhandledrejection', (event) => {
  const reason = event.reason;
  reportUiError(`Unhandled rejection: ${reason instanceof Error ? `${reason.message}\n${reason.stack ?? ''}` : String(reason)}`);
});

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <I18nProvider>
      <App />
      <TelemetryConsent />
    </I18nProvider>
  </React.StrictMode>,
);
