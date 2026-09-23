/// <reference lib="deno.ns" />
import { assertEquals } from 'jsr:@std/assert@1';
import { parseEvent } from './liquidation-feed.ts';

Deno.test('server-sent events are read block by block', () => {
  assertEquals(parseEvent('event: hit\ndata: {"venue":"okx-perp"}'), { event: 'hit', data: '{"venue":"okx-perp"}' });
  assertEquals(parseEvent('data: a\ndata: b'), { event: 'message', data: 'a\nb' });
  assertEquals(parseEvent(': keep-alive'), null);
});
