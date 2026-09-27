/**
 * Volume by price, as the server recorded it, folded into the bars on screen.
 *
 * The server keeps one minute at a time at the finest level it can — a dollar
 * on BTC, a tenth on ETH. A chart showing five-minute bars wants five of those
 * minutes added together, and a chart zoomed out wants the levels added too:
 * forty rows in a bar three pixels wide is a smudge, not a reading. Both are
 * sums, which is why the fine grain is what is stored.
 */
import { api } from './api';

/** A bar's levels: price, coins bought, coins sold — lowest price first. */
export interface Levels {
  time: number;
  levels: [number, number, number][];
}

/** The bars in a range, and the price one row of them covers. */
export interface Footprint {
  bars: Levels[];
  /** The same for every bar: a row must read straight across the screen. */
  grid: number;
}

/** Rows a bar is worth drawing at most; past this the levels are added up.
 *  Thirty of them at a chart's height is a row of some ten pixels, which is
 *  what it takes for the numbers in a cell to be read at all. */
const MAX_ROWS = 30;

/** What the reader chose: how many ticks a row covers, and in what unit. */
export interface Settings {
  /** Ticks a row covers, or 0 to fit the rows to the bar. */
  ticks: number;
  /** `coins` as the exchange counts them, or `usd` at the level's own price. */
  unit: 'coins' | 'usd';
}

/** The markets this server records, asked for once. */
let recorded: Promise<Map<string, number>> | null = null;
export function recordedMarkets(): Promise<Map<string, number>> {
  recorded ??= api
    .footprintMarkets()
    .then((r) => new Map(r.markets.map((m) => [m.market, m.tick])))
    .catch(() => new Map<string, number>());
  return recorded;
}

/** Minutes one request may carry; a longer range is asked for in pages. */
const PAGE_MINUTES = 1_440;
/** Pages one fetch will ask for: a week of minutes, which is plenty of screen. */
const MAX_PAGES = 7;

/**
 * The minutes between `from` and `to`, folded into bars of `barMs` and grouped
 * onto one price grid — the same for every bar, so a level reads straight
 * across the screen rather than each bar keeping a grid of its own.
 *
 * The server hands out a day of minutes at a time, so a longer range is asked
 * for in pages: a chart of fifteen-minute bars covers a day in a screenful.
 */
export async function fetchFootprint(market: string, barMs: number, from: number, to: number, settings: Settings = { ticks: 0, unit: 'coins' }): Promise<Footprint> {
  const pages: Promise<{ tick: number; bars: { at: number; levels: [number, number, number][] }[] }>[] = [];
  const page = PAGE_MINUTES * 60_000;
  for (let start = Math.max(from, to - MAX_PAGES * page); start < to; start += page) {
    pages.push(api.footprint(market, start, Math.min(start + page - 1, to)));
  }
  const answers = await Promise.all(pages);
  const tick = answers.find((a) => a.tick > 0)?.tick ?? 1;
  const bars = answers.flatMap((a) => a.bars);
  const byBar = new Map<number, Map<number, [number, number]>>();
  for (const bar of bars) {
    const at = Math.floor(bar.at / barMs) * barMs;
    let levels = byBar.get(at);
    if (!levels) {
      levels = new Map();
      byBar.set(at, levels);
    }
    for (const [price, buy, sell] of bar.levels) {
      const step = Math.round(price / tick);
      const held = levels.get(step);
      if (held) {
        held[0] += buy;
        held[1] += sell;
      } else {
        levels.set(step, [buy, sell]);
      }
    }
  }
  // One grouping for the lot, taken from a bar in the middle of the pack
  // rather than the wildest one: a single violent minute in the day would
  // otherwise set a grid so coarse that every quiet bar collapses into one row
  // and says nothing at all.
  const spans = [...byBar.values()]
    .map((levels) => {
      const steps = [...levels.keys()];
      return steps.length > 0 ? Math.max(...steps) - Math.min(...steps) + 1 : 0;
    })
    .sort((a, b) => a - b);
  const typical = spans.length > 0 ? spans[Math.floor(spans.length * 0.8)]! : 0;
  let by = Math.max(1, Math.round(settings.ticks) || 1);
  while (typical / by > MAX_ROWS) by = coarser(by);
  return {
    grid: by * tick,
    bars: [...byBar.entries()]
      .sort((a, b) => a[0] - b[0])
      .map(([time, levels]) => ({ time, levels: group(levels, tick, by, settings) })),
  };
}

/** Steps to prices, adding neighbouring levels together onto the shared grid. */
function group(levels: Map<number, [number, number]>, tick: number, by: number, settings: Settings): [number, number, number][] {
  const rounded = Math.round(by);
  const out = new Map<number, [number, number]>();
  for (const [step, [buy, sell]] of levels) {
    const key = Math.floor(step / rounded) * rounded;
    const held = out.get(key);
    if (held) {
      held[0] += buy;
      held[1] += sell;
    } else {
      out.set(key, [buy, sell]);
    }
  }
  return [...out.entries()]
    .sort((a, b) => a[0] - b[0])
    .map(([step, [buy, sell]]) => {
      const price = step * tick;
      // In dollars, a coin at this level is worth what it traded at here.
      const worth = settings.unit === 'usd' ? price : 1;
      return [price, buy * worth, sell * worth] as [number, number, number];
    });
}

/** The next grouping up: 1, 2, 5, 10, 20, 50, and so on. */
function coarser(by: number): number {
  const power = 10 ** Math.floor(Math.log10(by));
  const head = by / power;
  return (head < 2 ? 2 : head < 5 ? 5 : 10) * power;
}

/** The flat array the engine reads: per bar, its time, its row count, its rows. */
export function flatten(bars: Levels[]): Float64Array {
  const flat: number[] = [];
  for (const bar of bars) {
    flat.push(bar.time, bar.levels.length);
    for (const [price, buy, sell] of bar.levels) flat.push(price, buy, sell);
  }
  return Float64Array.from(flat);
}
