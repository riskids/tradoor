/// <reference lib="deno.ns" />
import { assert } from 'jsr:@std/assert@1';
import { CHART_EXTRAS, CHART_THEME, THEME_PRESETS } from './theme.ts';

Deno.test('a preset sets only keys the engine takes, to values it can read', () => {
  const known = new Set([...CHART_THEME, ...CHART_EXTRAS].map((t) => t.name));
  for (const p of THEME_PRESETS) {
    for (const [key, value] of Object.entries(p.theme)) {
      assert(known.has(key), `${p.label}: ${key} is not a chart key`);
      assert(key.endsWith('Alpha') ? Number(value) >= 0 && Number(value) <= 255 : /^#[0-9a-f]{6}$/.test(value), `${p.label}: ${key} = ${value}`);
    }
  }
});
