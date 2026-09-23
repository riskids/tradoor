/** Bar lengths each exchange serves, shared by the REST loader and the streams. */
import type { Venue } from '../lib/markets';

/**
 * Each exchange's name for a bar length, where it has one. A length it lacks
 * is folded from a shorter one it has (see `restInterval`). Weekly bars are
 * taken only where they start on Monday, as the chart's weeks do — Kraken's
 * and Hyperliquid's start on Thursday, so theirs are folded from days.
 */
export const INTERVALS: Partial<Record<Venue, Record<string, string>>> = {
  'okx-spot': { '1m': '1m', '5m': '5m', '15m': '15m', '30m': '30m', '1h': '1H', '2h': '2H', '4h': '4H', '6h': '6Hutc', '12h': '12Hutc', '1d': '1Dutc', '1w': '1Wutc', '1M': '1Mutc' },
  'bitget-spot': { '1m': '1min', '5m': '5min', '15m': '15min', '30m': '30min', '1h': '1h', '4h': '4h', '6h': '6Hutc', '12h': '12Hutc', '1d': '1Dutc', '1w': '1Wutc', '1M': '1Mutc' },
  'bitget-perp': { '1m': '1m', '5m': '5m', '15m': '15m', '30m': '30m', '1h': '1H', '4h': '4H', '6h': '6Hutc', '12h': '12Hutc', '1d': '1Dutc', '1w': '1Wutc', '1M': '1Mutc' },
  'hyperliquid-perp': { '1m': '1m', '5m': '5m', '15m': '15m', '30m': '30m', '1h': '1h', '2h': '2h', '4h': '4h', '12h': '12h', '1d': '1d', '1M': '1M' },
  'kraken-spot': { '1m': '1', '5m': '5', '15m': '15', '30m': '30', '1h': '60', '4h': '240', '1d': '1440' },
  'kraken-perp': { '1m': '1m', '5m': '5m', '15m': '15m', '30m': '30m', '1h': '1h', '4h': '4h', '12h': '12h', '1d': '1d' },
  'lighter-perp': { '1m': '1m', '5m': '5m', '15m': '15m', '30m': '30m', '1h': '1h', '4h': '4h', '12h': '12h', '1d': '1d' },
};
INTERVALS['okx-perp'] = INTERVALS['okx-spot'];

/** Shorter lengths a missing one is folded from, best first; each divides it on UTC boundaries. */
const FALLBACK: Record<string, string[]> = { '2h': ['1h'], '6h': ['2h', '1h'], '12h': ['4h', '2h', '1h'], '1w': ['1d'], '1M': ['1d'] };

/** The length to ask `venue` for when the chart wants `interval`. */
export function restInterval(venue: Venue, interval: string): string {
  const names = INTERVALS[venue];
  if (!names || names[interval]) return interval;
  return FALLBACK[interval]?.find((i) => names[i]) ?? interval;
}
