/// <reference lib="deno.ns" />
/**
 * The symbol picker's order, on a few markets made up for it.
 *
 * Run with `deno task test`; tsc skips this file (see tsconfig exclude).
 */
import { assertEquals } from 'jsr:@std/assert@1';
import type { Product } from './api.ts';
import { popularity, rank } from './symbol-rank.ts';

const market = (venue: string, symbol: string, base: string, quote: string, volume: number | null, change: number | null = 0): Product => ({
  market: `${venue}:${symbol}`,
  venue,
  symbol,
  base,
  quote,
  kind: venue.endsWith('perp') ? 'perp' : 'spot',
  priceDecimals: 2,
  quoteVolume24h: volume,
  lastPrice: 1,
  changePct: change,
});

const LIST: Product[] = [
  market('binance-perp', 'BTCUSDT', 'BTC', 'USDT', 900, 1),
  market('bybit-perp', 'BTCUSDT', 'BTC', 'USDT', 400, 1.2),
  market('binance-spot', 'BTCIDR', 'BTC', 'IDR', 5_000_000_000, 1),
  market('binance-perp', 'SOLUSDT', 'SOL', 'USDT', 100, 4),
  market('bybit-perp', 'SOLUSDT', 'SOL', 'USDT', 50, 4.5),
  market('binance-perp', 'SONICUSDT', 'SONIC', 'USDT', 120, -3),
  market('binance-perp', '1000SHIBUSDT', '1000SHIB', 'USDT', 30, 9),
  market('bybit-perp', 'SHIB1000USDT', 'SHIB1000', 'USDT', 20, 8),
  // A stock token: Bitget reports the stock's own turnover, which the server drops.
  market('bitget-spot', 'RSPYUSDT', 'RSPY', 'USDT', null, 0.7),
  // A stablecoin against another: billions a day, and nothing to chart.
  market('binance-spot', 'USDCUSDT', 'USDC', 'USDT', 5000, 0.01),
];
const VOLUMES = new Map(LIST.map((p) => [p.market, p.quoteVolume24h]));
// Movers need a million a day: these are scaled up for that test alone.
const BIG = LIST.map((p) => ({ ...p, quoteVolume24h: p.quoteVolume24h === null ? null : p.quoteVolume24h * 1_000_000 }));
const markets = (list: Product[]) => list.map((p) => p.market);

Deno.test('a coin is traded as the sum of its dollar markets, under every name', () => {
  const pop = popularity(LIST);
  assertEquals(pop.get('BTC'), 1300, 'BTC/IDR is counted in rupiah, not dollars');
  assertEquals(pop.get('SHIB'), 50, "Binance's 1000SHIB and Bybit's SHIB1000 are one coin");
  assertEquals(pop.has('RSPY'), false, 'no turnover of its own');
});

Deno.test('with nothing typed, each coin shows once, the most traded first', () => {
  const rows = rank(LIST, '', 'all', null, 'popular', popularity(LIST));
  assertEquals(markets(rows), ['binance-perp:BTCUSDT', 'binance-perp:SOLUSDT', 'binance-perp:SONICUSDT', 'binance-perp:1000SHIBUSDT', 'bitget-spot:RSPYUSDT']);
  assertEquals(markets(rank(LIST, 'USDC', 'all', null, 'popular', popularity(LIST))), ['binance-spot:USDCUSDT'], 'a stablecoin is found when typed');
});

Deno.test('a prefix puts the more traded coin first, then its busiest market', () => {
  const rows = rank(LIST, 'SO', 'all', null, 'popular', popularity(LIST));
  // SONIC's one market outdoes either of SOL's, but SOL trades more in all.
  assertEquals(markets(rows), ['binance-perp:SOLUSDT', 'bybit-perp:SOLUSDT', 'binance-perp:SONICUSDT']);
});

Deno.test('a coin asked for by name lists every market it has', () => {
  const rows = rank(LIST, 'BTC', 'all', null, 'popular', popularity(LIST));
  assertEquals(markets(rows), ['binance-perp:BTCUSDT', 'bybit-perp:BTCUSDT', 'binance-spot:BTCIDR']);
  assertEquals(VOLUMES.get(rows[0]!.market), 900);
});

Deno.test("the day's movers: dollar markets of a million or more, one per coin", () => {
  const up = rank(BIG, '', 'all', null, 'gainers', popularity(BIG));
  // SHIB at +9% on its busiest market; RSPY has no turnover to count; BTC/IDR is no dollar market.
  assertEquals(markets(up), ['binance-perp:1000SHIBUSDT', 'binance-perp:SOLUSDT', 'binance-perp:BTCUSDT', 'binance-perp:SONICUSDT']);
  const down = rank(BIG, '', 'all', null, 'losers', popularity(BIG));
  assertEquals(down[0]!.market, 'binance-perp:SONICUSDT');
  const quiet = rank(LIST, '', 'all', null, 'gainers', popularity(LIST));
  assertEquals(quiet, [], 'nothing here turns over a million');
});
