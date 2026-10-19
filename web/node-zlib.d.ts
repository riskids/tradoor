/** The few Node calls web/compress.ts makes at build, typed here rather than with the whole of @types/node. */
declare module 'node:zlib' {
  export const constants: { BROTLI_PARAM_QUALITY: number; BROTLI_PARAM_SIZE_HINT: number; BROTLI_MAX_QUALITY: number };
  export function brotliCompressSync(data: Uint8Array, options?: { params?: Record<number, number> }): Uint8Array;
  export function gzipSync(data: Uint8Array, options?: { level?: number }): Uint8Array;
}

declare module 'node:fs' {
  export function readFileSync(path: string): Uint8Array;
  export function writeFileSync(path: string, data: Uint8Array): void;
}
