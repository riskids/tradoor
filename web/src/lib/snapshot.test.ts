/// <reference lib="deno.ns" />
import { assertAlmostEquals, assertEquals } from 'jsr:@std/assert@1';
import type { DailyBars } from '../engine/protocol.ts';
import { atr, DAY, performance, priorLevels } from './snapshot.ts';

/** Days up to (not including) `today`, each closing at 100 + its index, range ±2. */
function days(today: number, n: number): DailyBars {
  const d: DailyBars = { time: [], open: [], high: [], low: [], close: [], precision: 2 };
  for (let i = 0; i < n; i += 1) {
    const c = 100 + i;
    d.time.push(today - (n - i) * DAY);
    d.open.push(c - 1);
    d.high.push(c + 2);
    d.low.push(c - 2);
    d.close.push(c);
  }
  return d;
}

// Wednesday 2026-09-23.
const today = Date.UTC(2026, 8, 23);

Deno.test('returns are against the close that many days before today', () => {
  const d = days(today, 400);
  const p = Object.fromEntries(performance(d, today, 499).map((x) => [x.label, x.pct]));
  // Yesterday closed at 499; seven days before today's open closed at 499 - 6.
  assertAlmostEquals(p['7D']!, ((499 - 493) / 493) * 100);
  assertAlmostEquals(p['1Y']!, ((499 - 135) / 135) * 100);
  const ytdRef = 100 + 400 - Math.round((today - (Date.UTC(2026, 0, 1) - DAY)) / DAY);
  assertAlmostEquals(p['YTD']!, ((499 - ytdRef) / ytdRef) * 100);
  assertEquals(performance(days(today, 20), today, 120).find((x) => x.label === '30D')!.pct, null);
});

Deno.test('ATR is the true range when every day has the same one', () => {
  // Each day spans 4 and gaps 1 from the last close: true range 4 (high - low), nothing larger.
  assertAlmostEquals(atr(days(today, 60))!, 4);
  assertEquals(atr(days(today, 10)), null);
});

Deno.test('prior day, Monday week and calendar month', () => {
  const d = days(today, 60);
  const l = Object.fromEntries(priorLevels(d, today).map((x) => [x.label, x.price]));
  assertEquals(l['Prior day high'], 159 + 2);
  // Last week: Monday the 14th to Sunday the 20th — days 51..57 (today would be day 60).
  assertEquals(l['Prior week high'], 157 + 2);
  assertEquals(l['Prior week low'], 151 - 2);
  // August ends 23 days before today: day 37.
  assertEquals(l['Prior month high'], 137 + 2);
});
