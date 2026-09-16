/// <reference lib="deno.ns" />
import { assertEquals } from 'jsr:@std/assert@1';
import { TPO_SECTIONS } from './tpo-schema.ts';
import { serializeTpo, TPO_DEFAULTS, tpoValue, withTpoValue } from './tpo-settings.ts';

Deno.test('the sections are laid out as Exocharts lays them out', () => {
  assertEquals(
    TPO_SECTIONS.map((s) => s.title),
    ['Main', 'Colors', 'POC, VA', 'IB, Stats', 'Single prints, Tails'],
  );
});

Deno.test('a chart keeps only what differs from the defaults', () => {
  let s = withTpoValue({}, 'letters', '1');
  s = withTpoValue(s, 'vaPct', '68');
  assertEquals(s, { letters: '1', vaPct: '68' });
  assertEquals(withTpoValue(s, 'vaPct', '70'), { letters: '1' }, 'back to the default is no setting at all');
  assertEquals(tpoValue({}, 'openMarker'), '1');
  assertEquals(tpoValue(s, 'letters'), '1');
});

Deno.test('the engine is sent known keys only', () => {
  assertEquals(serializeTpo({ letters: '1', sellTailMin: '3', bogus: 'x', vaColor: 'bad;value' }), 'letters=1;sellTailMin=3');
});

Deno.test('every field in the dialog has a default, and every default a field', () => {
  const fields = TPO_SECTIONS.flatMap((s) => s.fields.map((f) => f.key)).sort();
  assertEquals(fields, Object.keys(TPO_DEFAULTS).sort());
});
