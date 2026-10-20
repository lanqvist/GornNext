import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import { resolve } from 'node:path';

/**
 * Страницы приложения на Tauri: главное окно, оверлей, слой плашек, пульт,
 * стрелки тюрьмы. Сборка — в app/dist (frontendDist в tauri.conf.json).
 */
export default defineConfig({
  root: resolve(import.meta.dirname, 'src/renderer'),
  publicDir: resolve(import.meta.dirname, 'public'),
  base: './',
  plugins: [react()],
  build: {
    outDir: resolve(import.meta.dirname, '../app/dist'),
    emptyOutDir: true,
    // WebView2 — тот же Chromium, что у Electron.
    target: 'chrome130',
    chunkSizeWarningLimit: 1000,
    rollupOptions: {
      input: {
        index: resolve(import.meta.dirname, 'src/renderer/index.html'),
        overlay: resolve(import.meta.dirname, 'src/renderer/overlay.html'),
        toasts: resolve(import.meta.dirname, 'src/renderer/toasts.html'),
        panel: resolve(import.meta.dirname, 'src/renderer/panel.html'),
        prison: resolve(import.meta.dirname, 'src/renderer/prison.html'),
      },
    },
  },
  server: { port: 5274, strictPort: true },
});
