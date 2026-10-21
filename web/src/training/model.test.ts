/// <reference lib="deno.ns" />
import { assertAlmostEquals, assertEquals } from 'jsr:@std/assert@1';
import { pnl } from '../journal/model.ts';
import { asJournal, liquidationAt, roe, shapes, unrealized, worth, type ClosedTrade, type Position, type State } from './model.ts';

const position = (over: Partial<Position> = {}): Position => ({
  symbol: 'BTCUSDT',
  side: 'long',
  qty: 1,
  entry: 100,
  leverage: 10,
  margin: 10,
  tp: null,
  sl: null,
  liquidation: 90.4,
  openedAt: 0,
  realized: 0,
  fees: 0,
  funding: 0,
  ...over,
});

const state = (over: Partial<State> = {}): State => ({
  run: 1,
  wallet: 1000,
  startedAt: 0,
  start: 100_000,
  leverage: { BTCUSDT: 10, ETHUSDT: 20, SOLUSDT: 20 },
  maxLeverage: 100,
  maker: 0.0002,
  taker: 0.0005,
  mmr: { BTCUSDT: 0.004 },
  positions: [],
  orders: [],
  quotes: {},
  ...over,
});

Deno.test('an open position makes what the price says, and that over its margin', () => {
  assertEquals(unrealized(position(), 110), 10);
  assertEquals(unrealized(position({ side: 'short' }), 110), -10);
  assertEquals(roe(position(), 105), 0.5);
});

Deno.test('liquidation sits where the margin left is the maintenance margin', () => {
  const at = liquidationAt('long', 100, 100, 0.004);
  assertAlmostEquals(1 + (at - 100) - at * 0.004, 0, 1e-9);
  assertAlmostEquals(liquidationAt('short', 100, 100, 0.004), (101 / 1.004), 1e-9);
});

Deno.test('an account is worth its cash and its open positions; its margin and orders are not free', () => {
  const s = state({
    positions: [position()],
    orders: [
      { id: 1, symbol: 'BTCUSDT', side: 'sell', price: 120, qty: 3, tp: null, sl: null, createdAt: 0 },
      { id: 2, symbol: 'ETHUSDT', side: 'buy', price: 10, qty: 20, tp: null, sl: null, createdAt: 0 },
    ],
  });
  const w = worth(s, () => 110);
  // 10 held by the long; the sell closes its coin and opens two more: 240 at 10x; the ETH buy 200 at 20x.
  assertEquals(w, { equity: 1010, unrealized: 10, used: 10 + 24 + 10, available: 956 });
});

const closed = (over: Partial<ClosedTrade> = {}): ClosedTrade => ({
  id: 1,
  symbol: 'BTCUSDT',
  side: 'long',
  qty: 2,
  entry: 100,
  exit: 110,
  leverage: 10,
  margin: 20,
  pnl: 20,
  fees: 0.3,
  funding: -0.1,
  net: 19.6,
  reason: 'tp',
  tp: 110,
  sl: 95,
  openedAt: 1_000,
  closedAt: 61_000,
  ...over,
});

Deno.test("a closed trade reads in the journal's statistics as what it netted", () => {
  assertAlmostEquals(pnl(asJournal(closed()))!, 19.6, 1e-9);
  assertAlmostEquals(pnl(asJournal(closed({ side: 'short', exit: 90, net: 18 })))!, 18, 1e-9);
  assertEquals(asJournal(closed()).strategy, 'Take profit');
});

Deno.test('the chart shows each trade as its box and where it got out, the position running on, and the orders', () => {
  const s = state({ positions: [position({ sl: null, liquidation: 91, tp: null, openedAt: 5 })], orders: [{ id: 3, symbol: 'BTCUSDT', side: 'buy', price: 95, qty: 1, tp: null, sl: null, createdAt: 7 }] });
  const out = shapes(s, [closed(), closed({ symbol: 'ETHUSDT' })], 'BTCUSDT', 100);
  assertEquals(out.map((x) => x.kind), ['long', 'circle', 'long', 'trendline']);
  assertEquals(out[0]!.points.map((p) => p.price), [100, 95, 110]);
  assertEquals(out[2]!.points.map((p) => p.price), [100, 91, 109], 'no stop loss: the liquidation price, the target mirrored');
  assertEquals(out[2]!.style.risk, 9, "the box's quantity is the position's");
});
