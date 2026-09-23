/// <reference lib="deno.ns" />
import { assertEquals } from 'jsr:@std/assert@1';
import { alertTitle, fires } from './alert-rules.ts';

Deno.test('a cross needs a price on each side of the level', () => {
  assertEquals(fires('crossing', 100, null, 101), false, 'the first price only arms it');
  assertEquals(fires('crossing', 100, 99, 100), true, 'reaching the level counts');
  assertEquals(fires('crossing', 100, 101, 99.5), true, 'either way');
  assertEquals(fires('crossing', 100, 101, 100.5), false);
  assertEquals(fires('crossing_up', 100, 101, 99), false, 'down is not up');
  assertEquals(fires('crossing_up', 100, 99, 101), true);
  assertEquals(fires('crossing_down', 100, 99, 101), false);
  assertEquals(fires('crossing_down', 100, 100.2, 100), true);
});

Deno.test('greater and less fire on any price past the level', () => {
  assertEquals(fires('greater', 100, null, 100.1), true);
  assertEquals(fires('greater', 100, null, 100), false);
  assertEquals(fires('less', 100, 101, 99), true);
});

Deno.test('named as TradingView names it', () => {
  assertEquals(alertTitle({ market: 'bybit-perp:ETHFIUSDT', condition: 'crossing', price: 0.6223 }, 4), 'ETHFIUSDT.P Crossing 0.6223');
  assertEquals(alertTitle({ market: 'binance-spot:BTCUSDT', condition: 'greater', price: 75547.8 }, 1), 'BTCUSDT Greater Than 75,547.8');
});
