/**
 * A position's legs as its drawing keeps them: `legs` in its style, each a
 * kind, a price and a percent of the size its entry buys — `t,0.18,50|c,…|a,…`
 * for a partial take profit, a partial cut loss and an added entry. The chart
 * engine draws them (crates/render, `paint_position`) and the server fills
 * them as price reaches them (bins/server journal, `legs_of`); both read the
 * same string and keep only the legs a position's prices allow.
 */
import type { LegKind } from './model';

export interface DrawnLeg {
  kind: LegKind;
  price: number;
  /** Of the entry's size: 50 is half of it. */
  percent: number;
}

const CODE: Record<LegKind, string> = { take: 't', cut: 'c', add: 'a' };
const KIND: Record<string, LegKind> = { t: 'take', c: 'cut', a: 'add' };
export const MAX_LEGS = 12;

export function parseLegs(raw: unknown): DrawnLeg[] {
  if (typeof raw !== 'string' || raw === '') return [];
  return raw
    .split('|')
    .map((part) => {
      const [k, p, pct] = part.split(',').map((f) => f.trim());
      return { kind: KIND[k ?? ''], price: Number(p), percent: Number(pct) };
    })
    .filter((l): l is DrawnLeg => l.kind !== undefined && Number.isFinite(l.price) && l.percent > 0 && l.percent <= 1000)
    .slice(0, MAX_LEGS);
}

export const serializeLegs = (legs: readonly DrawnLeg[]): string => legs.map((l) => `${CODE[l.kind]},${l.price},${l.percent}`).join('|');

const between = (p: number, a: number, b: number) => p > Math.min(a, b) && p < Math.max(a, b);

/** Whether the position's prices allow the leg: a take between the entry and the target, a cut
 *  between the entry and the stop, an add anywhere between the stop and the target but the entry. */
export function fits(l: DrawnLeg, entry: number, stop: number, target: number): boolean {
  if (l.kind === 'take') return between(l.price, entry, target);
  if (l.kind === 'cut') return between(l.price, entry, stop);
  return between(l.price, stop, target) && l.price !== entry;
}

/** What `qty` coins make running from the entry to `end`, the legs on the way filled, against
 *  their average entry: all of it closed by then, so the order they fill in does not count — but
 *  for takes and cuts asking more than was bought, scaled down to it, as the chart engine reckons
 *  it. `dir` is 1 for a long, -1 for a short. */
export function runTo(entry: number, end: number, qty: number, legs: readonly DrawnLeg[], dir: number): number {
  let bought = qty;
  let cost = entry * qty;
  let out = 0;
  let proceeds = 0;
  for (const l of legs.filter((l) => (l.price - entry) * (end - entry) > 0)) {
    const q = (l.percent / 100) * qty;
    if (l.kind === 'add') {
      bought += q;
      cost += l.price * q;
    } else {
      out += q;
      proceeds += l.price * q;
    }
  }
  if (out > bought) {
    proceeds *= bought / out;
    out = bought;
  }
  proceeds += end * (bought - out);
  return (proceeds - cost) * dir;
}

/** The average entry once every add has filled. */
export function fullEntry(entry: number, qty: number, legs: readonly DrawnLeg[]): number {
  const adds = legs.filter((l) => l.kind === 'add');
  const bought = qty + adds.reduce((s, l) => s + (l.percent / 100) * qty, 0);
  return bought > 0 ? (entry * qty + adds.reduce((s, l) => s + l.price * (l.percent / 100) * qty, 0)) / bought : entry;
}
