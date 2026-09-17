/// <reference lib="deno.ns" />
import { assertEquals } from 'jsr:@std/assert@1';
import type { LastPrice } from './last-price.ts';
import { tabTitle } from './tab-title.ts';

const at = (market: string, price: number, precision: number, direction: LastPrice['direction'] = 0): LastPrice => ({ market, price, precision, direction });

Deno.test('the tab names the symbol, a perpetual marked .P, and its last price', () => {
  const perp = 'bybit-perp:BTCUSDT';
  assertEquals(tabTitle(perp, at(perp, 76551.8, 1)), 'BTCUSDT.P 76,551.8');
  assertEquals(tabTitle('binance-spot:ETHUSDT', at('binance-spot:ETHUSDT', 2430, 2)), 'ETHUSDT 2,430.00');
  assertEquals(tabTitle('binance-perp:PEPEUSDT', at('binance-perp:PEPEUSDT', 0.00000285, 7)), 'PEPEUSDT.P 0.00000285', 'decimals the price carries are kept');
});

Deno.test('an arrow tells which way the price last moved', () => {
  const perp = 'bybit-perp:BTCUSDT';
  assertEquals(tabTitle(perp, at(perp, 76551.8, 1, 1)), 'BTCUSDT.P 76,551.8 ▲');
  assertEquals(tabTitle(perp, at(perp, 76550.1, 1, -1)), 'BTCUSDT.P 76,550.1 ▼');
});

Deno.test('without a price of its own the tab names the symbol, and without a chart the app', () => {
  assertEquals(tabTitle('bybit-perp:SOLUSDT', at('bybit-perp:BTCUSDT', 76551.8, 1, 1)), 'SOLUSDT.P', 'the old market price is not shown');
  assertEquals(tabTitle('bybit-perp:SOLUSDT', at('bybit-perp:SOLUSDT', Number.NaN, 2)), 'SOLUSDT.P');
  assertEquals(tabTitle('bybit-perp:SOLUSDT', undefined), 'SOLUSDT.P');
  assertEquals(tabTitle(null, undefined), 'tradoor');
});
