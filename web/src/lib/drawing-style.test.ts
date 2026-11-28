/// <reference lib="deno.ns" />
import { assertEquals } from 'jsr:@std/assert@1';
import { defaultStyle, engineKind } from './drawing-style.ts';

// The browser's storage, in memory: nothing here outlives the test.
const store = new Map<string, string>();
Object.defineProperty(globalThis, 'localStorage', {
  configurable: true,
  value: { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v) },
});

Deno.test('a preset tool draws its kind with its own look', () => {
  assertEquals(engineKind('trendray'), 'trendline');
  assertEquals(engineKind('vertical'), 'vertical', 'a kind of its own is itself');
  assertEquals(defaultStyle('extended'), { color: '#2962ff', width: 1.5, extendLeft: true, extendRight: true });
  assertEquals(defaultStyle('angle'), { color: '#2962ff', width: 1.5, angle: true });
  assertEquals(defaultStyle('trendline'), { color: '#2962ff', width: 1.5 }, 'the trendline itself is unchanged');
});
