/**
 * An exchange's own name for a market. Binance, Bybit and Bitget spell markets
 * as this app does; the others do not — `BTC-USDT-SWAP`, `BTC/USD`, `kPEPE`,
 * a Lighter market number — and the server's catalogue carries their name.
 * Solid-free, so the chart's worker reads it too; each side fetches the
 * catalogue once.
 */
import { parseMarket, type Venue } from './markets';

const SAME: ReadonlySet<Venue> = new Set(['binance-spot', 'binance-perp', 'bybit-spot', 'bybit-perp', 'bitget-spot', 'bitget-perp']);

let table: Promise<Map<string, string>> | null = null;

function load(): Promise<Map<string, string>> {
  table ??= fetch('/api/markets')
    .then((r) => (r.ok ? (r.json() as Promise<{ products: { market: string; native?: string }[] }>) : { products: [] }))
    .then((c) => new Map(c.products.flatMap((p) => (p.native ? [[p.market, p.native] as [string, string]] : []))))
    .catch(() => {
      table = null; // tried again next time
      return new Map<string, string>();
    });
  return table;
}

/** Whether the exchange spells the market as this app does. */
export const sameName = (venue: Venue): boolean => SAME.has(venue);

/** The name to ask the exchange for. Without the catalogue, the app's own. */
export async function nativeOf(market: string): Promise<string> {
  const ref = parseMarket(market);
  if (!ref) return '';
  if (SAME.has(ref.venue)) return ref.symbol;
  return (await load()).get(market) ?? ref.symbol;
}
