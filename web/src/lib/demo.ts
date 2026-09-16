/**
 * Development only: liquidations that never happened, so the pings and the tape
 * can be judged without waiting on the market. `?liqdemo=30` invents thirty a
 * minute. Guarded by the build flag, so none of this reaches a built app.
 */
export const demoRate = (): number => {
  if (!import.meta.env.DEV) return 0;
  const n = Number(new URLSearchParams(location.search).get('liqdemo'));
  return Number.isFinite(n) && n > 0 ? Math.min(n, 600) : 0;
};

export interface DemoHit {
  price: number;
  qty: number;
  long: boolean;
}

/**
 * Sizes on a log scale — mostly small hits with the occasional big one, the way
 * they arrive in life — at a price that wanders a little around the last trade.
 */
export function demoHit(last: number): DemoHit {
  const usd = 50 * 10 ** (Math.random() * 4.6);
  const price = last * (1 + (Math.random() - 0.5) * 0.004);
  return { price, qty: usd / price, long: Math.random() < 0.5 };
}

/** `?renderer=2d` draws charts the way a browser without WebGL2 in workers does. */
export const forceFlatRenderer = (): boolean => import.meta.env.DEV && new URLSearchParams(location.search).get('renderer') === '2d';
