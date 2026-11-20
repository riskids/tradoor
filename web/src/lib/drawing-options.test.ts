/// <reference lib="deno.ns" />
import { assertEquals } from 'jsr:@std/assert@1';
import { rememberStyle } from './drawing-options.ts';
import { defaultStyle } from './drawing-style.ts';

// The browser's storage, in memory: nothing here outlives the test.
const store = new Map<string, string>();
Object.defineProperty(globalThis, 'localStorage', {
  configurable: true,
  value: { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v) },
});

Deno.test('a restyled drawing sets what the next one of its kind starts with', () => {
  store.clear();
  rememberStyle('trendline', { color: '#2962ff', width: 1.5 }, { color: '#f23645', width: 1.5 });
  assertEquals(defaultStyle('trendline'), { color: '#f23645', width: 1.5 }, 'only what changed is kept');
  rememberStyle('trendline', { color: '#f23645' }, { width: 3, dash: 'dashed' });
  assertEquals(defaultStyle('trendline'), { color: '#f23645', width: 3, dash: 'dashed' }, 'on top of what was kept before');
  assertEquals(defaultStyle('ray'), { color: '#2962ff', width: 1.5 }, 'each kind keeps its own');
  assertEquals(defaultStyle('brush'), {}, 'a kind nobody restyled stays as it was');
});

Deno.test('only colour, width and dash are remembered', () => {
  store.clear();
  rememberStyle('rect', { fill: '#9c27b033' }, { fill: '#00000033', middle: true, extendRight: true, hideOn: 'm', color: '#00ff00' });
  assertEquals(store.get('tradoor:drawingTemplates'), JSON.stringify({ rect: { color: '#00ff00' } }));
  store.clear();
  rememberStyle('rect', { color: '#9c27b0' }, { color: '#9c27b0', fill: '#00000033' });
  assertEquals(store.size, 0, 'nothing of the three changed: nothing written');
});
