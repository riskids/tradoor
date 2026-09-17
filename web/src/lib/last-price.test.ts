/// <reference lib="deno.ns" />
import { assertEquals } from 'jsr:@std/assert@1';
import { nextPrice } from './last-price.ts';

Deno.test('a price moves up or down, and holds its way through unchanged prints', () => {
  const m = 'bybit-perp:BTCUSDT';
  let p = nextPrice(undefined, m, 76000, 1);
  assertEquals(p.direction, 0, 'nothing to compare the first price with');
  p = nextPrice(p, m, 76001, 1);
  assertEquals(p.direction, 1);
  p = nextPrice(p, m, 76001, 1);
  assertEquals(p.direction, 1, 'an unchanged print keeps the last move');
  p = nextPrice(p, m, 75990.5, 1);
  assertEquals(p.direction, -1);
});

Deno.test('another market, or no price, starts over', () => {
  const up = nextPrice(nextPrice(undefined, 'bybit-perp:BTCUSDT', 1, 1), 'bybit-perp:BTCUSDT', 2, 1);
  assertEquals(nextPrice(up, 'bybit-perp:SOLUSDT', 3, 2).direction, 0);
  const cleared = nextPrice(up, 'bybit-perp:BTCUSDT', Number.NaN, 1);
  assertEquals(cleared.direction, 0);
  assertEquals(nextPrice(cleared, 'bybit-perp:BTCUSDT', 5, 1).direction, 0);
});
