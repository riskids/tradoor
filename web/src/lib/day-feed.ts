/**
 * The current UTC day of a market, live: its daily candle from the exchange's
 * stream. What the watchlist and the values panel read change and volume from,
 * as TradingView does — since 00:00 UTC, not over a rolling 24 hours.
 */
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
          onDay({ openTime: msg.openTime, open: msg.open, high: msg.high, low: msg.low, close: msg.close, volume: msg.volume });
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
