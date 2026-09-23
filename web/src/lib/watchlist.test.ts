/// <reference lib="deno.ns" />
import { assertEquals } from 'jsr:@std/assert@1';
import { blockOf, freshName, insertMarket, move, sectionEnd, visible } from './watchlist.ts';

const list = ['A', '#Main', 'B', 'C', '#Watch', 'D'];

Deno.test('a section moves with its markets', () => {
  assertEquals(blockOf(list, 1), [1, 4]);
  assertEquals(blockOf(list, 2), [2, 3]);
  assertEquals(move(list, 4, 1), ['A', '#Watch', 'D', '#Main', 'B', 'C']);
  assertEquals(move(list, 1, 6), ['A', '#Watch', 'D', '#Main', 'B', 'C']);
});

Deno.test('a market moves anywhere, a section only between sections', () => {
  assertEquals(move(list, 5, 2), ['A', '#Main', 'D', 'B', 'C', '#Watch']);
  assertEquals(move(list, 0, 6), ['#Main', 'B', 'C', '#Watch', 'D', 'A']);
  assertEquals(move(list, 4, 3), ['A', '#Main', 'B', 'C', '#Watch', 'D'], 'dropped inside Main, it lands after it');
  assertEquals(move(list, 2, 3), list, 'onto its own place: nothing moves');
});

Deno.test('names, inserts and what shows', () => {
  assertEquals(freshName(list), 'Section');
  assertEquals(freshName(['#Section', '#Section 2']), 'Section 3');
  assertEquals(insertMarket(list, 'E', sectionEnd(list, 1)), ['A', '#Main', 'B', 'C', 'E', '#Watch', 'D']);
  assertEquals(insertMarket(list, 'D', 1), ['A', 'D', '#Main', 'B', 'C', '#Watch']);
  assertEquals(visible(list, ['Main']), [0, 1, 4, 5]);
});
