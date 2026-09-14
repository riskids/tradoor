/**
 * The engine thread: WASM, history, exchange streams and every chart's
 * canvases. The page sends input and settings; this sends back only what the
 * page shows in the DOM — legend, cursor, events and status.
 */
import init, { ChartEngine, LiveFeedEngine, fold_candles, indicator_values, profile_spec, study_catalog, timeframe_info, tpo } from '../wasm/pkg/tradoor_wasm.js';
import wasmUrl from '../wasm/pkg/tradoor_wasm_bg.wasm?url';
import { decimalsOf, MAX_PRICE_DECIMALS } from '../lib/format';
import { parseMarket } from '../lib/markets';
import { concatCols, emptyCols, openSeries, sliceCols, type Cols, type Series } from './history';
import type { CallName, Calls, ChartEvent, ChartStatus, FromWorker, IndicatorValues, PointerKind, StudyDefinition, ToWorker, TpoData } from './protocol';
import { StreamHub, type SocketState, type StreamListener, type StreamMsg } from './streams';

/** Closed bars a chart opens with: enough for EMA 200 to settle, with room to scroll. */
const CHART_BARS = 1_500;
/** Cap on the base bars behind them — 1,500 bars of 30m is 45,000 minutes. */
const MAX_BASE_BARS = 60_000;
/** Timeframes the exchange does not stream (2D, 3M…) pick up new bars this often. */
const POLL_MS = 5 * 60_000;

interface TfInfo {
  timeframe: string;
  interval: string;
  group: number;
  base: string;
  live: string | null;
  barMs: number;
  baseMs: number;
  intervalMs: number;
}

interface Bar {
  time: number;
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
  closed: boolean;
}

interface ProfileSpec {
  session: string;
  from: string;
}

const scope = self as unknown as { postMessage(msg: FromWorker): void; onmessage: ((e: MessageEvent<ToWorker>) => void) | null };
const post = (msg: FromWorker): void => scope.postMessage(msg);
const message = (e: unknown): string => (e instanceof Error ? e.message : String(e));
const EMPTY = new Float64Array(0);

const hub = new StreamHub();
const theme = new Map<string, string>();
const slots = new Map<string, Slot>();

const info = (tf: string): TfInfo => timeframe_info(tf) as unknown as TfInfo;

function barCols(bars: readonly Bar[]): Cols {
  return {
    time: Float64Array.from(bars, (b) => b.time),
    open: Float64Array.from(bars, (b) => b.open),
    high: Float64Array.from(bars, (b) => b.high),
    low: Float64Array.from(bars, (b) => b.low),
    close: Float64Array.from(bars, (b) => b.close),
    volume: Float64Array.from(bars, (b) => b.volume),
  };
}

/** The decimals an instrument trades at, read off its recent prices. */
function precisionOf(c: Cols): number {
  let digits = 2;
  for (let i = Math.max(0, c.time.length - 500); i < c.time.length; i += 1) {
    digits = Math.max(digits, decimalsOf(c.open[i]!), decimalsOf(c.high[i]!), decimalsOf(c.low[i]!), decimalsOf(c.close[i]!));
    if (digits >= MAX_PRICE_DECIMALS) return MAX_PRICE_DECIMALS;
  }
  return digits;
}

/** Base bars needed for `bars` bars of `tf` from this series. */
const baseBars = (series: Series, tf: TfInfo, bars: number): number =>
  Math.min(MAX_BASE_BARS, series.source === 'archive' ? Math.ceil((bars * tf.barMs) / tf.baseMs) : bars * tf.group);

class Slot {
  readonly engine: ChartEngine;
  private market = '';
  private timeframe = '';
  private series: Series | null = null;
  /** Closed bars at the chart's timeframe, as the engine holds them. */
  private closed: Cols = emptyCols();
  /** Bars the stream closed that stored history and REST do not cover yet. */
  private extra: Bar[] = [];
  private live: Bar | null = null;
  private feed: LiveFeedEngine | null = null;
  private gen = 0;
  private cleanup: (() => void)[] = [];
  private status: ChartStatus = { loading: false, error: null, bars: 0, source: null, live: 'off', exhausted: false };
  private legendKey = '';
  private cursor = '';
  private studiesKey = '';
  private metaDirty = true;
  private price = Number.NaN;
  private priceDirty = false;

  constructor(
    readonly id: string,
    gl: OffscreenCanvas,
    text: OffscreenCanvas,
  ) {
    this.engine = new ChartEngine(gl, text, '1h');
    for (const [key, value] of theme) this.engine.set_theme_value(key, value);
  }

  markMeta(): void {
    this.metaDirty = true;
  }

  destroy(): void {
    this.detach();
    this.engine.free();
  }

  private detach(): void {
    this.gen += 1;
    for (const fn of this.cleanup.splice(0)) fn();
    this.feed?.free();
    this.feed = null;
    this.series = null;
  }

  async load(market: string, timeframe: string): Promise<void> {
    if (market === this.market && timeframe === this.timeframe && !this.status.error) return;
    this.detach();
    const gen = this.gen;
    this.market = market;
    this.timeframe = timeframe;
    this.closed = emptyCols();
    this.extra = [];
    this.live = null;
    this.price = Number.NaN;
    this.engine.clear_live();
    this.engine.set_candles(EMPTY, EMPTY, EMPTY, EMPTY, EMPTY, EMPTY);
    this.metaDirty = true;
    let tf: TfInfo;
    try {
      tf = info(timeframe);
      this.engine.set_timeframe(timeframe);
    } catch (e) {
      this.fail(message(e));
      return;
    }
    this.engine.set_message('Loading…');
    this.setStatus({ loading: true, error: null, bars: 0, source: null, live: 'off', exhausted: false });
    try {
      const series = await openSeries(market, tf.base, tf.interval);
      if (gen !== this.gen) return;
      this.series = series;
      this.cleanup.push(series.subscribe(() => this.refold()));
      await series.ensure(baseBars(series, tf, CHART_BARS));
      if (gen !== this.gen) return;
      this.refold();
      this.startLive(gen, tf);
    } catch (e) {
      if (gen === this.gen) this.fail(message(e));
    }
  }

  private fail(error: string): void {
    this.engine.set_message(`Could not load candles — ${error}`);
    this.setStatus({ loading: false, error });
  }

  private lastClosedTime(): number {
    const t = this.closed.time;
    return t.length > 0 ? t[t.length - 1]! : Number.NEGATIVE_INFINITY;
  }

  private refold(): void {
    const series = this.series;
    if (!series) return;
    const c = series.cols;
    const folded = fold_candles(this.timeframe, c.time, c.open, c.high, c.low, c.close, c.volume, Date.now()) as unknown as Cols;
    const last = folded.time.length > 0 ? folded.time[folded.time.length - 1]! : Number.NEGATIVE_INFINITY;
    this.extra = this.extra.filter((b) => b.time > last);
    this.closed = this.extra.length > 0 ? concatCols([folded, barCols(this.extra)]) : folded;
    this.pushCandles();
    this.setStatus({ loading: false, error: null, bars: this.closed.time.length, source: series.source, exhausted: series.exhausted });
  }

  private pushCandles(): void {
    const k = this.closed;
    this.engine.set_candles(k.time, k.open, k.high, k.low, k.close, k.volume);
    this.engine.set_message(k.time.length > 0 ? '' : 'No closed candles for this market and timeframe yet.');
    this.pushLive();
    this.metaDirty = true;
    if (!this.live && k.time.length > 0) this.setPrice(k.close[k.time.length - 1]!);
  }

  private startLive(gen: number, tf: TfInfo): void {
    const ref = parseMarket(this.market);
    if (!ref || !tf.live) {
      this.setStatus({ live: 'off' });
      const timer = setInterval(() => void this.series?.refreshTail(), POLL_MS);
      this.cleanup.push(() => clearInterval(timer));
      return;
    }
    const feed = new LiveFeedEngine(this.timeframe);
    this.feed = feed;
    const listener: StreamListener = {
      message: (m: StreamMsg) => {
        if (gen !== this.gen) return;
        const changed =
          m.kind === 'kline'
            ? feed.apply_kline(m.openTime, m.open, m.high, m.low, m.close, m.volume, m.lastTradeId, m.closed)
            : m.kind === 'trade' && feed.apply_trade(m.time, m.price, m.qty, m.lastTradeId);
        if (changed) this.onLive(feed.bar() as Bar | null);
      },
      state: (state: SocketState) => {
        if (gen !== this.gen) return;
        const was = this.status.live;
        this.setStatus({ live: state });
        // Back from an outage: bars that closed meanwhile come from REST.
        if (state === 'live' && (was === 'retrying' || was === 'stalled')) void this.series?.refreshTail();
      },
    };
    this.cleanup.push(hub.subscribe(ref.venue, { kind: 'kline', symbol: ref.symbol, interval: tf.live }, listener));
    // Binance's kline publishes about once a second; trades fill in between.
    if (ref.venue.startsWith('binance')) this.cleanup.push(hub.subscribe(ref.venue, { kind: 'trades', symbol: ref.symbol }, listener));
  }

  private onLive(bar: Bar | null): void {
    if (!bar) return;
    const prev = this.live;
    this.live = bar;
    if (prev && bar.time > prev.time) {
      // The previous bar is final. Keep it until stored history or REST covers it.
      if (prev.time > this.lastClosedTime()) {
        const done = { ...prev, closed: true };
        this.extra.push(done);
        this.closed = concatCols([this.closed, barCols([done])]);
        this.pushCandles();
      }
      void this.series?.refreshTail();
    }
    this.pushLive();
    this.setPrice(bar.close);
  }

  private pushLive(): void {
    const b = this.live;
    if (b && b.time > this.lastClosedTime()) this.engine.set_live(b.time, b.open, b.high, b.low, b.close, b.volume);
  }

  private setPrice(price: number): void {
    if (price === this.price) return;
    this.price = price;
    this.priceDirty = true;
  }

  private setStatus(patch: Partial<ChartStatus>): void {
    const next = { ...this.status, ...patch };
    const keys = Object.keys(next) as (keyof ChartStatus)[];
    if (keys.every((k) => next[k] === this.status[k])) return;
    this.status = next;
    post({ t: 'status', id: this.id, value: next });
  }

  pointer(kind: PointerKind, x: number, y: number, button: number): void {
    const e = this.engine;
    if (kind === 'move') e.pointer_move(x, y);
    else if (kind === 'down') e.pointer_down(x, y, button);
    else if (kind === 'up') e.pointer_up(x, y);
    else if (kind === 'leave') e.pointer_leave();
    else e.double_click(x, y);
    this.afterInput();
  }

  afterInput(): void {
    const cursor = this.engine.cursor();
    if (cursor !== this.cursor) {
      this.cursor = cursor;
      post({ t: 'cursor', id: this.id, value: cursor });
    }
    this.flushEvents();
  }

  private flushEvents(): void {
    for (const event of this.engine.take_events() as ChartEvent[]) {
      if (event.type === 'needHistory') void this.loadOlder();
      else post({ t: 'event', id: this.id, event });
    }
  }

  private async loadOlder(): Promise<void> {
    const series = this.series;
    const gen = this.gen;
    if (!series || series.exhausted) return;
    this.setStatus({ loading: true });
    try {
      await series.older();
      if (gen === this.gen) this.setStatus({ loading: false, exhausted: series.exhausted });
    } catch (e) {
      if (gen === this.gen) this.setStatus({ loading: false, error: `older history: ${message(e)}` });
    }
  }

  frame(now: number): void {
    if (this.engine.draw(now)) {
      const legend = this.engine.legend();
      const key = JSON.stringify(legend);
      if (key !== this.legendKey) {
        this.legendKey = key;
        post({ t: 'legend', id: this.id, value: legend });
      }
      this.flushEvents();
    }
    if (this.metaDirty) {
      this.metaDirty = false;
      const studies = this.engine.studies();
      const key = JSON.stringify(studies);
      if (key !== this.studiesKey) {
        this.studiesKey = key;
        post({ t: 'studies', id: this.id, value: studies });
      }
    }
    if (this.priceDirty) {
      this.priceDirty = false;
      post({ t: 'price', id: this.id, price: this.price, precision: this.engine.precision() });
    }
  }
}

// ---------------------------------------------------------------- analysis calls

/** A series kept alive and loaded for the length of one call. */
async function hold(market: string, timeframe: string, bars: number): Promise<{ series: Series; tf: TfInfo; release: () => void }> {
  const tf = info(timeframe);
  const series = await openSeries(market, tf.base, tf.interval);
  const release = series.subscribe(() => {});
  try {
    await series.ensure(baseBars(series, tf, bars));
  } catch (e) {
    release();
    throw e;
  }
  return { series, tf, release };
}

const lastBars = (c: Cols, n: number): Cols => sliceCols(c, Math.max(0, c.time.length - n));

/** Source bars in one profile session. */
const SESSION_BARS: Record<string, number> = { '1d': 24, '1w': 7, '1M': 31 };

async function tpoCall({ market, timeframe, sessions }: Calls['tpo']['args']): Promise<TpoData | null> {
  const spec = profile_spec(timeframe) as ProfileSpec | null;
  if (!spec) return null;
  const want = (sessions + 2) * (SESSION_BARS[spec.session] ?? 31);
  const src = await hold(market, spec.from, want);
  try {
    const c = lastBars(src.series.cols, want);
    const data = tpo(timeframe, c.time, c.open, c.high, c.low, c.close, c.volume, sessions) as Omit<TpoData, 'precision'> | null;
    return data && { ...data, precision: precisionOf(c) };
  } finally {
    src.release();
  }
}

async function valuesCall({ market, timeframe, basis }: Calls['values']['args']): Promise<IndicatorValues | null> {
  const chart = await hold(market, timeframe, CHART_BARS);
  const spec = profile_spec(timeframe) as ProfileSpec | null;
  let src: Awaited<ReturnType<typeof hold>> | null = null;
  try {
    const c = chart.series.cols;
    const bars = fold_candles(timeframe, c.time, c.open, c.high, c.low, c.close, c.volume, Date.now()) as unknown as Cols;
    let s = emptyCols();
    if (spec) {
      // Enough source bars to cover the prior session of the last bar, and the view.
      const want = Math.min(20_000, Math.ceil((Math.min(bars.time.length, 300) * chart.tf.barMs) / info(spec.from).barMs) + 2 * (SESSION_BARS[spec.session] ?? 31));
      src = await hold(market, spec.from, want);
      s = lastBars(src.series.cols, want);
    }
    const k = lastBars(bars, 300);
    const out = indicator_values(timeframe, bars.time, bars.open, bars.high, bars.low, bars.close, bars.volume, s.time, s.open, s.high, s.low, s.close, s.volume, basis) as Omit<IndicatorValues, 'precision'> | null;
    return out && { ...out, precision: precisionOf(k) };
  } finally {
    chart.release();
    src?.release();
  }
}

type Handlers = { [K in CallName]: (args: Calls[K]['args']) => Calls[K]['result'] | Promise<Calls[K]['result']> };

const calls: Handlers = {
  studyCatalog: () => study_catalog() as StudyDefinition[],
  key: ({ id, key }) => {
    const slot = slots.get(id);
    if (!slot) return false;
    const used = slot.engine.key(key);
    slot.afterInput();
    return used;
  },
  tpo: tpoCall,
  values: valuesCall,
};

async function answer(req: number, name: CallName, args: unknown): Promise<void> {
  try {
    const value = await (calls[name] as (a: unknown) => unknown)(args);
    post({ t: 'reply', req, value });
  } catch (e) {
    post({ t: 'reply', req, error: message(e) });
  }
}

// ---------------------------------------------------------------- loop and messages

const raf: (cb: () => void) => void =
  typeof requestAnimationFrame === 'function' ? (cb) => void requestAnimationFrame(cb) : (cb) => void setTimeout(cb, 16);
let scheduled = false;

function schedule(): void {
  if (scheduled || slots.size === 0) return;
  scheduled = true;
  raf(tick);
}

function tick(): void {
  scheduled = false;
  const now = Date.now();
  for (const slot of slots.values()) {
    try {
      slot.frame(now);
    } catch (e) {
      console.error('chart frame failed', e);
    }
  }
  schedule();
}

function handle(msg: ToWorker): void {
  if (msg.t === 'call') {
    void answer(msg.req, msg.name, msg.args);
    return;
  }
  if (msg.t === 'theme') {
    theme.set(msg.key, msg.value);
    for (const slot of slots.values()) {
      slot.engine.set_theme_value(msg.key, msg.value);
      slot.markMeta();
    }
    return;
  }
  if (msg.t === 'create') {
    try {
      const slot = new Slot(msg.id, msg.gl, msg.text);
      slot.engine.resize(msg.width, msg.height, msg.dpr);
      slots.set(msg.id, slot);
      schedule();
    } catch (e) {
      post({ t: 'status', id: msg.id, value: { loading: false, error: message(e), bars: 0, source: null, live: 'off', exhausted: false } });
    }
    return;
  }
  const slot = slots.get(msg.id);
  if (!slot) return;
  switch (msg.t) {
    case 'destroy':
      slot.destroy();
      slots.delete(msg.id);
      break;
    case 'resize':
      slot.engine.resize(msg.width, msg.height, msg.dpr);
      break;
    case 'market':
      void slot.load(msg.market, msg.timeframe);
      break;
    case 'studies':
      slot.engine.set_studies(msg.value);
      slot.markMeta();
      break;
    case 'hidden':
      slot.engine.set_hidden(msg.value);
      slot.markMeta();
      break;
    case 'log':
      slot.engine.set_log_scale(msg.value);
      break;
    case 'paneWeights':
      slot.engine.set_pane_weights(msg.value);
      break;
    case 'tool':
      slot.engine.set_tool(msg.value);
      break;
    case 'drawings':
      slot.engine.set_drawings(msg.list);
      break;
    case 'selected':
      slot.engine.set_selected(msg.value ?? Number.NaN);
      break;
    case 'resetView':
      slot.engine.reset_view();
      break;
    case 'autoScale':
      slot.engine.set_auto_scale();
      slot.afterInput();
      break;
    case 'range':
      slot.engine.show_range(msg.days ?? Number.NaN);
      break;
    case 'pointer':
      slot.pointer(msg.kind, msg.x, msg.y, msg.button);
      break;
    case 'wheel':
      slot.engine.wheel(msg.x, msg.y, msg.dx, msg.dy);
      slot.afterInput();
      break;
  }
}

let ready = false;
const queued: ToWorker[] = [];

scope.onmessage = (e) => {
  if (ready) handle(e.data);
  else queued.push(e.data);
};

init({ module_or_path: wasmUrl })
  .then(() => {
    ready = true;
    post({ t: 'ready' });
    for (const msg of queued.splice(0)) handle(msg);
  })
  .catch((e: unknown) => post({ t: 'fatal', message: `the chart engine failed to load: ${message(e)}` }));
