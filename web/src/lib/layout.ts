/**
 * Which named layout this browser tab has open. Resolved once, before the
 * workspace store starts, and constant for the life of the page: switching
 * layouts reloads the tab with another `?layout=`, so tabs never share one.
 * Lives apart from the store so the api module can read it without a cycle.
 */
import { isPerp, symbolOf } from './markets';
import { readPref, writePref } from './prefs';
import { tfLabel } from './timeframes';

export const DEFAULT_LAYOUT = 'default';
export const SESSION_KEY = 'tradoor:layout';
const LAST_KEY = 'lastLayout';

export const isLayoutId = (v: unknown): v is string => typeof v === 'string' && /^[a-z0-9][a-z0-9-]{0,63}$/.test(v);

/** The URL's `?layout=` first, then this tab's last, then this browser's last. */
export function resolveLayoutId(search: string, session: string | null, last: string | null): string {
  const url = new URLSearchParams(search).get('layout');
  return [url, session, last].find(isLayoutId) ?? DEFAULT_LAYOUT;
}

/** The default layout keeps its old key, so a browser's existing workspace is still found. */
export const localKeyFor = (id: string): string => (id === DEFAULT_LAYOUT ? 'workspace-tv5' : `workspace-tv5:${id}`);

/** A new layout's id: its name as a slug, and six random characters so two with one name differ. */
export function layoutIdFor(name: string, random: () => number = Math.random): string {
  const slug = name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 40).replace(/-+$/, '');
  const tail = random().toString(36).slice(2, 8).padEnd(6, '0');
  return slug ? `${slug}-${tail}` : tail;
}

/** `BTCUSDT.P, 240` as TradingView writes a layout's chart: perps end in .P, intraday is minutes. */
export function layoutSummary(market: string, timeframe: string): string {
  const n = Number.parseInt(timeframe, 10);
  const unit = timeframe.slice(-1);
  const tf = unit === 'm' ? String(n) : unit === 'h' ? String(n * 60) : tfLabel(timeframe);
  return `${symbolOf(market)}${isPerp(market) ? '.P' : ''}, ${tf}`.slice(0, 64);
}

const guard = <T>(f: () => T, otherwise: T): T => {
  try {
    return f();
  } catch {
    return otherwise;
  }
};

function resolve(): string {
  const id = resolveLayoutId(
    guard(() => location.search, ''),
    guard(() => sessionStorage.getItem(SESSION_KEY), null),
    readPref(LAST_KEY),
  );
  guard(() => sessionStorage.setItem(SESSION_KEY, id), undefined);
  writePref(LAST_KEY, id);
  // A reload, a copied link and a bookmark all open this layout.
  guard(() => {
    const url = new URL(location.href);
    if (url.searchParams.get('layout') !== id) {
      url.searchParams.set('layout', id);
      // Deno's test types know no `history`; the browser's does.
      const h = (globalThis as unknown as { history: { state: unknown; replaceState(s: unknown, t: string, u: URL): void } }).history;
      h.replaceState(h.state, '', url);
    }
  }, undefined);
  return id;
}

const active = resolve();
export const activeLayoutId = (): string => active;
