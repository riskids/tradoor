/// <reference lib="deno.ns" />
import { assertEquals } from 'jsr:@std/assert@1';
import type { Drawing } from './api.ts';
import { before, DrawingHistory } from './drawing-history.ts';

const drawing = (id: number): Drawing => ({ id, market: 'bybit-perp:BTCUSDT', timeframe: '1h', kind: 'trendline', points: [], style: { color: '#fff' }, label: null, createdAt: 0 });

Deno.test('undo and redo walk the steps back and forth, and a new step drops the redo', () => {
  const h = new DrawingHistory();
  h.record({ kind: 'create', ref: h.ref(1), drawing: drawing(1) });
  h.record({ kind: 'delete', ref: h.ref(1), drawing: drawing(1) });
  assertEquals(h.takeUndo()?.kind, 'delete');
  assertEquals(h.takeUndo()?.kind, 'create');
  assertEquals(h.takeUndo(), undefined);
  assertEquals(h.takeRedo()?.kind, 'create');
  h.record({ kind: 'change', ref: h.ref(1), before: {}, after: {} });
  assertEquals(h.canRedo, false, 'the undone delete is gone');
});

Deno.test('a drawing saved again keeps its steps', () => {
  const h = new DrawingHistory();
  const ref = h.ref(-1);
  h.record({ kind: 'create', ref, drawing: drawing(-1) });
  h.rekey(ref, 40);
  assertEquals(h.ref(40), ref, 'the saved id finds the same ref');
  h.rekey(ref, 41);
  assertEquals((h.takeUndo() as { ref: { id: number } }).ref.id, 41);
});

Deno.test('a drawing the server refused is forgotten', () => {
  const h = new DrawingHistory();
  const ref = h.ref(-2);
  h.record({ kind: 'create', ref, drawing: drawing(-2) });
  h.forget(ref);
  assertEquals(h.canUndo, false);
});

Deno.test('before keeps only what a change overwrites', () => {
  const d = drawing(3);
  assertEquals(before(d, { style: { color: '#000' } }), { style: { color: '#fff' } });
  assertEquals(before(d, { label: 'x', points: [{ time: 1, price: 2 }] }), { label: null, points: [] });
});
