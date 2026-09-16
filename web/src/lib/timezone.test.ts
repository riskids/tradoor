/// <reference lib="deno.ns" />
import { assertEquals } from 'jsr:@std/assert@1';
import { formatClock, formatDateTime, offsetLabel, offsetMinutes } from './timezone.ts';

// The module reads the chosen zone from the workspace; these tests pass zones in.

const at = Date.UTC(2026, 8, 16, 10, 19, 5); // 2026-09-16 10:19:05 UTC

Deno.test('offsets are minutes east of UTC, daylight saving included', () => {
  assertEquals(offsetMinutes('UTC', at), 0);
  assertEquals(offsetMinutes('Asia/Jakarta', at), 7 * 60);
  assertEquals(offsetMinutes('Asia/Kolkata', at), 5 * 60 + 30);
  assertEquals(offsetMinutes('America/New_York', at), -4 * 60, 'summer time in September');
  assertEquals(offsetMinutes('America/New_York', Date.UTC(2026, 0, 16)), -5 * 60, 'standard time in January');
});

Deno.test('times are written in the zone chosen', () => {
  assertEquals(formatClock(at, 'Asia/Jakarta'), '17:19:05');
  assertEquals(formatClock(at, 'UTC'), '10:19:05');
  assertEquals(formatDateTime(at, 'Asia/Kolkata'), '2026-09-16 15:49 UTC+5:30');
  assertEquals([offsetLabel(0), offsetLabel(420), offsetLabel(-210)], ['UTC', 'UTC+7', 'UTC-3:30']);
});
