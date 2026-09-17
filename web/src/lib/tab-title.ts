/**
 * The browser tab's title: the chart in use, its last price and which way that
 * last moved, the way TradingView writes it — `BTCUSDT.P 76,551.8 ▲`, `.P`
 * marking a perpetual — so the price can be read from another tab.
 */
import { decimalsOf, formatPrice, MAX_PRICE_DECIMALS } from './format';
import type { LastPrice } from './last-price';
import { isPerp, symbolOf } from './markets';

export const APP_TITLE = 'tradoor';

const ARROW = { [1]: ' ▲', [0]: '', [-1]: ' ▼' } as const;

export function tabTitle(market: string | null, last: LastPrice | undefined): string {
  if (!market) return APP_TITLE;
  const name = isPerp(market) ? `${symbolOf(market)}.P` : symbolOf(market);
  // A price from before the chart switched market belongs to the old one.
  if (!last || last.market !== market || !Number.isFinite(last.price)) return name;
  const digits = Math.min(MAX_PRICE_DECIMALS, Math.max(last.precision, decimalsOf(last.price)));
  return `${name} ${formatPrice(last.price, digits)}${ARROW[last.direction]}`;
}
