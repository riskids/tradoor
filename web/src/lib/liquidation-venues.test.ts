/// <reference lib="deno.ns" />
import { assertEquals } from 'jsr:@std/assert@1';
import type { Product } from './api.ts';
import { coinOf, liquidationVenues } from './liquidation-venues.ts';

const product = (market: string, base: string, quote: string): Product => {
  const [venue, symbol] = market.split(':') as [string, string];
  return { market, venue, symbol, base, quote, kind: venue.endsWith('perp') ? 'perp' : 'spot', priceDecimals: null, quoteVolume24h: null, lastPrice: null, changePct: null };
};

// Spellings as the catalogue served them on 2026-09-16.
const CATALOG: Product[] = [
  product('binance-perp:BTCUSDT', 'BTC', 'USDT'),
  product('binance-perp:BTCUSDC', 'BTC', 'USDC'),
  product('bybit-perp:BTCUSDT', 'BTC', 'USDT'),
  product('bybit-perp:BTCPERP', 'BTC', 'USDC'),
  product('binance-spot:BTCUSDT', 'BTC', 'USDT'),
  product('binance-perp:1000SHIBUSDT', '1000SHIB', 'USDT'),
  product('bybit-perp:SHIB1000USDT', 'SHIB1000', 'USDT'),
  product('binance-perp:AINUSDT', 'AIN', 'USDT'),
  product('binance-perp:1INCHUSDT', '1INCH', 'USDT'),
  product('bybit-perp:1INCHUSDT', '1INCH', 'USDT'),
];

Deno.test('a ticker gathers the same coin and quote on every exchange', () => {
  assertEquals(liquidationVenues('binance-perp:BTCUSDT', CATALOG), [
    { venue: 'binance-perp', market: 'binance-perp:BTCUSDT' },
    { venue: 'bybit-perp', market: 'bybit-perp:BTCUSDT' },
  ]);
  // USDC-margined is its own ticker: Bybit spells it BTCPERP.
  assertEquals(liquidationVenues('binance-perp:BTCUSDC', CATALOG)[1], { venue: 'bybit-perp', market: 'bybit-perp:BTCPERP' });
});

Deno.test('a spot chart gets its ticker\'s perpetuals', () => {
  assertEquals(
    liquidationVenues('binance-spot:BTCUSDT', CATALOG).map((v) => v.market),
    ['binance-perp:BTCUSDT', 'bybit-perp:BTCUSDT'],
  );
});

Deno.test('a thousand before the name and a thousand after it are one coin', () => {
  assertEquals(coinOf('1000SHIB'), coinOf('SHIB1000'));
  assertEquals(coinOf('1INCH'), '1INCH', 'a 1 is part of a name, not a multiplier');
  assertEquals(liquidationVenues('binance-perp:1000SHIBUSDT', CATALOG).map((v) => v.market), ['binance-perp:1000SHIBUSDT', 'bybit-perp:SHIB1000USDT']);
  assertEquals(liquidationVenues('bybit-perp:1INCHUSDT', CATALOG).map((v) => v.market), ['binance-perp:1INCHUSDT', 'bybit-perp:1INCHUSDT']);
});

Deno.test('an exchange that does not list the ticker says so', () => {
  assertEquals(liquidationVenues('binance-perp:AINUSDT', CATALOG), [
    { venue: 'binance-perp', market: 'binance-perp:AINUSDT' },
    { venue: 'bybit-perp', market: null },
  ]);
});

Deno.test('without the catalogue the same symbol is tried everywhere', () => {
  assertEquals(liquidationVenues('bybit-perp:ETHUSDT', null).map((v) => v.market), ['binance-perp:ETHUSDT', 'bybit-perp:ETHUSDT']);
  assertEquals(liquidationVenues('not a market', null), []);
});
