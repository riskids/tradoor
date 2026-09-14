/** The tradoor server: the market catalogue, drawings and workspaces. Candles
 *  do not come through here — they are files under /data and exchange REST. */
import type { DrawingPoint } from '../engine/protocol';
import { readPref } from './prefs';

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
    this.name = 'ApiError';
  }
}

export const errorText = (e: unknown): string => (e instanceof Error ? e.message : String(e));

/** Stored per browser; a server started with TRADOOR_API_TOKEN needs it for writes. */
export const API_TOKEN_PREF = 'api-token';

async function request<T>(path: string, init: { method?: string; body?: unknown } = {}): Promise<{ status: number; body: T | null }> {
  const method = init.method ?? 'GET';
  const headers: Record<string, string> = {};
  const token = readPref(API_TOKEN_PREF);
  if (token && method !== 'GET') headers.authorization = `Bearer ${token}`;
  if (init.body !== undefined) headers['content-type'] = 'application/json';
  let res: Response;
  try {
    res = await fetch(path, { method, headers, body: init.body === undefined ? undefined : JSON.stringify(init.body) });
  } catch {
    throw new ApiError(0, 'the tradoor server is not reachable');
  }
  const text = await res.text();
  if (!text) return { status: res.status, body: null };
  try {
    return { status: res.status, body: JSON.parse(text) as T };
  } catch {
    // A dev proxy with nothing behind it answers with plain text.
    throw new ApiError(res.status, res.status >= 500 ? 'the tradoor server is not reachable' : `unexpected response (${res.status})`);
  }
}

async function json<T>(path: string, init?: { method?: string; body?: unknown }): Promise<T> {
  const { status, body } = await request<T>(path, init);
  if (status >= 400 || body === null) {
    const error = (body as { error?: unknown } | null)?.error;
    throw new ApiError(status, typeof error === 'string' ? error : `request failed (${status})`);
  }
  return body;
}

export interface Product {
  market: string;
  venue: string;
  symbol: string;
  base: string;
  quote: string;
  kind: 'spot' | 'perp';
  priceDecimals: number | null;
  quoteVolume24h: number | null;
  lastPrice: number | null;
  changePct: number | null;
}

export interface VenueStatus {
  id: string;
  label: string;
  count: number;
  error: string | null;
}

export interface Catalog {
  updatedAt: number;
  venues: VenueStatus[];
  products: Product[];
  byMarket: Map<string, Product>;
}

let catalogCache: { at: number; value: Promise<Catalog> } | null = null;

/** Every market on every venue, fetched once and searched locally. */
export function catalog(): Promise<Catalog> {
  if (catalogCache && Date.now() - catalogCache.at < 30 * 60_000) return catalogCache.value;
  const value = json<Omit<Catalog, 'byMarket'>>('/api/markets').then((c) => ({ ...c, byMarket: new Map(c.products.map((p) => [p.market, p])) }));
  catalogCache = { at: Date.now(), value };
  value.catch(() => {
    if (catalogCache?.value === value) catalogCache = null;
  });
  return value;
}

export interface Drawing {
  id: number;
  market: string;
  timeframe: string;
  kind: string;
  points: DrawingPoint[];
  style: Record<string, unknown>;
  label: string | null;
  createdAt: number;
}

const scope = (market: string, timeframe: string): string => `market=${encodeURIComponent(market)}&timeframe=${encodeURIComponent(timeframe)}`;

export const api = {
  async health(): Promise<{ ok: boolean; db: boolean }> {
    const { body } = await request<{ ok: boolean; db: boolean }>('/api/health');
    return body ?? { ok: false, db: false };
  },
  drawings: (market: string, timeframe: string) => json<{ count: number; drawings: Drawing[] }>(`/api/drawings?${scope(market, timeframe)}`),
  createDrawing: (market: string, timeframe: string, input: { kind: string; points: DrawingPoint[]; label?: string }) =>
    json<{ drawing: Drawing }>(`/api/drawings?${scope(market, timeframe)}`, { method: 'POST', body: input }),
  deleteDrawing: (id: number) => json<{ id: number }>(`/api/drawings/${id}`, { method: 'DELETE' }),
  clearDrawings: (market: string, timeframe: string) => json<{ deleted: number }>(`/api/drawings?${scope(market, timeframe)}`, { method: 'DELETE' }),
  workspace: (id: string) => json<{ id: string; name: string; doc: unknown; version: number }>(`/api/workspaces/${id}`),
  async saveWorkspace(id: string, body: { name: string; doc: unknown; version: number }): Promise<{ version: number; conflict: boolean; doc?: unknown }> {
    const res = await request<{ version: number; doc?: unknown; error?: string }>(`/api/workspaces/${id}`, { method: 'PUT', body });
    if (res.status === 409 && res.body) return { version: res.body.version, conflict: true, doc: res.body.doc };
    if (res.status >= 400 || !res.body) throw new ApiError(res.status, res.body?.error ?? `saving the workspace failed (${res.status})`);
    return { version: res.body.version, conflict: false };
  },
};
