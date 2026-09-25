/** Sizes as a trader writes them: `5M`, `750K`, `2.5b`, `750000`. */

/** `5M`, `500K` — what a threshold is shown as. */
export function shortUsd(v: number): string {
  if (v >= 1e9) return `${+(v / 1e9).toFixed(2)}B`;
  if (v >= 1e6) return `${+(v / 1e6).toFixed(2)}M`;
  if (v >= 1e3) return `${+(v / 1e3).toFixed(0)}K`;
  return String(Math.round(v));
}

/** Reads what `shortUsd` writes, and the plain numbers between. Null if it is not a size. */
export function readUsd(text: string): number | null {
  const m = /^\s*\$?([\d.,]+)\s*([kmb])?\s*$/i.exec(text);
  if (!m) return null;
  const n = Number(m[1]!.replaceAll(',', ''));
  if (!Number.isFinite(n) || n <= 0) return null;
  const scale = { k: 1e3, m: 1e6, b: 1e9 }[(m[2] ?? '').toLowerCase()] ?? 1;
  return n * scale;
}
