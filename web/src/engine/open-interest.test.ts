/// <reference lib="deno.ns" />
import { assertEquals } from 'jsr:@std/assert@1';
import { coinOf, merge, oiVenues, OI_ALL, stepFor } from './open-interest.ts';

Deno.test('the study, its venues, the coin and the step', () => {
  assertEquals(oiVenues('ema:50,oi:5:1:0'), 5);
  assertEquals(oiVenues('oi'), OI_ALL);
  assertEquals(oiVenues('ema:50'), null);
  assertEquals([coinOf('BTCUSDT'), coinOf('1000PEPEUSDT'), coinOf('ETHUSDC'), coinOf('BTCUSD')], ['BTC', '1000PEPE', 'ETH', 'BTC']);
  assertEquals([stepFor(60_000), stepFor(900_000), stepFor(14_400_000)], [60_000, 900_000, 3_600_000]);
});

Deno.test('a top-up replaces the steps from its first on', () => {
  const r = (time: number[]) => ({ time, open: time, high: time, low: time, close: time });
  assertEquals(merge(r([1, 2, 3]), r([3, 4])).time, [1, 2, 3, 4]);
  assertEquals(merge(r([1, 2]), r([])).time, [1, 2]);
});
