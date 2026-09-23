/**
 * Aggregated open interest for a chart that shows it: the server's summed
 * candles, fetched when the chart or the study's venues change and topped up
 * every half minute. The engine folds the steps into the chart's bars.
 */

/** The server's venues, in the order the study's `venues` bits count them. */
export const OI_VENUES = ['binance-usdt', 'bybit-usdt', 'okx-usdt', 'deribit-usd', 'hyperliquid-usd', 'binance-usd', 'bybit-usd', 'okx-usd'] as const;
export const OI_ALL = (1 << OI_VENUES.length) - 1;

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const REFRESH_MS = 30_000;
/** What the server keeps — hourly past its first month — and the most steps it answers with. */
const KEPT_MS = 400 * 24 * HOUR;
const MAX_STEPS = 19_000;

export interface OiSteps {
  time: Float64Array;
  open: Float64Array;
  high: Float64Array;
  low: Float64Array;
  close: Float64Array;
}

interface Response {
  time: number[];
  open: number[];
  high: number[];
  low: number[];
  close: number[];
}

/** The `oi` study's venue bits, or null when the chart does not show it. */
export function oiVenues(studies: string): number | null {
  for (const token of studies.split(',')) {
    const [kind, venues] = token.trim().split(':');
    if (kind === 'oi') return Number(venues ?? OI_ALL) || OI_ALL;
  }
  return null;
}

/** The coin behind a symbol: `BTCUSDT` → `BTC`, `1000PEPEUSDT` → `1000PEPE`. */
export function coinOf(symbol: string): string {
  return symbol.replace(/(USDT|USDC|FDUSD|BUSD|USD)$/, '') || symbol;
}

/** Steps as long as a bar up to an hour; an hour divides every longer bar. */
export function stepFor(barMs: number): number {
  for (const step of [MINUTE, 5 * MINUTE, 15 * MINUTE, 30 * MINUTE]) if (barMs <= step) return step;
  return HOUR;
}

export class OpenInterestFeed {
  private key = '';
  private steps: Response = { time: [], open: [], high: [], low: [], close: [] };
  private timer: ReturnType<typeof setInterval> | undefined;
  private request = 0;

  constructor(private readonly apply: (steps: OiSteps) => void) {}

  /** Follow `coin` from `firstBar` on, or stop when `venues` is null. */
  follow(coin: string, barMs: number, firstBar: number, venues: number | null): void {
    if (venues === null || !Number.isFinite(firstBar)) {
      this.stop();
      return;
    }
    const step = stepFor(barMs);
    // Floored to the hour, so the moving edge of what is kept does not refetch every refold.
    const edge = Math.max(Date.now() - KEPT_MS, Date.now() - step * MAX_STEPS);
    const from = Math.max(firstBar, Math.ceil(edge / HOUR) * HOUR);
    const list = OI_VENUES.filter((_, i) => venues & (1 << i)).join(',');
    const key = `${coin}|${step}|${list}|${Math.floor(from / step)}`;
    if (key === this.key) return;
    this.stop();
    this.key = key;
    const query = (since: number) => `/api/open-interest?coin=${encodeURIComponent(coin)}&step=${step}&from=${since}&venues=${list}`;
    const load = async (since: number, tail: boolean) => {
      const req = ++this.request;
      try {
        const res = await fetch(query(since));
        if (!res.ok || req !== this.request || key !== this.key) return;
        const got = (await res.json()) as Response;
        this.steps = tail ? merge(this.steps, got) : got;
        this.apply({
          time: Float64Array.from(this.steps.time),
          open: Float64Array.from(this.steps.open),
          high: Float64Array.from(this.steps.high),
          low: Float64Array.from(this.steps.low),
          close: Float64Array.from(this.steps.close),
        });
      } catch {
        // The server away: the study keeps what it had and tries again next time.
      }
    };
    void load(from, false);
    this.timer = setInterval(() => {
      const t = this.steps.time;
      // From the newest step: it is still filling.
      void load(t.length > 0 ? t[t.length - 1]! : from, t.length > 0);
    }, REFRESH_MS);
  }

  stop(): void {
    clearInterval(this.timer);
    this.timer = undefined;
    this.key = '';
    this.request += 1;
    this.steps = { time: [], open: [], high: [], low: [], close: [] };
  }
}

/** `newer` replaces `older` from its first step on. */
export function merge(older: Response, newer: Response): Response {
  if (newer.time.length === 0) return older;
  const cut = older.time.findIndex((t) => t >= newer.time[0]!);
  const keep = cut === -1 ? older.time.length : cut;
  const join = (a: number[], b: number[]) => [...a.slice(0, keep), ...b];
  return { time: join(older.time, newer.time), open: join(older.open, newer.open), high: join(older.high, newer.high), low: join(older.low, newer.low), close: join(older.close, newer.close) };
}
