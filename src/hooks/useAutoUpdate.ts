import { useEffect, useState } from 'react';
import { check } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';

export type UpdatePhase =
  | { kind: 'idle' }
  | { kind: 'downloading'; version: string; percent: number | null }
  | { kind: 'installing'; version: string };

/**
 * При запуске проверяет обновления и, если есть, сразу скачивает, ставит и перезапускает приложение.
 * Любая ошибка (нет сети, нет релиза, подпись не сошлась) тихо игнорируется: обновление — не повод
 * мешать человеку пользоваться VPN, ручная проверка остаётся в профиле.
 */
export function useAutoUpdate(): UpdatePhase {
  const [phase, setPhase] = useState<UpdatePhase>({ kind: 'idle' });

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const update = await check();
        if (!update || cancelled) return;
        const version = update.version;
        setPhase({ kind: 'downloading', version, percent: null });

        let total = 0;
        let received = 0;
        await update.downloadAndInstall((event) => {
          if (event.event === 'Started') {
            total = event.data.contentLength ?? 0;
          } else if (event.event === 'Progress') {
            received += event.data.chunkLength;
            setPhase({ kind: 'downloading', version, percent: total > 0 ? Math.min(100, Math.round((received * 100) / total)) : null });
          } else if (event.event === 'Finished') {
            setPhase({ kind: 'installing', version });
          }
        });
        await relaunch();
      } catch {
        if (!cancelled) setPhase({ kind: 'idle' });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  return phase;
}
