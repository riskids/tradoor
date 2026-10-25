/// <reference lib="deno.ns" />
import { assertEquals } from 'jsr:@std/assert@1';
import { remapStudyStyles, serializeStudyStyles } from './model.ts';

Deno.test('study styles are serialized key=value, one per line', () => {
  assertEquals(serializeStudyStyles({}), '');
  assertEquals(serializeStudyStyles({ 'ema:50/ema': '#ff0000,2,0' }), 'ema:50/ema=#ff0000,2,0');
  assertEquals(
    serializeStudyStyles({ 'ema:50/ema': '#ff0000,2,0', 'dvwap:1:2:3/u1': ',,1' }),
    'ema:50/ema=#ff0000,2,0\ndvwap:1:2:3/u1=,,1',
  );
});

Deno.test('remapStudyStyles renames keys of the old study id and leaves the rest', () => {
  const styles = { 'ema:50/ema': '#ff0000,2,0', 'sma:50/sma': '#00ff00,,0', 'ema:50plus/ema': '#0000ff,,0' };
  assertEquals(remapStudyStyles(styles, 'ema:50', 'ema:100'), {
    'ema:100/ema': '#ff0000,2,0',
    'sma:50/sma': '#00ff00,,0',
    'ema:50plus/ema': '#0000ff,,0',
  });
});
