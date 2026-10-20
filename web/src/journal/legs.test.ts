/// <reference lib="deno.ns" />
import { assertEquals } from 'jsr:@std/assert@1';
import { fits, fullEntry, parseLegs, runTo, serializeLegs } from './legs.ts';
import { book, openPnl, pnl, type Trade } from './model.ts';

Deno.test('legs read back as they were written, the unreadable dropped', () => {
  const legs = parseLegs('t,110,50|c,95,25|a,97,100|x,1,1|t,112|t,115,0');
  assertEquals(legs, [
    { kind: 'take', price: 110, percent: 50 },
    { kind: 'cut', price: 95, percent: 25 },
    { kind: 'add', price: 97, percent: 100 },
  ]);
  assertEquals(serializeLegs(legs), 't,110,50|c,95,25|a,97,100');
  assertEquals(parseLegs(undefined), []);
});

Deno.test('a leg fits where the prices allow it', () => {
  const at = (kind: 'take' | 'cut' | 'add', price: number) => fits({ kind, price, percent: 50 }, 100, 90, 120);
  assertEquals([at('take', 110), at('take', 95), at('cut', 95), at('cut', 105), at('add', 95), at('add', 110), at('add', 100)], [true, false, true, false, true, true, false]);
});

Deno.test('the target and the stop come to what the legs on the way make of them', () => {
  const legs = parseLegs('t,110,50|a,95,100');
  assertEquals(runTo(100, 120, 2, [], 1), 40);
  assertEquals(runTo(100, 120, 2, legs, 1), 30, 'half taken at 110, half run to 120');
  assertEquals(runTo(100, 90, 2, legs, 1), -30, 'two more coins at 95, all four stopped at 90');
  assertEquals(runTo(100, 90, 1, parseLegs('c,95,50'), -1), 7.5, 'a short gains as it falls');
  assertEquals(fullEntry(100, 2, legs), 97.5);
});

const trade = (over: Partial<Trade>): Trade => ({
  id: 1,
  drawingId: null,
  tracked: true,
  market: 'binance-perp:BTCUSDT',
  side: 'long',
  timeframe: '1h',
  startAt: 0,
  entry: 100,
  stop: 90,
  target: 120,
  risk: 10,
  qty: 1,
  status: 'planned',
  openedAt: null,
  closedAt: null,
  exit: null,
  fees: 0,
  auto: true,
  strategy: '',
  notes: '',
  createdAt: 0,
  updatedAt: 0,
  legs: [],
  fills: [],
  ...over,
});

Deno.test('a trade with fills is weighed by them, against their average entry', () => {
  const fills: Trade['fills'] = [
    { kind: 'entry', price: 100, qty: 1, at: 0 },
    { kind: 'add', price: 96, qty: 1, at: 1 },
    { kind: 'take', price: 110, qty: 1, at: 2, leg: 0 },
  ];
  assertEquals(book(trade({ fills })), { bought: 2, sold: 1, held: 1, avgEntry: 98, realized: 12 });
  assertEquals(openPnl(trade({ status: 'open', fills }), 100), 14, 'twelve taken, two more on the coin held');
  const closed = trade({ status: 'win', exit: 115, fees: 1, fills: [...fills, { kind: 'target', price: 120, qty: 1, at: 3 }] });
  assertEquals(pnl(closed), 33, '12 + 22, a dollar of fees off');
  assertEquals(pnl(trade({ status: 'win', exit: 120 })), 20, 'one that kept no fills, by its exit');
});
