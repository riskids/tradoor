/** Chart timeframes, in TradingView's spelling. The engine validates every one
 *  it is handed; this table exists so the toolbar can render without waiting
 *  for the worker to start. */

export interface TimeframeDef {
  value: string;
  label: string;
  name: string;
  /** Minutes, hours, days, weeks, months. */
  group: number;
  /** Whether the exchange streams a candle of this length. Folded timeframes
   *  (2D, 2W, 3M…) show closed candles only. */
  live: boolean;
}

const DEFS: [string, string, string, number, boolean][] = [
  ['1m', '1m', '1 minute', 0, true],
  ['5m', '5m', '5 minutes', 0, true],
  ['15m', '15m', '15 minutes', 0, true],
  ['30m', '30m', '30 minutes', 0, true],
  ['1h', '1h', '1 hour', 1, true],
  ['2h', '2h', '2 hours', 1, true],
  ['4h', '4h', '4 hours', 1, true],
  ['6h', '6h', '6 hours', 1, true],
  ['12h', '12h', '12 hours', 1, true],
  ['1d', 'D', 'Daily', 2, true],
  ['2d', '2D', '2 days', 2, false],
  ['3d', '3D', '3 days', 2, false],
  ['1w', 'W', 'Weekly', 3, true],
  ['2w', '2W', '2 weeks', 3, false],
  ['1M', 'M', 'Monthly', 4, true],
  ['3M', '3M', '3 months', 4, false],
  ['6M', '6M', '6 months', 4, false],
  ['12M', '12M', '12 months', 4, false],
];

export const TIMEFRAMES: readonly TimeframeDef[] = DEFS.map(([value, label, name, group, live]) => ({ value, label, name, group, live }));

const BY_VALUE = new Map(TIMEFRAMES.map((t) => [t.value, t]));

/** The chart bar's buttons until the reader stars their own (`WorkspaceDoc.timeframes`). */
export const FAVORITE_TIMEFRAMES = ['1m', '5m', '15m', '1h', '4h', '1d', '1w'];

export const isTimeframe = (v: unknown): v is string => typeof v === 'string' && BY_VALUE.has(v);

export const tfLabel = (tf: string): string => BY_VALUE.get(tf)?.label ?? tf;

export const tfName = (tf: string): string => BY_VALUE.get(tf)?.name ?? tf;

/** Is `a` longer than `b`? The list runs shortest first. */
export const tfLonger = (a: string, b: string): boolean => TIMEFRAMES.findIndex((t) => t.value === a) > TIMEFRAMES.findIndex((t) => t.value === b);

const APPROX_MS: Record<string, number> = { m: 60_000, h: 3_600_000, d: 86_400_000, w: 604_800_000, M: 2_592_000_000 };

/** Roughly how long one bar is; a month counts as thirty days. */
export const tfMs = (tf: string): number => (Number.parseInt(tf, 10) || 1) * (APPROX_MS[tf.slice(-1)] ?? 60_000);

/** One step shorter or longer, stopping at either end. */
export function stepTimeframe(tf: string, delta: number): string {
  const i = TIMEFRAMES.findIndex((t) => t.value === tf);
  const next = TIMEFRAMES[Math.max(0, Math.min(TIMEFRAMES.length - 1, i + delta))];
  return next?.value ?? tf;
}
