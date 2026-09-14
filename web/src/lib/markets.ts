/** A market is `venue:SYMBOL` — the same spelling the server, the data
 *  directory and the drawings table use. BTCUSDT exists on four venues at four
 *  prices, so the venue is part of the identity. */

export type Venue = 'binance-spot' | 'binance-perp' | 'bybit-spot' | 'bybit-perp';

export const VENUES: readonly Venue[] = ['binance-spot', 'binance-perp', 'bybit-spot', 'bybit-perp'];

export const VENUE_LABEL: Record<Venue, string> = {
  'binance-spot': 'Binance',
  'binance-perp': 'Binance Perp',
  'bybit-spot': 'Bybit',
  'bybit-perp': 'Bybit Perp',
};

export interface MarketRef {
  venue: Venue;
  symbol: string;
}

const MARKET = /^(binance|bybit)-(spot|perp):([A-Z0-9_]+)$/;

export function parseMarket(ref: string): MarketRef | null {
  const m = MARKET.exec(ref);
  return m ? { venue: `${m[1]}-${m[2]}` as Venue, symbol: m[3]! } : null;
}

export const isMarket = (v: unknown): v is string => typeof v === 'string' && MARKET.test(v);

export const symbolOf = (ref: string): string => parseMarket(ref)?.symbol ?? ref;

export function venueLabel(ref: string): string {
  const m = parseMarket(ref);
  return m ? VENUE_LABEL[m.venue] : '';
}

export const isPerp = (ref: string): boolean => parseMarket(ref)?.venue.endsWith('perp') ?? false;

export const DEFAULT_MARKET = 'binance-perp:BTCUSDT';
