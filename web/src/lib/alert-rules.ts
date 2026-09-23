/**
 * When a price alert fires, and how it is named — TradingView's conditions and
 * wording: `ETHFIUSDT.P Crossing 0.6223`.
 */
import type { AlertCondition } from './api';
import { formatPrice } from './format';
import { isPerp, symbolOf } from './markets';

export const CONDITIONS: { value: AlertCondition; label: string }[] = [
  { value: 'crossing', label: 'Crossing' },
  { value: 'crossing_up', label: 'Crossing Up' },
  { value: 'crossing_down', label: 'Crossing Down' },
  { value: 'greater', label: 'Greater Than' },
  { value: 'less', label: 'Less Than' },
];

export const conditionLabel = (c: AlertCondition): string => CONDITIONS.find((x) => x.value === c)?.label ?? c;

/**
 * Whether a move from `prev` to `cur` fires an alert at `level`. A cross needs
 * a price on each side, so the first price seen (`prev` null) only arms it;
 * reaching the level exactly counts, as TradingView counts it.
 */
export function fires(condition: AlertCondition, level: number, prev: number | null, cur: number): boolean {
  switch (condition) {
    case 'greater':
      return cur > level;
    case 'less':
      return cur < level;
    case 'crossing_up':
      return prev !== null && prev < level && cur >= level;
    case 'crossing_down':
      return prev !== null && prev > level && cur <= level;
    case 'crossing':
      return prev !== null && ((prev < level && cur >= level) || (prev > level && cur <= level));
  }
}

/** A market as TradingView names it: `BTCUSDT.P` for a perpetual. */
export const tvName = (market: string): string => (isPerp(market) ? `${symbolOf(market)}.P` : symbolOf(market));

export function alertTitle(a: { market: string; condition: AlertCondition; price: number }, digits?: number): string {
  return `${tvName(a.market)} ${conditionLabel(a.condition)} ${formatPrice(a.price, digits)}`;
}
