/// <reference lib="deno.ns" />
import { assertEquals } from 'jsr:@std/assert@1';
import { readUsd, shortUsd } from './flow.ts';

Deno.test('a size is written short and read back', () => {
  assertEquals(shortUsd(5_000_000), '5M');
  assertEquals(shortUsd(2_500_000), '2.5M');
  assertEquals(shortUsd(750_000), '750K');
  assertEquals(shortUsd(1_200_000_000), '1.2B');
  assertEquals(shortUsd(400), '400');
  for (const size of [5_000_000, 2_500_000, 750_000, 400]) assertEquals(readUsd(shortUsd(size)), size, `${size} survives the round trip`);
});

Deno.test('the ways a size is typed', () => {
  assertEquals(readUsd('5m'), 5_000_000);
  assertEquals(readUsd(' $2.5M '), 2_500_000);
  assertEquals(readUsd('750,000'), 750_000);
  assertEquals(readUsd('1b'), 1_000_000_000);
  assertEquals(readUsd(''), null);
  assertEquals(readUsd('0'), null, 'a threshold of nothing is not one');
  assertEquals(readUsd('five million'), null);
});
