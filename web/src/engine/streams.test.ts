/// <reference lib="deno.ns" />
/**
 * The exchange payloads, as they actually arrive, read through the real hub.
 * The socket is faked so the test needs neither network nor a busy market: a
 * quiet hour on Binance is no reason for a parser to go unchecked.
 *
 * Run with `deno task test`; tsc skips this file (see tsconfig exclude).
 */
import { assert, assertEquals } from 'jsr:@std/assert@1';
import { StreamHub, type StreamMsg, type StreamSpec } from './streams.ts';
import type { Venue } from '../lib/markets.ts';

class FakeSocket {
  static opened: FakeSocket[] = [];
  readyState = 1;
  readonly sent: string[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((e: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;

  constructor(readonly url: string) {
    FakeSocket.opened.push(this);
    queueMicrotask(() => this.onopen?.());
  }

  send(data: string): void {
    this.sent.push(data);
  }

  close(): void {
    this.readyState = 3;
  }
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

/** Subscribes through a real hub on a fake socket, and reports what it saw. */
async function listen(venue: Venue, spec: StreamSpec) {
  FakeSocket.opened = [];
  (globalThis as unknown as { WebSocket: unknown }).WebSocket = FakeSocket;
  const seen: StreamMsg[] = [];
  const stop = new StreamHub().subscribe(venue, spec, { message: (m) => seen.push(m), state: () => {} });
  await tick();
  const socket = FakeSocket.opened.at(-1);
  assert(socket, 'the hub opened no socket');
  return {
    url: socket.url,
    frames: socket.sent.join(' '),
    seen,
    deliver: (payload: unknown) => socket.onmessage?.({ data: JSON.stringify(payload) }),
    stop,
  };
}

/** The hub keeps heartbeat and linger timers of its own; they outlive the test. */
const test = (name: string, fn: () => Promise<void>) => Deno.test({ name, fn, sanitizeOps: false, sanitizeResources: false });

// Captured from fstream.binance.com, 2026-09-16.
const FORCE_ORDER = {
  e: 'forceOrder',
  E: 1789531696996,
  o: { s: 'ONUSDT', S: 'BUY', o: 'LIMIT', f: 'IOC', q: '706', p: '0.1781000', ap: '0.1753635', X: 'FILLED', l: '193', z: '706', T: 1789531695983, ps: 'ONUSDT' },
};

test('a Binance forced BUY is a short being liquidated', async () => {
  const s = await listen('binance-perp', { kind: 'liquidations', symbol: 'ONUSDT' });
  assert(s.url.startsWith('wss://fstream.binance.com/'), s.url);
  assert(s.frames.includes('onusdt@forceOrder'), s.frames);

  // 0.1753635 is where it filled; 0.1781 is only the limit it was pushed through.
  s.deliver(FORCE_ORDER);
  assertEquals(s.seen, [{ kind: 'liquidation', time: 1789531695983, price: 0.1753635, qty: 706, long: false }]);

  // An order that has not filled yet has no average price to report.
  s.deliver({ ...FORCE_ORDER, o: { ...FORCE_ORDER.o, ap: '0', X: 'NEW' } });
  assertEquals((s.seen[1] as { price: number }).price, 0.1781, 'the limit price stands in until it fills');

  s.deliver({ ...FORCE_ORDER, o: { ...FORCE_ORDER.o, S: 'SELL' } });
  assertEquals((s.seen[2] as { long: boolean }).long, true, 'a forced SELL closes a long');
  s.stop();
});

test('a Bybit allLiquidation batch reports the position side', async () => {
  const s = await listen('bybit-perp', { kind: 'liquidations', symbol: 'BTCUSDT' });
  assert(s.frames.includes('allLiquidation.BTCUSDT'), s.frames);

  s.deliver({
    topic: 'allLiquidation.BTCUSDT',
    type: 'snapshot',
    ts: 1789531696000,
    data: [
      { T: 1789531695000, s: 'BTCUSDT', S: 'Buy', v: '0.4', p: '75900.5' },
      { T: 1789531695500, s: 'BTCUSDT', S: 'Sell', v: '0.1', p: '75880' },
    ],
  });
  assertEquals(s.seen, [
    { kind: 'liquidation', time: 1789531695000, price: 75900.5, qty: 0.4, long: true },
    { kind: 'liquidation', time: 1789531695500, price: 75880, qty: 0.1, long: false },
  ]);
  s.stop();
});

test('a liquidation topic is not confused with the candle stream', async () => {
  const s = await listen('binance-perp', { kind: 'kline', symbol: 'BTCUSDT', interval: '15m' });
  s.deliver(FORCE_ORDER);
  assertEquals(s.seen, [], 'nobody subscribed to ONUSDT liquidations');

  s.deliver({ e: 'kline', s: 'BTCUSDT', k: { t: 1789531680000, i: '15m', o: '75800', h: '75950', l: '75790', c: '75900', v: '12.5', L: 42, x: false } });
  assertEquals(s.seen, [{ kind: 'kline', openTime: 1789531680000, open: 75800, high: 75950, low: 75790, close: 75900, volume: 12.5, lastTradeId: 42, closed: false }]);
  s.stop();
});

test('one unknown Bybit symbol does not take the rest of the frame with it', async () => {
  FakeSocket.opened = [];
  (globalThis as unknown as { WebSocket: unknown }).WebSocket = FakeSocket;
  const seen: StreamMsg[] = [];
  const hub = new StreamHub();
  const listener = { message: (m: StreamMsg) => seen.push(m), state: () => {} };
  hub.subscribe('bybit-perp', { kind: 'liquidations', symbol: 'NOPEUSDT' }, listener);
  hub.subscribe('bybit-perp', { kind: 'liquidations', symbol: 'BTCUSDT' }, listener);
  await tick();
  const socket = FakeSocket.opened.at(-1);
  assert(socket, 'the hub opened no socket');
  assertEquals(socket.sent.length, 1, 'both topics travel in one frame');

  // Bybit's own words, verbatim: the frame is refused, one topic is named.
  socket.onmessage?.({
    data: JSON.stringify({
      success: false,
      ret_msg: 'error:handler not found,topic:allLiquidation.NOPEUSDT',
      conn_id: 'da7to24d40ndveq8rdsg-66uqh',
      req_id: 'allLiquidation.NOPEUSDT,allLiquidation.BTCUSDT',
      op: 'subscribe',
    }),
  });
  assertEquals(JSON.parse(socket.sent[1] ?? '{}').args, ['allLiquidation.BTCUSDT'], 'the good topic is re-sent alone');

  socket.onmessage?.({ data: JSON.stringify({ topic: 'allLiquidation.BTCUSDT', data: [{ T: 1789531695000, s: 'BTCUSDT', S: 'Buy', v: '0.4', p: '75900.5' }] }) });
  assertEquals(seen.length, 1, 'BTCUSDT still streams');

  // The refused topic is never sent again, alone or in a batch.
  socket.onmessage?.({ data: JSON.stringify({ success: false, ret_msg: 'error:handler not found,topic:allLiquidation.BTCUSDT', req_id: 'allLiquidation.BTCUSDT', op: 'subscribe' }) });
  assertEquals(socket.sent.length, 2, 'a frame of one is not retried');
});
