/**
 * The current UTC day of a market, live: its daily candle from the exchange's
 * stream. What the watchlist and the values panel read change and volume from,
 * as TradingView does — since 00:00 UTC, not over a rolling 24 hours.
 */
import { createStore } from 'solid-js/store';
import { StreamHub } from '../engine/streams';
import { parseMarket } from './markets';
import { nativeOf } from './native';

export interface Day {
  openTime: number;
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
}

/** Tickers are few and slow, so the page keeps its own sockets off the engine thread. */
const hub = new StreamHub();

/**
 * The last day seen for each market, kept for the page. A panel that mounts
 * afresh — a tab switched away from and back — reads these rather than showing
 * dashes until the next trade, which on a quiet market is a long wait.
 */
const [days, setDays] = createStore<Record<string, Day>>({});

export const dayOf = (market: string): Day | undefined => days[market];

/** Calls `onDay` with each update of today's candle; returns the unsubscribe. */
export function watchDay(market: string, onDay: (day: Day) => void): () => void {
  const ref = parseMarket(market);
  if (!ref) return () => {};
  let off: (() => void) | null = null;
  let live = true;
  let latest = 0;
  // The exchange's own name for it, which may take the catalogue to know.
  void nativeOf(market).then((symbol) => {
    if (!live) return;
    off = hub.subscribe(
      ref.venue,
      { kind: 'kline', symbol, interval: '1d' },
      {
        message: (msg) => {
          // A late update of yesterday's candle must not replace today's.
          if (msg.kind !== 'kline' || !(msg.open > 0) || msg.openTime < latest) return;
          latest = msg.openTime;
          const day = { openTime: msg.openTime, open: msg.open, high: msg.high, low: msg.low, close: msg.close, volume: msg.volume };
          setDays(market, day);
          onDay(day);
        },
        state: () => {},
      },
    );
  }, () => {});
  return () => {
    live = false;
    off?.();
  };
}
