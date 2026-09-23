/**
 * What the values panel reads from closed UTC days: returns over the usual
 * periods, the day's average true range and the prior day's, week's and month's
 * highs and lows. Pure, so it is tested without an exchange.
 */
import type { DailyBars } from '../engine/protocol';

export const DAY = 86_400_000;

export interface Period {
  label: string;
  /** Percent from the reference close to `price`, or null without that much history. */
  pct: number | null;
}

/** The close of the day that opened at `time`, if held. */
function closeAt(d: DailyBars, time: number): number | null {
  // Days are in order and one apart, so the index is a subtraction away — but gaps happen.
  for (let i = d.time.length - 1; i >= 0; i -= 1) {
    if (d.time[i] === time) return d.close[i]!;
    if (d.time[i]! < time) return null;
  }
  return null;
}

const pct = (price: number, from: number | null): number | null => (from && from > 0 ? ((price - from) / from) * 100 : null);

/**
 * Returns to `price`, today's open (`today`, 00:00 UTC) as the end of the last closed day:
 * 7D is against the close seven days before today's open, YTD against the year's last close.
 */
export function performance(d: DailyBars, today: number, price: number): Period[] {
  const ago = (days: number) => closeAt(d, today - days * DAY);
  const year = Date.UTC(new Date(today).getUTCFullYear(), 0, 1);
  return [
    { label: '7D', pct: pct(price, ago(7)) },
    { label: '30D', pct: pct(price, ago(30)) },
    { label: '90D', pct: pct(price, ago(90)) },
    { label: 'YTD', pct: pct(price, today === year ? price : closeAt(d, year - DAY)) },
    { label: '1Y', pct: pct(price, ago(365)) },
  ];
}

/** Average true range over `n` days, Wilder-smoothed as TradingView's `ta.atr`. */
export function atr(d: DailyBars, n = 14): number | null {
  const len = d.time.length;
  if (len < n + 1) return null;
  let value = 0;
  for (let i = 1; i < len; i += 1) {
    const tr = Math.max(d.high[i]! - d.low[i]!, Math.abs(d.high[i]! - d.close[i - 1]!), Math.abs(d.low[i]! - d.close[i - 1]!));
    value = i <= n ? value + tr / n : (value * (n - 1) + tr) / n;
  }
  return value;
}

export interface Level {
  label: string;
  price: number;
}

/** High and low of the days opening in [from, to), if any are held. */
function range(d: DailyBars, from: number, to: number): { high: number; low: number } | null {
  let high = -Infinity;
  let low = Infinity;
  for (let i = 0; i < d.time.length; i += 1) {
    if (d.time[i]! < from || d.time[i]! >= to) continue;
    high = Math.max(high, d.high[i]!);
    low = Math.min(low, d.low[i]!);
  }
  return high >= low ? { high, low } : null;
}

/** The prior day's, week's (Monday to Sunday) and calendar month's highs and lows. */
export function priorLevels(d: DailyBars, today: number): Level[] {
  const date = new Date(today);
  const monday = today - ((date.getUTCDay() + 6) % 7) * DAY;
  const month = Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), 1);
  const lastMonth = Date.UTC(date.getUTCFullYear(), date.getUTCMonth() - 1, 1);
  const out: Level[] = [];
  for (const [name, from, to] of [['day', today - DAY, today], ['week', monday - 7 * DAY, monday], ['month', lastMonth, month]] as const) {
    const r = range(d, from, to);
    if (r) out.push({ label: `Prior ${name} high`, price: r.high }, { label: `Prior ${name} low`, price: r.low });
  }
  return out;
}
