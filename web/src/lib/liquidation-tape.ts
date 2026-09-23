/**
 * A ticker's liquidation tape, owned by the page rather than by a panel.
 * Where the server follows the ticker the tape is its live feed, every
 * exchange it listens to; elsewhere the page listens to Binance and Bybit.
 * Switching tabs unmounts a panel; its tape must still be there when it comes
 * back, with what happened meanwhile. Once no panel has shown a tape for half
 * an hour its sockets close. A reload starts every tape over — the server keeps
 * the day for the tickers it follows, and seeds the tape from that.
 */
import { createSignal, type Accessor } from 'solid-js';
import { createStore } from 'solid-js/store';
import { StreamHub, type SocketState } from '../engine/streams';
import { catalog, type LiquidationRow } from './api';
import { demoHit, demoRate } from './demo';
import { followLiquidations, type ServerVenue } from './liquidation-feed';
import { liquidationVenues, type LiquidationVenue } from './liquidation-venues';
import { parseMarket, symbolOf } from './markets';

/** Tapes keep their own sockets: liquidations are few and the engine thread is busy. */
const hub = new StreamHub();

/** Rows held before the oldest drop off. */
const MAX_ROWS = 500;
/** How long a tape nobody shows keeps listening. */
const LINGER_MS = 30 * 60_000;

export interface TapeHit {
  /** The same hit from the server and from the stream has the same key. */
  key: string;
  /** `okx-perp` and the like: the exchange it happened on. */
  venue: string;
  time: number;
  price: number;
  qty: number;
  usd: number;
  /** A long position was closed out. */
  long: boolean;
}

export type VenueStatus = SocketState | 'unlisted';

export interface Tape {
  readonly ticker: string;
  hits: Accessor<TapeHit[]>;
  /** Every perpetual exchange, and the ticker's market there or null. */
  venues: Accessor<LiquidationVenue[]>;
  status: Partial<Record<string, VenueStatus>>;
  /** Hits come from the server's feed rather than the page's own sockets. */
  fromServer: Accessor<boolean>;
  /** Hits the server kept, merged under the ones streamed since. */
  seed(rows: readonly LiquidationRow[]): void;
}

interface Entry {
  tape: Tape;
  users: number;
  linger: ReturnType<typeof setTimeout> | undefined;
  close(): void;
}

const tapes = new Map<string, Entry>();

const keyOf = (venue: string, time: number, price: number, qty: number) => `${venue}|${time}|${price}|${qty}`;

function open(market: string): Entry {
  const ticker = symbolOf(market);
  const [hits, setHits] = createSignal<TapeHit[]>([]);
  const [venues, setVenues] = createSignal<LiquidationVenue[]>([]);
  const [status, setStatus] = createStore<Partial<Record<string, VenueStatus>>>({});
  const [fromServer, setFromServer] = createSignal(false);
  const offs: (() => void)[] = [];
  let closed = false;

  const merge = (incoming: TapeHit[]) =>
    setHits((rows) => {
      const seen = new Set(rows.map((r) => r.key));
      const fresh = incoming.filter((h) => !seen.has(h.key));
      if (fresh.length === 0) return rows;
      return [...fresh, ...rows].sort((a, b) => b.time - a.time).slice(0, MAX_ROWS);
    });

  const add = (venue: string, time: number, price: number, qty: number, long: boolean) =>
    merge([{ key: keyOf(venue, time, price, qty), venue, time, price, qty, usd: price * qty, long }]);

  const SERVER_STATE: Record<string, VenueStatus> = { connected: 'live', connecting: 'connecting', retrying: 'retrying' };
  const showServerVenues = (list: ServerVenue[]) => {
    setVenues(list.map((v) => ({ venue: v.venue, market: `${v.venue}:${ticker}` })));
    for (const v of list) setStatus(v.venue, SERVER_STATE[v.state] ?? 'connecting');
  };

  // The server first: one feed for every exchange it follows.
  offs.push(
    followLiquidations(ticker, {
      hit: (h) => {
        setFromServer(true);
        add(h.venue, h.time, h.price, h.qty, h.long);
      },
      venues: (list) => {
        setFromServer(true);
        showServerVenues(list);
      },
      unavailable: () => {
        if (!closed) listenDirectly();
      },
    }),
  );

  const listenDirectly = () => void catalog()
    .then(
      (c) => ({ list: liquidationVenues(market, c.products), last: c.byMarket.get(market)?.lastPrice ?? 0 }),
      () => ({ list: liquidationVenues(market, null), last: 0 }),
    )
    .then(({ list, last }) => {
      if (closed) return;
      setVenues(list);
      for (const v of list) {
        const ref = v.market ? parseMarket(v.market) : null;
        if (!ref) {
          setStatus(v.venue, 'unlisted');
          continue;
        }
        setStatus(v.venue, 'connecting');
        offs.push(
          hub.subscribe(
            ref.venue,
            { kind: 'liquidations', symbol: ref.symbol },
            {
              message: (msg) => {
                if (msg.kind === 'liquidation') add(v.venue, msg.time, msg.price, msg.qty, msg.long);
              },
              state: (s) => setStatus(v.venue, s),
            },
          ),
        );
      }
      // Development only: invented hits spread over the listed exchanges.
      const listed = list.filter((v) => v.market);
      if (demoRate() > 0 && listed.length > 0 && last > 0) {
        const timer = setInterval(() => {
          const hit = demoHit(last);
          add(listed[Math.floor(Math.random() * listed.length)]!.venue, Date.now(), hit.price, hit.qty, hit.long);
        }, 60_000 / demoRate());
        offs.push(() => clearInterval(timer));
      }
    });

  const tape: Tape = {
    ticker,
    hits,
    venues,
    status,
    fromServer,
    seed: (rows) => merge(rows.filter((r) => r.venue.endsWith('-perp')).map((r) => ({ key: keyOf(r.venue, r.time, r.price, r.qty), venue: r.venue, time: r.time, price: r.price, qty: r.qty, usd: r.usd, long: r.long }))),
  };
  return {
    tape,
    users: 0,
    linger: undefined,
    close: () => {
      closed = true;
      for (const off of offs.splice(0)) off();
    },
  };
}

/** The tape for `market`'s ticker, shared by every panel showing it. Pair with `releaseTape`. */
export function holdTape(market: string): Tape {
  const ticker = symbolOf(market);
  let entry = tapes.get(ticker);
  if (!entry) {
    entry = open(market);
    tapes.set(ticker, entry);
  }
  entry.users += 1;
  clearTimeout(entry.linger);
  return entry.tape;
}

export function releaseTape(tape: Tape): void {
  const entry = tapes.get(tape.ticker);
  if (!entry || entry.tape !== tape) return;
  entry.users = Math.max(0, entry.users - 1);
  if (entry.users > 0) return;
  entry.linger = setTimeout(() => {
    entry.close();
    tapes.delete(tape.ticker);
  }, LINGER_MS);
}
