/** A chart's last traded price, and which way it last moved. */
export interface LastPrice {
  /** The market it was traded on: a chart switched to another has none yet. */
  market: string;
  price: number;
  precision: number;
  /** Up 1, down -1, held through unchanged prints; 0 until the price has moved. */
  direction: -1 | 0 | 1;
}

export function nextPrice(before: LastPrice | undefined, market: string, price: number, precision: number): LastPrice {
  if (!before || before.market !== market || !Number.isFinite(before.price) || !Number.isFinite(price)) {
    return { market, price, precision, direction: 0 };
  }
  const direction = price > before.price ? 1 : price < before.price ? -1 : before.direction;
  return { market, price, precision, direction };
}
