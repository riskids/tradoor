import { defineConfig } from 'vite';
import solid from 'vite-plugin-solid';
import { site } from './site';

declare const process: { env: Record<string, string | undefined> };

/** Where `tradoor-server` listens in development. The page asks for /api and
 *  /data on its own origin, exactly as it will behind nginx in production. */
const server = process.env.TRADOOR_API ?? 'http://127.0.0.1:8080';

export default defineConfig({
  plugins: [solid(), site()],
  server: {
    port: 5173,
    // ws: the exchange stream relay for networks that block the exchanges.
    proxy: { '/api': { target: server, ws: true }, '/data': server },
  },
  worker: { format: 'es' },
  build: {
    target: 'es2022',
    modulePreload: { polyfill: false },
    assetsInlineLimit: 0,
  },
});
