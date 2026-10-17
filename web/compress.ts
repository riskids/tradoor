/**
 * Each built file worth it, beside itself compressed at the best brotli and
 * gzip do — `index-….js.br`, `index-….js.gz` — for nginx to send as it is
 * (`brotli_static`, `gzip_static`; see deploy/nginx/tradoor.conf) rather than
 * squeeze it per request at its quickest: brotli takes a fifth or so more off
 * the scripts and the WebAssembly than nginx's own gzip did, once, at build.
 */
import { brotliCompressSync, constants, gzipSync } from 'node:zlib';
import type { Plugin } from 'vite';

const COMPRESSIBLE = /\.(js|css|wasm|svg|html|json|webmanifest|txt|xml)$/;
/** Smaller than this, a file is one packet either way. */
const AT_LEAST = 1024;

export function compress(): Plugin {
  return {
    name: 'tradoor-compress',
    apply: 'build',
    enforce: 'post',
    generateBundle(_options, bundle) {
      for (const [name, file] of Object.entries(bundle)) {
        if (!COMPRESSIBLE.test(name)) continue;
        const source = file.type === 'chunk' ? file.code : file.source;
        const data = typeof source === 'string' ? new TextEncoder().encode(source) : source;
        if (data.length < AT_LEAST) continue;
        const brotli = brotliCompressSync(data, { params: { [constants.BROTLI_PARAM_QUALITY]: constants.BROTLI_MAX_QUALITY, [constants.BROTLI_PARAM_SIZE_HINT]: data.length } });
        this.emitFile({ type: 'asset', fileName: `${name}.br`, source: brotli });
        this.emitFile({ type: 'asset', fileName: `${name}.gz`, source: gzipSync(data, { level: 9 }) });
      }
    },
  };
}
