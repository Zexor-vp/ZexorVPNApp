import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  // Tauri обслуживает фронтенд с фиксированного порта в dev-режиме.
  server: { port: 1420, strictPort: true },
  build: { outDir: 'dist', target: 'chrome105' },
  clearScreen: false,
});
