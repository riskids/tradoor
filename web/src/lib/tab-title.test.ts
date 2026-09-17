/// <reference lib="deno.ns" />
import { assertEquals } from 'jsr:@std/assert@1';
import { tabTitle } from './tab-title.ts';

Deno.test('the tab names the symbol, a perpetual marked .P, and its last price', () => {
  const perp = 'bybit-perp:BTCUSDT';
  assertEquals(tabTitle(perp, { market: perp, price: 76551.8, precision: 1 }), 'BTCUSDT.P 76,551.8');
  assertEquals(tabTitle('binance-spot:ETHUSDT', { market: 'binance-spot:ETHUSDT', price: 2430, precision: 2 }), 'ETHUSDT 2,430.00');
  assertEquals(tabTitle('binance-perp:PEPEUSDT', { market: 'binance-perp:PEPEUSDT', price: 0.00000285, precision: 7 }), 'PEPEUSDT.P 0.00000285', 'decimals the price carries are kept');
});

Deno.test('without a price of its own the tab names the symbol, and without a chart the app', () => {
  assertEquals(tabTitle('bybit-perp:SOLUSDT', { market: 'bybit-perp:BTCUSDT', price: 76551.8, precision: 1 }), 'SOLUSDT.P', 'the old market price is not shown');
  assertEquals(tabTitle('bybit-perp:SOLUSDT', { market: 'bybit-perp:SOLUSDT', price: Number.NaN, precision: 2 }), 'SOLUSDT.P');
  assertEquals(tabTitle('bybit-perp:SOLUSDT', undefined), 'SOLUSDT.P');
  assertEquals(tabTitle(null, undefined), 'tradoor');
});
