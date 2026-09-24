/**
 * A watchlist is one ordered list of items: markets, and section headers
 * written `#Name`. A section holds the markets after it, up to the next one;
 * markets before the first section belong to none. Keeping it one list keeps
 * older saved watchlists valid as they are.
 */

export const isSection = (item: string): boolean => item.startsWith('#');
export const sectionName = (item: string): string => item.slice(1);
export const SECTION_NAME_MAX = 40;

/** Flag colours, in TradingView's order. */
export const FLAG_COLORS: Record<string, string> = { red: '#f23645', blue: '#2962ff', green: '#089981', orange: '#ff9800', purple: '#9c27b0', cyan: '#00bcd4', pink: '#e91e63' };
export const FLAGS = Object.keys(FLAG_COLORS);

/** The half-open range an item moves with: a section takes its markets along. */
export function blockOf(items: readonly string[], i: number): [number, number] {
  if (!isSection(items[i] ?? '')) return [i, i + 1];
  let end = i + 1;
  while (end < items.length && !isSection(items[end]!)) end += 1;
  return [i, end];
}

/** Move the item at `from` — with its section's markets — to before position `to`. */
export function move(items: readonly string[], from: number, to: number): string[] {
  const [start, end] = blockOf(items, from);
  if (to >= start && to <= end) return [...items];
  // A market dropped past the last row of a list must not slip into a section it was not dropped in.
  const block = items.slice(start, end);
  const rest = [...items.slice(0, start), ...items.slice(end)];
  const at = to > end ? to - (end - start) : to;
  // A section lands only between sections, never inside another's markets.
  let target = at;
  if (isSection(block[0]!)) while (target < rest.length && target > 0 && !isSection(rest[target]!)) target += 1;
  rest.splice(target, 0, ...block);
  return rest;
}

/** Where a whole section may land: the top, before another section, and the end. */
export function sectionStops(items: readonly string[]): number[] {
  const stops = new Set<number>([0, items.length]);
  items.forEach((item, i) => {
    if (isSection(item)) stops.add(i);
  });
  return [...stops].sort((a, b) => a - b);
}

/** A section name not yet used here: `Section`, `Section 2`, … */
export function freshName(items: readonly string[], base = 'Section'): string {
  const taken = new Set(items.filter(isSection).map(sectionName));
  if (!taken.has(base)) return base;
  let n = 2;
  while (taken.has(`${base} ${n}`)) n += 1;
  return `${base} ${n}`;
}

/** Add a market at `at` (the end by default), moving it there if it is listed already. */
export function insertMarket(items: readonly string[], market: string, at = items.length): string[] {
  const i = items.indexOf(market);
  if (i === -1) return [...items.slice(0, at), market, ...items.slice(at)];
  return move(items, i, at);
}

/** The end of the section an item sits in — where a market added to it goes. */
export function sectionEnd(items: readonly string[], i: number): number {
  let end = i + 1;
  while (end < items.length && !isSection(items[end]!)) end += 1;
  return end;
}

/** Items shown, by index: a collapsed section hides its markets. */
export function visible(items: readonly string[], collapsed: readonly string[]): number[] {
  const hidden = new Set(collapsed);
  const out: number[] = [];
  let hiding = false;
  items.forEach((item, i) => {
    if (isSection(item)) {
      hiding = hidden.has(sectionName(item));
      out.push(i);
    } else if (!hiding) out.push(i);
  });
  return out;
}
