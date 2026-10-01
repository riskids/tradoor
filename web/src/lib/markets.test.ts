/// <reference lib="deno.ns" />
import { assert, assertEquals } from 'jsr:@std/assert@1';
import { isChartMarket, isMarket, isRatio, ratioLegs, symbolOf, venueLabel } from './markets.ts';

Deno.test('a ratio is two whole markets, venues and all', () => {
  const xpl = 'bybit-perp:XPLUSDT/binance-spot:BTCUSDT';
  assertEquals(ratioLegs(xpl), ['bybit-perp:XPLUSDT', 'binance-spot:BTCUSDT']);
  assertEquals(symbolOf(xpl), 'XPLUSDT/BTCUSDT');
  assertEquals(venueLabel(xpl), 'Bybit Perp / Binance');
  assert(isRatio(xpl) && isChartMarket(xpl) && !isMarket(xpl), 'a chart can show it; a watchlist row or an alert cannot');

  assertEquals(ratioLegs('binance-perp:BTCUSDT'), null, 'one market is not a ratio');
  assertEquals(ratioLegs('binance-perp:BTCUSDT/binance-perp:BTCUSDT'), null, 'nor is a market over itself');
  assertEquals(ratioLegs('XPLUSDT/BTCUSDT'), null, 'each side names its venue');
  assertEquals(ratioLegs('binance-perp:BTCUSDT/'), null);
  assertEquals(isChartMarket('nowhere'), false);
});
