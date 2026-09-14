import { defineConfig } from 'vite';
import solid from 'vite-plugin-solid';

declare const process: { env: Record<string, string | undefined> };

/** Where `tradoor-server` listens in development. The page asks for /api and
 *  /data on its own origin, exactly as it will behind nginx in production. */
const server = process.env.TRADOOR_API ?? 'http://127.0.0.1:8080';

export default defineConfig({
  plugins: [solid()],
  server: {
    port: 5173,
    proxy: { '/api': server, '/data': server },
  },
  worker: { format: 'es' },
  build: {
    target: 'es2022',
    modulePreload: { polyfill: false },
    assetsInlineLimit: 0,
  },
});
