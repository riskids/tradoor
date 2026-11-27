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

import { activate, defaultWorkspace, railPick, validate, type SplitNode, type TabsNode } from './model.ts';

Deno.test('the rail folds a side group on its showing icon, and anything asking for a panel opens it', () => {
  const d = defaultWorkspace();
  const side = (d.root as SplitNode).children[2] as TabsNode;
  const [first, second] = side.panels as [string, string];
  railPick(d, side.id, first);
  assertEquals(side.hidden, true);
  // Folded survives a save and a load.
  assertEquals(((validate(JSON.parse(JSON.stringify(d)))!.root as SplitNode).children[2] as TabsNode).hidden, true);
  railPick(d, side.id, first);
  assertEquals(side.hidden, false);
  railPick(d, side.id, second);
  assertEquals([side.active, side.hidden], [second, false]);
  railPick(d, side.id, second);
  railPick(d, side.id, first);
  assertEquals([side.active, side.hidden], [first, false]);
  railPick(d, side.id, first);
  activate(d, side.id, second);
  assertEquals([side.active, side.hidden], [second, false]);
});

Deno.test('a link group shares the market only: a layout saved while it held the timeframe too opens as it showed', () => {
  const d = defaultWorkspace();
  const [a, b] = Object.values(d.panels).filter((p) => p.type === 'chart');
  assertEquals([a!.link, b!.link], ['green', 'green']);
  b!.link = null;
  b!.timeframe = '15m';
  const old = { ...d, links: { ...d.links, green: { market: d.links.green.market, timeframe: '4h' } } };
  const read = validate(JSON.parse(JSON.stringify(old)))!;
  assertEquals(read.panels[a!.id]!.timeframe, '4h', 'a linked panel showed its group’s');
  assertEquals(read.panels[b!.id]!.timeframe, '15m', 'an unlinked one its own');
  assertEquals(read.links.green, { market: d.links.green.market });
  assertEquals(validate(JSON.parse(JSON.stringify(read)))!.panels[a!.id]!.timeframe, '4h');
});
