/**
 * Exchange WebSocket streams, shared: one socket per venue however many charts
 * and watchlist rows listen, each topic subscribed once and reference counted.
 * The worker uses it for candles and trades, the page for watchlist tickers;
 * nothing here touches the DOM.
 */
import type { Venue } from '../lib/markets';

export type SocketState = 'connecting' | 'live' | 'stalled' | 'retrying';

export type StreamSpec =
  | { kind: 'kline'; symbol: string; interval: string }
  | { kind: 'trades'; symbol: string }
  | { kind: 'ticker'; symbol: string };

export interface KlineMsg {
  kind: 'kline';
  openTime: number;
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
  /** Last trade folded into the kline, comparable with a trade's id. */
  lastTradeId: number;
  closed: boolean;
}

export interface TradeMsg {
  kind: 'trade';
  time: number;
  price: number;
  qty: number;
  lastTradeId: number;
}

export interface TickerMsg {
  kind: 'ticker';
  last: number;
  changePct: number | null;
  quoteVolume: number | null;
}

export type StreamMsg = KlineMsg | TradeMsg | TickerMsg;

export interface StreamListener {
  message(msg: StreamMsg): void;
  state(state: SocketState): void;
}

type Json = Record<string, unknown>;

interface Protocol {
  /** Tried in turn when a socket opens and nothing ever arrives. */
  urls: readonly string[];
  topic(spec: StreamSpec): string;
  frames(op: 'subscribe' | 'unsubscribe', topics: string[]): unknown[];
  read(msg: Json, emit: (topic: string, msg: StreamMsg) => void): void;
  heartbeat?: unknown;
}

const num = (v: unknown): number => (typeof v === 'number' ? v : typeof v === 'string' ? Number(v) : Number.NaN);

function chunks<T>(list: T[], size: number): T[][] {
  const out: T[][] = [];
  for (let i = 0; i < list.length; i += size) out.push(list.slice(i, i + size));
  return out;
}

/**
 * Binance: a bare `/ws` socket and SUBSCRIBE frames. Streams named in the URL
 * path are silently dropped on some networks — the handshake succeeds and
 * nothing is pushed — while a SUBSCRIBE on the bare path is not.
 */
function binance(urls: string[]): Protocol {
  let id = 0;
  return {
    urls,
    topic(spec) {
      const s = spec.symbol.toLowerCase();
      if (spec.kind === 'kline') return `${s}@kline_${spec.interval}`;
      return spec.kind === 'trades' ? `${s}@aggTrade` : `${s}@miniTicker`;
    },
    // Every frame counts against a five-a-second limit, so topics travel in batches.
    frames: (op, topics) => chunks(topics, 100).map((params) => ({ method: op.toUpperCase(), params, id: ++id })),
    read(m, emit) {
      const s = typeof m.s === 'string' ? m.s.toLowerCase() : '';
      if (m.e === 'kline' && m.k && typeof m.k === 'object') {
        const k = m.k as Json;
        emit(`${s}@kline_${k.i}`, {
          kind: 'kline',
          openTime: num(k.t),
          open: num(k.o),
          high: num(k.h),
          low: num(k.l),
          close: num(k.c),
          volume: num(k.v),
          lastTradeId: typeof k.L === 'number' ? k.L : -1,
          closed: k.x === true,
        });
      } else if (m.e === 'aggTrade') {
        emit(`${s}@aggTrade`, { kind: 'trade', time: num(m.T), price: num(m.p), qty: num(m.q), lastTradeId: typeof m.l === 'number' ? m.l : -1 });
      } else if (m.e === '24hrMiniTicker') {
        const last = num(m.c);
        const open = num(m.o);
        emit(`${s}@miniTicker`, { kind: 'ticker', last, changePct: open > 0 ? ((last - open) / open) * 100 : null, quoteVolume: num(m.q) });
      }
    },
  };
}

const BYBIT_INTERVAL: Record<string, string> = {
  '1m': '1', '5m': '5', '15m': '15', '30m': '30',
  '1h': '60', '2h': '120', '4h': '240', '6h': '360', '12h': '720',
  '1d': 'D', '1w': 'W', '1M': 'M',
};

/**
 * Bybit: a ping every 15 seconds against its 20-second idle timeout, and linear
 * tickers that arrive as a snapshot followed by deltas. Trades are not folded —
 * its trade ids are strings that cannot be compared with a kline, and its kline
 * already pushes often.
 */
function bybit(url: string): Protocol {
  const tickers = new Map<string, { last: number; pct: number | null; turnover: number | null }>();
  return {
    urls: [url],
    topic(spec) {
      if (spec.kind === 'kline') return `kline.${BYBIT_INTERVAL[spec.interval] ?? spec.interval}.${spec.symbol}`;
      return spec.kind === 'ticker' ? `tickers.${spec.symbol}` : `publicTrade.${spec.symbol}`;
    },
    // Spot accepts at most ten topics a frame.
    frames: (op, topics) => chunks(topics, 10).map((args) => ({ op, args })),
    read(m, emit) {
      const topic = m.topic;
      if (typeof topic !== 'string') return;
      if (topic.startsWith('kline.') && Array.isArray(m.data)) {
        for (const k of m.data as Json[]) {
          // No trades are folded here, so every kline is taken whole: id 0.
          emit(topic, { kind: 'kline', openTime: num(k.start), open: num(k.open), high: num(k.high), low: num(k.low), close: num(k.close), volume: num(k.volume), lastTradeId: 0, closed: k.confirm === true });
        }
      } else if (topic.startsWith('tickers.') && m.data && typeof m.data === 'object') {
        const d = m.data as Json;
        const prev = m.type === 'delta' ? tickers.get(topic) : undefined;
        const next = {
          last: d.lastPrice !== undefined ? num(d.lastPrice) : (prev?.last ?? Number.NaN),
          pct: d.price24hPcnt !== undefined ? num(d.price24hPcnt) * 100 : (prev?.pct ?? null),
          turnover: d.turnover24h !== undefined ? num(d.turnover24h) : (prev?.turnover ?? null),
        };
        tickers.set(topic, next);
        if (Number.isFinite(next.last)) emit(topic, { kind: 'ticker', last: next.last, changePct: next.pct, quoteVolume: next.turnover });
      }
    },
    heartbeat: { op: 'ping' },
  };
}

const PROTOCOLS: Record<Venue, () => Protocol> = {
  'binance-spot': () => binance(['wss://stream.binance.com:9443/ws', 'wss://data-stream.binance.vision/ws']),
  'binance-perp': () => binance(['wss://fstream.binance.com/market/ws', 'wss://fstream.binance.com/ws']),
  'bybit-spot': () => bybit('wss://stream.bybit.com/v5/public/spot'),
  'bybit-perp': () => bybit('wss://stream.bybit.com/v5/public/linear'),
};

const STALL_MS = 12_000;
const RETRY_MS = [1_000, 3_000, 5_000, 10_000];
const HEARTBEAT_MS = 15_000;
/** An unused socket stays open this long, so switching timeframe does not reconnect. */
const LINGER_MS = 10_000;

type Timer = ReturnType<typeof setTimeout>;

class Connection {
  private ws: WebSocket | null = null;
  private current: SocketState = 'connecting';
  private readonly topics = new Map<string, Set<StreamListener>>();
  private readonly pending = { subscribe: new Set<string>(), unsubscribe: new Set<string>() };
  private flushQueued = false;
  private urlIndex = 0;
  private attempt = 0;
  private heard = false;
  private retryTimer: Timer | undefined;
  private stallTimer: Timer | undefined;
  private lingerTimer: Timer | undefined;
  private beatTimer: ReturnType<typeof setInterval> | undefined;

  constructor(private readonly proto: Protocol) {}

  topic(spec: StreamSpec): string {
    return this.proto.topic(spec);
  }

  add(topic: string, listener: StreamListener): () => void {
    clearTimeout(this.lingerTimer);
    let set = this.topics.get(topic);
    if (!set) {
      set = new Set();
      this.topics.set(topic, set);
      this.queue('subscribe', topic);
    }
    set.add(listener);
    listener.state(this.current);
    if (!this.ws && this.retryTimer === undefined) this.connect();
    return () => this.remove(topic, listener);
  }

  private remove(topic: string, listener: StreamListener): void {
    const set = this.topics.get(topic);
    if (!set || !set.delete(listener) || set.size > 0) return;
    this.topics.delete(topic);
    this.queue('unsubscribe', topic);
    if (this.topics.size === 0) this.lingerTimer = setTimeout(() => this.shutdown(), LINGER_MS);
  }

  /** Coalesces the changes made in one task into as few frames as possible. */
  private queue(op: 'subscribe' | 'unsubscribe', topic: string): void {
    const opposite = op === 'subscribe' ? this.pending.unsubscribe : this.pending.subscribe;
    if (opposite.delete(topic)) return;
    this.pending[op].add(topic);
    if (this.flushQueued) return;
    this.flushQueued = true;
    queueMicrotask(() => {
      this.flushQueued = false;
      const ws = this.ws;
      // Not open yet: onopen subscribes everything wanted at that moment.
      if (!ws || ws.readyState !== WebSocket.OPEN) return;
      for (const kind of ['unsubscribe', 'subscribe'] as const) {
        const list = [...this.pending[kind]];
        this.pending[kind].clear();
        if (list.length > 0) this.sendFrames(ws, kind, list);
      }
    });
  }

  private sendFrames(ws: WebSocket, op: 'subscribe' | 'unsubscribe', topics: string[]): void {
    for (const frame of this.proto.frames(op, topics)) ws.send(JSON.stringify(frame));
  }

  private setState(state: SocketState): void {
    if (state === this.current) return;
    this.current = state;
    for (const set of this.topics.values()) for (const listener of set) listener.state(state);
  }

  private connect(): void {
    this.retryTimer = undefined;
    const ws = new WebSocket(this.proto.urls[this.urlIndex % this.proto.urls.length]!);
    this.ws = ws;
    this.heard = false;

    ws.onopen = () => {
      this.pending.subscribe.clear();
      this.pending.unsubscribe.clear();
      this.sendFrames(ws, 'subscribe', [...this.topics.keys()]);
      this.stallTimer = setTimeout(() => {
        if (this.heard) return;
        // Connected and subscribed, yet silent. Say so rather than show a
        // frozen price as live, and try the next endpoint if there is one.
        this.setState('stalled');
        if (this.proto.urls.length > 1) ws.close();
      }, STALL_MS);
      if (this.proto.heartbeat !== undefined) {
        const beat = JSON.stringify(this.proto.heartbeat);
        this.beatTimer = setInterval(() => {
          if (ws.readyState === WebSocket.OPEN) ws.send(beat);
        }, HEARTBEAT_MS);
      }
    };

    ws.onmessage = (event) => {
      let msg: unknown;
      try {
        msg = JSON.parse(String(event.data));
      } catch {
        return;
      }
      if (!msg || typeof msg !== 'object') return;
      this.proto.read(msg as Json, (topic, m) => {
        if (!this.heard) {
          this.heard = true;
          this.attempt = 0;
          this.setState('live');
        }
        const set = this.topics.get(topic);
        if (set) for (const listener of set) listener.message(m);
      });
    };

    ws.onclose = () => {
      this.clearTimers();
      if (this.ws === ws) this.ws = null;
      if (this.topics.size === 0) return;
      if (!this.heard) this.urlIndex += 1;
      this.attempt += 1;
      this.setState('retrying');
      this.retryTimer = setTimeout(() => this.connect(), RETRY_MS[Math.min(this.attempt, RETRY_MS.length) - 1]);
    };
    ws.onerror = () => ws.close();
  }

  private clearTimers(): void {
    clearTimeout(this.stallTimer);
    clearInterval(this.beatTimer);
    this.stallTimer = undefined;
    this.beatTimer = undefined;
  }

  private shutdown(): void {
    if (this.topics.size > 0) return;
    clearTimeout(this.retryTimer);
    this.retryTimer = undefined;
    this.clearTimers();
    const ws = this.ws;
    this.ws = null;
    if (ws) {
      ws.onopen = null;
      ws.onclose = null;
      ws.onerror = null;
      ws.onmessage = null;
      ws.close();
    }
    this.pending.subscribe.clear();
    this.pending.unsubscribe.clear();
    this.attempt = 0;
    this.current = 'connecting';
  }
}

export class StreamHub {
  private readonly connections = new Map<Venue, Connection>();

  subscribe(venue: Venue, spec: StreamSpec, listener: StreamListener): () => void {
    let conn = this.connections.get(venue);
    if (!conn) {
      conn = new Connection(PROTOCOLS[venue]());
      this.connections.set(venue, conn);
    }
    return conn.add(conn.topic(spec), listener);
  }
}
