/**
 * Each built file worth it, beside itself compressed at the best brotli and
 * gzip do — `index-….js.br`, `index-….js.gz` — for nginx to send as it is
 * (`brotli_static`, `gzip_static`; see deploy/nginx/tradoor.conf) rather than
 * squeeze it per request at its quickest: brotli takes a fifth or so more off
 * the scripts and the WebAssembly than nginx's own gzip did, once, at build.
 *
 * Read back from disk once written, not from the bundle: Vite fills in some
 * of a chunk's code — `__VITE_PRELOAD__`, the files a lazy import loads with
 * it — in its own last steps, and a copy taken before them is a chunk that
 * throws on its first lazy import. Every browser reading the compressed copy
 * got that until the page itself failed to start.
 */
import { readFileSync, writeFileSync } from 'node:fs';
import { brotliCompressSync, constants, gzipSync } from 'node:zlib';
import type { Plugin } from 'vite';

const COMPRESSIBLE = /\.(js|css|wasm|svg|html|json|webmanifest|txt|xml)$/;
/** Smaller than this, a file is one packet either way. */
const AT_LEAST = 1024;

export function compress(): Plugin {
  return {
    name: 'tradoor-compress',
    apply: 'build',
    writeBundle(options, bundle) {
      for (const name of Object.keys(bundle)) {
        if (!COMPRESSIBLE.test(name)) continue;
        const path = `${options.dir}/${name}`;
        const data = readFileSync(path);
        if (data.length < AT_LEAST) continue;
        writeFileSync(`${path}.br`, brotliCompressSync(data, { params: { [constants.BROTLI_PARAM_QUALITY]: constants.BROTLI_MAX_QUALITY, [constants.BROTLI_PARAM_SIZE_HINT]: data.length } }));
        writeFileSync(`${path}.gz`, gzipSync(data, { level: 9 }));
      }
    },
  };
}
