// Только для визуальной проверки: `npx vite --config vite.mock.config.ts` подставляет моки вместо Tauri.
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';

const mock = (name: string) => fileURLToPath(new URL(`./tools/mock/${name}.ts`, import.meta.url));

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      '@tauri-apps/api/core': mock('core'),
      '@tauri-apps/api/app': mock('app'),
      '@tauri-apps/api/event': mock('event'),
      '@tauri-apps/plugin-updater': mock('updater'),
      '@tauri-apps/plugin-process': mock('process'),
      '@tauri-apps/plugin-dialog': mock('dialog'),
      '@tauri-apps/plugin-notification': mock('notification'),
    },
  },
  build: { assetsInlineLimit: 200000 },
  server: { port: 1421, strictPort: true },
});
