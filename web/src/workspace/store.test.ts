import { assert } from 'jsr:@std/assert';
import { localIsOurs } from './store.ts';

Deno.test('a layout kept in this browser is only opened with when it is this browser’s', () => {
  assert(localIsOurs('abc123', 'abc123'), 'the copy this browser saved');
  assert(localIsOurs(null, 'abc123'), 'a copy from before the stamp existed is this browser’s own');
  assert(
    !localIsOurs('abc123', 'def456'),
    'pasting another browser’s id asks for their layout: opening with this one would save it over theirs',
  );
});

import { assertEquals } from 'jsr:@std/assert';
import { DEFAULT_LAYOUT, layoutIdFor, layoutSummary, localKeyFor, resolveLayoutId } from '../lib/layout.ts';

Deno.test('layout id: URL, then tab, then browser, then default; bad ids skipped', () => {
  assertEquals(resolveLayoutId('?layout=a', 'b', 'c'), 'a');
  assertEquals(resolveLayoutId('', 'b', 'c'), 'b');
  assertEquals(resolveLayoutId('', null, 'c'), 'c');
  assertEquals(resolveLayoutId('', null, null), DEFAULT_LAYOUT);
  assertEquals(resolveLayoutId('?layout=Bad Id!', 'b', 'c'), 'b');
});

Deno.test('local key: default keeps the old key, others get their own', () => {
  assertEquals(localKeyFor('default'), 'workspace-tv5');
  assertEquals(localKeyFor('htf-white-x1'), 'workspace-tv5:htf-white-x1');
});

Deno.test('summary reads as TradingView writes it', () => {
  assertEquals(layoutSummary('binance-perp:BTCUSDT', '4h'), 'BTCUSDT.P, 240');
  assertEquals(layoutSummary('binance-spot:ETHUSDT', '15m'), 'ETHUSDT, 15');
  assertEquals(layoutSummary('binance-spot:ETHUSDT', '1d'), 'ETHUSDT, D');
  assertEquals(layoutSummary('binance-spot:ETHUSDT', '1w'), 'ETHUSDT, W');
});

Deno.test('generated ids are valid and unique-looking', () => {
  const re = /^[a-z0-9][a-z0-9-]{0,63}$/;
  for (const n of ['HTF White', 'main', '  ', '!!!', 'ü'.repeat(100), 'a'.repeat(100)]) {
    const id = layoutIdFor(n);
    if (!re.test(id)) throw new Error(`bad id ${id} for ${n}`);
  }
  if (!layoutIdFor('HTF White').startsWith('htf-white-')) throw new Error('slug first');
});

import { doc, marketOf, setPanelMarket, setPanelTimeframe, timeframeOf } from './store.ts';

Deno.test('a timeframe is set on one panel alone, though its link group still shares the market', () => {
  const [a, b] = Object.values(doc.panels).filter((p) => p.type === 'chart');
  assert(a && b && a.link && a.link === b.link, 'the default layout links its two charts');
  const before = timeframeOf(b);
  const next = before === '4h' ? '1h' : '4h';
  // Every change schedules a save; nothing is to be written from a test.
  const timeout = globalThis.setTimeout;
  globalThis.setTimeout = (() => 0) as unknown as typeof setTimeout;
  try {
    setPanelTimeframe(a.id, next);
    assertEquals([timeframeOf(a), timeframeOf(b)], [next, before]);
    setPanelMarket(a.id, 'bybit-perp:ETHUSDT');
    assertEquals(marketOf(b), 'bybit-perp:ETHUSDT');
  } finally {
    globalThis.setTimeout = timeout;
  }
});
