/**
 * The order of the symbol picker's list: what a query matches, how much each
 * coin trades on every exchange, and the day's movers. Kept apart from the
 * picker so it can be tested without a page.
 */
import type { Product } from './api';

export type Kind = 'all' | 'perp' | 'spot';
/** The list's order: the coins most traded, or the day's biggest moves up or down. */
export type Sort = 'popular' | 'gainers' | 'losers';
/** A mover that trades less than this a day is noise, not news. */
const MOVER_MIN_USD = 1_000_000;

export const DOLLARS = new Set(['USDT', 'USDC', 'USD', 'FDUSD', 'BUSD']);
/** Coins pegged to a currency: USDC/USDT trades billions and is no chart to browse — it is found when typed. */
const STABLE = new Set([...DOLLARS, 'TUSD', 'DAI', 'USDE', 'USDS', 'USD1', 'PYUSD', 'RLUSD', 'USDP', 'BFUSD', 'XUSD', 'EUR', 'EURI', 'EURC']);

/** A coin under every name it trades as: Binance's 1000SHIB is Bybit's SHIB1000. */
const coinOf = (base: string): string => base.replace(/^1(?:000)+(?=[A-Z])/, '').replace(/(?<=[A-Z])1(?:000)+$/, '');

/** Each coin's dollar turnover a day, every exchange and kind together — how much it is traded, not where. */
export function popularity(products: readonly Product[]): Map<string, number> {
  const out = new Map<string, number>();
  for (const p of products) {
    if (!DOLLARS.has(p.quote) || !p.quoteVolume24h) continue;
    const coin = coinOf(p.base);
    out.set(coin, (out.get(coin) ?? 0) + p.quoteVolume24h);
  }
  return out;
}

/** One market a coin: its most traded dollar market, where it has one. */
function perCoin(list: Product[]): Product[] {
  const best = new Map<string, Product>();
  const weight = (p: Product) => (DOLLARS.has(p.quote) ? 1e15 : 0) + (p.quoteVolume24h ?? 0);
  for (const p of list) {
    const coin = coinOf(p.base);
    const held = best.get(coin);
    if (!held || weight(p) > weight(held)) best.set(coin, p);
  }
  return [...best.values()];
}

/**
 * Exact symbol, then base, then prefixes, then substrings; ties by how much the
 * coin trades everywhere, then by the market's own turnover — so "SO" is SOL
 * before SONIC, and BTC's most traded market leads its others. With nothing
 * typed, each coin shows once, the most traded first; or the day's movers.
 * Words like "bybit" or "perp" in the query filter instead of matching, and
 * TradingView's spelling works too: `BYBIT:XPLUSDT.P` is Bybit's perpetual.
 */
export function rank(
  products: readonly Product[],
  query: string,
  kindFilter: Kind,
  exchangeFilter: string | null,
  sort: Sort = 'popular',
  pop: Map<string, number> = new Map(),
  /** The exchanges a word of the query can name: `bybit btc`. */
  exchanges: readonly string[] = [],
): Product[] {
  let exchange: string | null = exchangeFilter;
  let kind = kindFilter;
  const terms: string[] = [];
  for (let word of query.toUpperCase().split(/[\s/:-]+/).filter(Boolean)) {
    if (word.endsWith('.P')) {
      kind = 'perp';
      word = word.slice(0, -2);
    }
    if (exchanges.includes(word.toLowerCase())) exchange = word.toLowerCase();
    else if (word === 'PERP' || word === 'SPOT') kind = word.toLowerCase() as Kind;
    // TradingView's index and other prefixes name no exchange of ours.
    else if (word !== 'CRYPTO' && word !== 'INDEX') terms.push(word.replace(/[^A-Z0-9]/g, ''));
  }
  const needle = terms.join('');
  const scored: [number, number, Product][] = [];
  for (const p of products) {
    if (kind !== 'all' && p.kind !== kind) continue;
    if (exchange && !p.venue.startsWith(exchange)) continue;
    let score = 1;
    if (needle) {
      if (p.symbol === needle) score = 100;
      else if (p.base === needle) score = 90;
      else if (p.symbol.startsWith(needle)) score = 70;
      else if (p.base.startsWith(needle)) score = 60;
      else if (p.symbol.includes(needle)) score = 30;
      else continue;
    }
    // Turnover is counted in the quote: a BTC/IDR market's runs to trillions
    // of rupiah, so among equals a dollar-quoted market comes first.
    scored.push([score + (DOLLARS.has(p.quote) ? 0.5 : 0), p.quoteVolume24h ?? 0, p]);
  }
  if (sort !== 'popular') {
    const movers = perCoin(scored.map((s) => s[2]).filter((p) => DOLLARS.has(p.quote) && !STABLE.has(coinOf(p.base)) && (p.quoteVolume24h ?? 0) >= MOVER_MIN_USD && p.changePct !== null));
    const way = sort === 'gainers' ? -1 : 1;
    return movers.sort((a, b) => way * (a.changePct! - b.changePct!)).slice(0, 100);
  }
  const coinPop = (p: Product) => pop.get(coinOf(p.base)) ?? 0;
  if (!needle) {
    return perCoin(scored.map((s) => s[2]).filter((p) => !STABLE.has(coinOf(p.base))))
      .sort((a, b) => coinPop(b) - coinPop(a) || (b.quoteVolume24h ?? 0) - (a.quoteVolume24h ?? 0))
      .slice(0, 100);
  }
  scored.sort((a, b) => b[0] - a[0] || coinPop(b[2]) - coinPop(a[2]) || b[1] - a[1]);
  return scored.slice(0, 100).map((s) => s[2]);
}
