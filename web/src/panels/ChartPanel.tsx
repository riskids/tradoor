/**
 * One chart. The canvases belong to the engine worker; this component owns
 * the DOM around them — toolbar, drawing rail, legend, menus — and forwards
 * pointer input.
 */
import { batch, createEffect, createSignal, For, lazy, on, onCleanup, onMount, Show, untrack } from 'solid-js';
import { chartControllers, cycleMagnet, magnet, setOverlay, TOOL_KEYS } from '../app/ui';
import { call, holdChart, lastPrice, releaseChart, send } from '../engine/client';
import type { ChartEvent, ChartStatus, DrawingPoint, Legend, LiveStatus, MagnetMode, PointerKind, StudyMeta } from '../engine/protocol';
import { api, catalog, errorText, type ChartKind, type Drawing, type Product } from '../lib/api';
import type { DrawingPatch } from '../components/DrawingSettings';
import { defaultStyle, visibleOn, type Style } from '../lib/drawing-style';
import { before, DrawingHistory, type DrawingChange, type DrawingRef, type Step } from '../lib/drawing-history';
import { captureChart } from '../lib/capture';
import { demoRate, forceFlatRenderer } from '../lib/demo';
import { flowMarks, watchFlowMarks } from '../lib/flow-marks';
import { useDismiss } from '../lib/dismiss';
import { decimalsOf, formatPrice, formatTurnover, MAX_PRICE_DECIMALS } from '../lib/format';
import type { LastPrice } from '../lib/last-price';
import { Icon, type IconName } from '../lib/icons';
import { VenueTag } from '../lib/exchanges';
import { liquidationVenues } from '../lib/liquidation-venues';
import { formatClock, formatFileTime, zoneLabel } from '../lib/timezone';
import { serializeTpo, tpoValue, withTpoValue } from '../lib/tpo-settings';
import { symbolOf, venueLabel } from '../lib/markets';
import { alertState, createAlert, deleteAlert, digitsFor, editAlert, titleOf } from '../lib/alerts';
import { toast } from '../lib/toasts';
import { FAVORITE_TIMEFRAMES, TIMEFRAMES, tfLabel, tfLonger, tfMs, tfName } from '../lib/timeframes';
import { TPO_SPLITS, type ChartPanelState } from '../workspace/model';
import { doc, marketOf, patchPanel, setPanelTimeframe, timeframeOf } from '../workspace/store';

// Drawing settings load when a drawing is first selected: nobody needs them to see a chart.
const DrawingSettings = lazy(() => import('../components/DrawingSettings').then((m) => ({ default: m.DrawingSettings })));
const OpenInterestSettings = lazy(() => import('../components/OpenInterestSettings').then((m) => ({ default: m.OpenInterestSettings })));
const DrawingToolbar = lazy(() => import('../components/DrawingToolbar').then((m) => ({ default: m.DrawingToolbar })));

const TOOLS: { kind: string; icon: IconName; title: string }[] = [
  { kind: '', icon: 'pointer', title: 'Select and pan (Esc) — click a drawing to select it, then Delete to remove it' },
  { kind: 'path', icon: 'path', title: 'Path — click each turn, then Enter or double-click to finish' },
  { kind: 'trendline', icon: 'trendline', title: 'Trendline — click the two ends; anchored to time and price' },
  { kind: 'horizontal', icon: 'horizontal', title: 'Horizontal level — one click, drawn edge to edge' },
  { kind: 'ray', icon: 'ray', title: 'Ray — one click; the level starts where you put it' },
  { kind: 'rect', icon: 'rect', title: 'Zone — click opposite corners' },
  { kind: 'channel', icon: 'channel', title: 'Parallel channel — click the two ends of one line, then where the parallel line runs' },
  { kind: 'fib', icon: 'fib', title: 'Fib retracement — click the 100% end first, then the 0% end' },
  { kind: 'text', icon: 'text', title: 'Note — click where it goes, then type it' },
  { kind: 'measure', icon: 'measure', title: 'Measure — click from and to; reports the move, percent and bars' },
  { kind: 'long', icon: 'long', title: 'Long position — click the entry; drag its handles to set stop, target and width' },
  { kind: 'short', icon: 'short', title: 'Short position — click the entry; drag its handles to set stop, target and width' },
  { kind: 'brush', icon: 'brush', title: 'Brush — drag to draw freehand' },
];

/** The tool's name with its Alt shortcut: `Trendline (Alt+T) — click the two ends…`. */
const toolTip = (t: { kind: string; title: string }): string => {
  const key = Object.entries(TOOL_KEYS).find(([, kind]) => kind === t.kind)?.[0];
  if (!key) return t.title;
  const [name, ...rest] = t.title.split(' — ');
  return [`${name} (Alt+${key.slice(3)})`, ...rest].join(' — ');
};

const MAGNET_TIP: Record<MagnetMode, string> = {
  off: 'Magnet off (Alt+G) — click for a weak magnet, or hold Ctrl while drawing for one',
  weak: 'Weak magnet (Alt+G) — anchors near a bar take its open, high, low or close; elsewhere a whole tick. Click for strong',
  strong: 'Strong magnet (Alt+G) — anchors always take the nearest open, high, low or close. Click to turn it off',
};

/** What each click is for, in order — on a position tool the order is the meaning. */
const TOOL_STEPS: Record<string, string> = {
  path: 'Path: click each turn, then Enter or double-click to finish · esc cancels',
  brush: 'Brush: drag across the chart to draw',
  trendline: 'Trendline: click the first point, then the second · esc cancels',
  horizontal: 'Level: click the price',
  ray: 'Ray: click where the level starts',
  rect: 'Zone: click one corner, then the opposite corner · esc cancels',
  fib: 'Fib: click the 100% end — where the move started — then the 0% end · esc cancels',
  text: 'Note: click where the note goes',
  measure: 'Measure: click from, then to · esc cancels',
  channel: 'Channel: click both ends of the first line, then where the parallel line runs · shift keeps it level · esc cancels',
  long: 'Long: click where the entry goes',
  short: 'Short: click where the entry goes',
};

const LAYERS = [
  { id: 'ribbon', label: 'EMA 12/25 ribbon', candles: true },
  { id: 'ema200', label: 'EMA 200', candles: true },
  { id: 'volume', label: 'Volume', candles: true },
  { id: 'liquidations', label: 'Liquidations', candles: false },
];

const RANGES: { label: string; days: number | 'ytd' | null }[] = [
  { label: '1D', days: 1 },
  { label: '5D', days: 5 },
  { label: '1M', days: 30 },
  { label: '3M', days: 90 },
  { label: '6M', days: 180 },
  { label: 'YTD', days: 'ytd' },
  { label: '1Y', days: 365 },
  { label: 'All', days: null },
];

const LIVE: Record<LiveStatus, { label: string; tone: string; title: string }> = {
  off: { label: 'CLOSED BARS', tone: 'dim', title: 'The exchange streams no candle of this length; it is folded from shorter bars and refreshed every few minutes.' },
  connecting: { label: 'CONNECTING', tone: 'dim', title: 'Opening the exchange stream.' },
  live: { label: 'LIVE', tone: 'bull', title: 'Streaming from the exchange into the forming candle.' },
  stalled: { label: 'NO TICKS', tone: 'warn', title: 'Connected and subscribed, but the exchange has pushed nothing. Closed candles are unaffected.' },
  retrying: { label: 'RECONNECTING', tone: 'warn', title: 'The stream dropped; reconnecting. Bars that close meanwhile are fetched when it returns.' },
};

const splitList = (s: string): string[] =>
  s
    .split(',')
    .map((x) => x.trim())
    .filter(Boolean);

export function ChartPanel(props: { panel: ChartPanelState }) {
  const id = props.panel.id;
  const market = () => marketOf(props.panel);
  const timeframe = () => timeframeOf(props.panel);
  /** Profiles in place of candles: chosen, and a session longer than these bars. */
  /** The TPO split drawn now: chosen, and longer than the candles it stacks. */
  const split = () => (props.panel.tpo && tfLonger(props.panel.tpo, timeframe()) ? props.panel.tpo : null);
  const tpoShown = () => split() !== null;
  /** Candles and TPO profiles keep separate drawings. */
  const chartKind = (): ChartKind => (tpoShown() ? 'tpo' : 'candles');

  const [legend, setLegend] = createSignal<Legend | null>(null);
  const [studies, setStudies] = createSignal<StudyMeta[]>([]);
  const [status, setStatus] = createSignal<ChartStatus>({ loading: true, error: null, bars: 0, source: null, live: 'off', exhausted: false });
  const [cursor, setCursor] = createSignal('crosshair');
  const [tool, setTool] = createSignal('');
  const [autoScale, setAutoScale] = createSignal(true);
  const [selected, setSelected] = createSignal<number | null>(null);
  const [drawings, setDrawings] = createSignal<Drawing[]>([]);
  const [offline, setOffline] = createSignal<string | null>(null);
  const [drawError, setDrawError] = createSignal<string | null>(null);
  /** The study whose settings dialog is open. */
  const [studyEditing, setStudyEditing] = createSignal<string | null>(null);
  /** The drawing whose settings dialog is open. */
  const [editing, setEditing] = createSignal<number | null>(null);
  const [menu, setMenu] = createSignal<{ x: number; y: number } | null>(null);
  const [tfMenu, setTfMenu] = createSignal<number | null>(null);
  /** The TPO quick access menu, open at this x in the toolbar. */
  const [tpoMenu, setTpoMenu] = createSignal<number | null>(null);
  useDismiss(tpoMenu, () => setTpoMenu(null));
  useDismiss(menu, () => setMenu(null));
  useDismiss(tfMenu, () => setTfMenu(null));

  let box!: HTMLDivElement;
  let tpoButton!: HTMLButtonElement;

  // The panel's canvases and engine slot outlive this mount: see holdChart.
  const chart = holdChart(id, {
    cursor: setCursor,
    legend: setLegend,
    studies: setStudies,
    status: setStatus,
    event: (e) => onEvent(e),
  });
  const sid = chart.id;
  const price = () => {
    const p = lastPrice(id);
    return p && p.market === market() ? p : null;
  };

  // ---------------------------------------------------------------- flow
  // Minutes whose taker flow ran hard, marked on the candles. They are the Flow
  // study's: without it nothing is asked for and nothing is drawn. The server
  // keeps two days; which minutes count is the size set in the flow panel.
  const flowOn = () => splitList(props.panel.studies).some((s) => s.split(':')[0] === 'flow');
  createEffect(() => {
    if (flowOn()) onCleanup(watchFlowMarks(market()));
  });
  createEffect(() => {
    const list = flowOn() ? flowMarks(market()) : [];
    send({ t: 'flow', id: sid, marks: list.map((m) => ({ at: m.at, price: m.price, usd: m.deltaUsd })) });
  });
  const hoveredFlow = () => {
    const at = legend()?.flow;
    const mark = at && flowMarks(market())[at.index];
    return mark ? { mark, x: at.x, y: at.y } : null;
  };

  // ---------------------------------------------------------------- alerts
  // The market's active alerts, drawn by the engine as levels.
  createEffect(() => {
    const list = alertState.alerts.filter((a) => a.active && a.market === market()).map((a) => ({ id: a.id, price: a.price }));
    send({ t: 'alerts', id: sid, list });
  });
  // The add-alert button and an alert's label stay while the pointer is on
  // them, though the chart below has lost it by then.
  const [overClock, setOverClock] = createSignal<{ y: number; price: number } | null>(null);
  const [overAlert, setOverAlert] = createSignal<{ id: number; y: number } | null>(null);
  const axisX = () => legend()?.axisX ?? 0;
  // Not over an alert's line: its own label is there, with its own delete.
  const clock = () => overClock() ?? (tool() === '' && !legend()?.alert && !overAlert() ? (legend()?.cursor ?? null) : null);
  const hoveredAlert = () => {
    const h = overAlert() ?? legend()?.alert ?? null;
    const a = h && alertState.alerts.find((x) => x.id === h.id && x.active);
    return a && h ? { alert: a, y: h.y } : null;
  };
  const addAlertAt = (level: number) => {
    const digits = digitsFor(market(), level);
    const rounded = Number(level.toFixed(digits));
    // The price it was set from says which way it is reached.
    createAlert(market(), 'crossing', rounded, null, price()?.price).then(
      (a) => toast({ tone: 'info', title: 'Alert created', body: titleOf(a) }, 4_000),
      (err) => toast({ tone: 'warn', title: 'Alert not created', body: errorText(err) }),
    );
  };

  const local = (e: MouseEvent) => {
    const r = box.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };
  const pointer = (kind: PointerKind, e: MouseEvent) => {
    const p = local(e);
    send({ t: 'pointer', id: sid, kind, x: p.x, y: p.y, button: e.button, shift: e.shiftKey, ctrl: e.ctrlKey || e.metaKey });
  };

  onMount(() => {
    box.prepend(chart.gl, chart.text);
    const rect = box.getBoundingClientRect();
    if (chart.created) {
      send({ t: 'visible', id: sid, value: true });
      send({ t: 'resize', id: sid, width: rect.width, height: rect.height, dpr: devicePixelRatio });
    } else {
      try {
        const gl = chart.gl.transferControlToOffscreen();
        const text = chart.text.transferControlToOffscreen();
        send({ t: 'create', id: sid, gl, text, width: rect.width, height: rect.height, dpr: devicePixelRatio, flat: forceFlatRenderer() }, [gl, text]);
        chart.created = true;
      } catch (e) {
        setStatus((s) => ({ ...s, loading: false, error: `this browser cannot draw off the main thread (${errorText(e)})` }));
      }
    }
    const observer = new ResizeObserver(() => {
      const r = box.getBoundingClientRect();
      send({ t: 'resize', id: sid, width: r.width, height: r.height, dpr: devicePixelRatio });
    });
    observer.observe(box);
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const p = local(e);
      const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? box.clientHeight : 1;
      send({ t: 'wheel', id: sid, x: p.x, y: p.y, dx: e.deltaX * unit, dy: e.deltaY * unit });
    };
    box.addEventListener('wheel', onWheel, { passive: false });
    // `?liqdemo=30` invents thirty liquidations a minute, for judging the feed.
    if (demoRate() > 0) send({ t: 'demoLiquidations', id: sid, perMinute: demoRate() });
    chartControllers.set(id, {
      key: (key) => call('key', { id: sid, key }),
      tool: (kind) => {
        if (offline() === null) setTool(tool() === kind ? '' : kind);
      },
      resetView: () => send({ t: 'resetView', id: sid }),
      showRange: (days) => send({ t: 'range', id: sid, days }),
      undo,
      redo,
      capture,
    });
    onCleanup(() => {
      observer.disconnect();
      box.removeEventListener('wheel', onWheel);
      chartControllers.delete(id);
      // A tab switched away keeps its chart; a closed panel frees it.
      releaseChart(id, id in doc.panels);
    });
  });

  // The candles are the TPO brackets. A weekly split of 15-minute brackets is
  // 672 a profile, so a TPO chart opens with enough for a run of profiles.
  createEffect(() => {
    const s = split();
    const perProfile = s ? Math.round(tfMs(s) / tfMs(timeframe())) : 0;
    send({ t: 'market', id: sid, market: market(), timeframe: timeframe(), bars: s ? Math.min(Math.max(perProfile * 8, 1_500), 6_000) : undefined });
  });
  // The liquidation feed follows this ticker on every exchange that lists it.
  createEffect(() => {
    const m = market();
    let current = true;
    onCleanup(() => {
      current = false;
    });
    const post = (products: readonly Product[] | null) => {
      if (current) send({ t: 'liquidationMarkets', id: sid, markets: liquidationVenues(m, products).flatMap((v) => (v.market ? [v.market] : [])) });
    };
    catalog().then(
      (c) => post(c.products),
      () => post(null),
    );
  });
  createEffect(() => send({ t: 'studies', id: sid, value: props.panel.studies }));
  createEffect(() => send({ t: 'hidden', id: sid, value: props.panel.hidden }));
  createEffect(() => send({ t: 'log', id: sid, value: props.panel.log }));
  createEffect(() => send({ t: 'tpo', id: sid, value: split() ?? '' }));
  createEffect(() => send({ t: 'tpoSettings', id: sid, value: serializeTpo(props.panel.tpoSettings) }));
  const tpoSet = (key: string, value: string) => patchPanel(id, { tpoSettings: withTpoValue(props.panel.tpoSettings, key, value) });
  const tpoOn = (key: string) => tpoValue(props.panel.tpoSettings, key) === '1';
  createEffect(() => send({ t: 'paneWeights', id: sid, value: props.panel.paneWeights }));
  createEffect(() => send({ t: 'tool', id: sid, value: tool(), style: tool() ? defaultStyle(tool()) : {} }));
  createEffect(() => send({ t: 'magnet', id: sid, value: magnet() }));
  createEffect(() => send({ t: 'selected', id: sid, value: selected() }));
  // Every timeframe shows the market's drawings, bar those hidden on this one.
  createEffect(() => {
    const tf = timeframe();
    send({
      t: 'drawings',
      id: sid,
      list: drawings()
        .filter((d) => visibleOn(d.style, tf))
        .map((d) => ({ id: d.id, kind: d.kind, points: d.points, label: d.label, style: d.style })),
    });
  });

  // ---------------------------------------------------------------- drawings

  let drawingsRequest = 0;
  async function loadDrawings(): Promise<void> {
    const req = ++drawingsRequest;
    try {
      const res = await api.drawings(market(), timeframe(), chartKind());
      if (req !== drawingsRequest) return;
      batch(() => {
        setDrawings(res.drawings);
        setOffline(null);
      });
    } catch (e) {
      if (req !== drawingsRequest) return;
      batch(() => {
        setDrawings([]);
        setOffline(errorText(e));
      });
    }
  }

  // A tool armed on one chart must not start drawing on the next. Drawings
  // belong to the market and the chart — candles or TPO — so a new timeframe
  // keeps them without a refetch, and switching to TPO reads its own.
  createEffect(
    on([market, timeframe, chartKind], ([m, , kind], prev) => {
      batch(() => {
        setTool('');
        setSelected(null);
        setDrawError(null);
        setEditing(null);
      });
      if (!prev || prev[0] !== m || prev[2] !== kind) {
        // Steps are about this chart's drawings.
        setDrawings([]);
        history.reset();
        void loadDrawings();
      }
    }),
  );

  /* Every change is applied here first and sent afterwards, so a drawing appears
     under the pointer the moment it is placed rather than a round trip later. A
     rejected change is rolled back and the server's list is re-read. */
  let pending = 0;
  const onSameChart = (m: string, kind: ChartKind = chartKind()) => m === market() && kind === chartKind();
  const history = new DrawingHistory();

  /** Save a drawing. `ref`: one undo is bringing back, which keeps its steps. */
  async function saveDrawing(input: { kind: string; points: DrawingPoint[]; label?: string | null; style: Style; timeframe?: string }, ref?: DrawingRef): Promise<void> {
    const m = market();
    const kind = chartKind();
    const tf = input.timeframe ?? timeframe();
    const temporary: Drawing = { id: --pending, market: m, timeframe: tf, kind: input.kind, points: input.points, style: input.style, label: input.label ?? null, createdAt: Date.now() };
    const r = ref ?? history.ref(temporary.id);
    history.rekey(r, temporary.id);
    if (!ref) history.record({ kind: 'create', ref: r, drawing: temporary });
    setDrawings((list) => [...list, temporary]);
    try {
      const { drawing } = await api.createDrawing(m, tf, kind, { kind: input.kind, points: input.points, label: input.label ?? undefined, style: input.style });
      setDrawError(null);
      const kept = onSameChart(m, kind) && drawings().some((d) => d.id === temporary.id);
      if (!kept) {
        // Undone, or the chart moved on, before the server answered.
        void api.deleteDrawing(drawing.id).catch(() => {});
        return;
      }
      history.rekey(r, drawing.id);
      setDrawings((list) => list.map((d) => (d.id === temporary.id ? drawing : d)));
    } catch (e) {
      setDrawError(errorText(e));
      history.forget(r);
      if (onSameChart(m, kind)) setDrawings((list) => list.filter((d) => d.id !== temporary.id));
    }
  }

  const createDrawing = (kind: string, points: DrawingPoint[], label?: string) => saveDrawing({ kind, points, label, style: defaultStyle(kind) });

  /** `previous`: what the change replaces, when the list already shows it (a dialog's preview). */
  async function changeDrawing(drawingId: number, patch: DrawingChange, record = true, previous?: DrawingChange): Promise<void> {
    const m = market();
    const kind = chartKind();
    const current = drawings().find((d) => d.id === drawingId);
    if (record && current) history.record({ kind: 'change', ref: history.ref(drawingId), before: previous ?? before(current, patch), after: patch });
    setDrawings((list) => list.map((d) => (d.id === drawingId ? { ...d, ...patch } : d)));
    if (drawingId < 0) return; // still being created; its own request carries the points
    try {
      const { drawing } = await api.updateDrawing(drawingId, patch);
      setDrawError(null);
      if (onSameChart(m, kind)) setDrawings((list) => list.map((d) => (d.id === drawingId ? drawing : d)));
    } catch (e) {
      setDrawError(errorText(e));
      if (onSameChart(m, kind)) void loadDrawings();
    }
  }

  async function deleteDrawing(drawingId: number, record = true): Promise<void> {
    const current = drawings().find((d) => d.id === drawingId);
    if (record && current) history.record({ kind: 'delete', ref: history.ref(drawingId), drawing: current });
    setDrawings((list) => list.filter((d) => d.id !== drawingId));
    if (selected() === drawingId) setSelected(null);
    if (editing() === drawingId) setEditing(null);
    if (drawingId < 0) return;
    try {
      await api.deleteDrawing(drawingId);
      setDrawError(null);
    } catch (e) {
      setDrawError(errorText(e));
      void loadDrawings();
    }
  }

  /** Put a removed drawing back as it was, under the ref its steps know it by. */
  const restore = (ref: DrawingRef, d: Drawing) => saveDrawing({ kind: d.kind, points: d.points, label: d.label, style: d.style, timeframe: d.timeframe }, ref);

  function replay(step: Step, back: boolean): void {
    switch (step.kind) {
      case 'create':
        if (back) void deleteDrawing(step.ref.id, false);
        else void restore(step.ref, step.drawing);
        break;
      case 'delete':
        if (back) void restore(step.ref, step.drawing);
        else void deleteDrawing(step.ref.id, false);
        break;
      case 'change':
        void changeDrawing(step.ref.id, back ? step.before : step.after, false);
        break;
      case 'clear':
        if (back) for (const item of step.items) void restore(item.ref, item.drawing);
        else void clearDrawings(false);
        break;
    }
  }

  /** A picture of this chart, to the clipboard — or to a file when `save`. */
  const capture = (save: boolean) => captureChart(sid, `tradoor ${symbolOf(market())} ${tfLabel(timeframe())} ${formatFileTime(Date.now())}.png`, save);

  const undo = () => {
    const step = history.takeUndo();
    if (step) replay(step, true);
    return step !== undefined;
  };
  const redo = () => {
    const step = history.takeRedo();
    if (step) replay(step, false);
    return step !== undefined;
  };

  const selectedDrawing = () => {
    const s = selected();
    const d = s === null ? undefined : drawings().find((x) => x.id === s);
    return d ?? null;
  };
  const restyle = (d: Drawing, patch: Style) => void changeDrawing(d.id, { style: { ...d.style, ...patch } });

  /** The settings dialog shows each change on the chart before it is saved. */
  const previewDrawing = (drawingId: number, patch: DrawingPatch) =>
    setDrawings((list) => list.map((d) => (d.id === drawingId ? { ...d, style: patch.style, label: patch.label, points: patch.points } : d)));

  function commitDrawing(d: Drawing, patch: DrawingPatch): void {
    setEditing(null);
    // A note cannot lose its text: an emptied one keeps what it had.
    const label = d.kind === 'text' && patch.label === null ? d.label : patch.label;
    // `d` is the drawing as the dialog opened on it: what undo goes back to.
    void changeDrawing(d.id, { style: patch.style, points: patch.points, label }, true, { style: d.style, points: d.points, label: d.label });
  }

  async function clearDrawings(record = true): Promise<void> {
    const list = drawings();
    const n = list.length;
    if (n === 0 || (record && !confirm(`Remove all ${n} drawings on ${symbolOf(market())}, on every timeframe? Ctrl+Z brings them back.`))) return;
    if (record) history.record({ kind: 'clear', items: list.map((d) => ({ ref: history.ref(d.id), drawing: d })) });
    setDrawings([]);
    setSelected(null);
    try {
      await api.clearDrawings(market(), timeframe(), chartKind());
      setDrawError(null);
    } catch (e) {
      setDrawError(errorText(e));
      void loadDrawings();
    }
  }

  function onEvent(e: ChartEvent): void {
    switch (e.type) {
      case 'drawingCreated': {
        const points = e.points;
        if (e.kind === 'text') setOverlay({ kind: 'note', save: (label) => void createDrawing('text', points, label) });
        else void createDrawing(e.kind, points);
        break;
      }
      case 'selection':
        setSelected(e.id);
        break;
      case 'drawingChanged':
        void changeDrawing(e.id, { points: e.points });
        break;
      case 'editRequested':
        if (e.id >= 0) setEditing(e.id);
        break;
      case 'delete':
        if (editing() === e.id) setEditing(null);
        void deleteDrawing(e.id);
        break;
      case 'toolFinished':
        setTool('');
        break;
      case 'paneWeights':
        patchPanel(id, { paneWeights: e.value });
        break;
      case 'autoScale':
        setAutoScale(e.value);
        break;
      case 'alertMoved':
        editAlert(e.id, { price: e.price, atPrice: price()?.price }).catch((err) => toast({ tone: 'warn', title: 'Alert not moved', body: errorText(err) }));
        break;
      case 'needHistory':
        break;
    }
  }

  // ---------------------------------------------------------------- layers

  const hidden = () => new Set(splitList(props.panel.hidden));
  const studyCount = () => splitList(props.panel.studies).length;

  const toggleLayer = (layer: string) => {
    const next = hidden();
    if (next.has(layer)) next.delete(layer);
    else next.add(layer);
    patchPanel(id, { hidden: [...next].join(',') });
  };

  /** Swap a study for the same one with other parameters, keeping it shown or hidden. */
  const replaceStudy = (from: string, to: string) => {
    patchPanel(id, {
      studies: splitList(props.panel.studies)
        .map((s) => (s === from ? to : s))
        .filter((s, i, all) => all.indexOf(s) === i)
        .join(','),
      hidden: splitList(props.panel.hidden)
        .map((h) => (h === from ? to : h))
        .join(','),
    });
  };

  const removeStudy = (study: string) => {
    patchPanel(id, {
      studies: splitList(props.panel.studies)
        .filter((s) => s !== study)
        .join(','),
      // A removed study must not stay hidden, or adding it back would draw nothing.
      hidden: splitList(props.panel.hidden)
        .filter((h) => h !== study)
        .join(','),
    });
  };

  const showRange = (days: number | 'ytd' | null) => {
    const year = new Date().getUTCFullYear();
    send({ t: 'range', id: sid, days: days === 'ytd' ? (Date.now() - Date.UTC(year, 0, 1)) / 86_400_000 : days });
  };

  let tfButton!: HTMLButtonElement;

  return (
    <div class="panelcol" style={{ position: 'relative' }}>
      <div class="toolbar">
        <button class="symbol" data-tip="Change symbol — or just start typing" onClick={() => setOverlay({ kind: 'symbol', panelId: id, initial: '' })}>
          {symbolOf(market())}
          <Icon name="chevron" size={10} />
        </button>
        <VenueTag market={market()} name />
        <span class="sep" />
        <For each={FAVORITE_TIMEFRAMES}>
          {(tf) => (
            <button class="tb num" classList={{ on: timeframe() === tf }} data-tip={tfName(tf)} onClick={() => setPanelTimeframe(id, tf)}>
              {tfLabel(tf)}
            </button>
          )}
        </For>
        <Show when={!FAVORITE_TIMEFRAMES.includes(timeframe())}>
          <button class="tb num on" data-tip={tfName(timeframe())}>
            {tfLabel(timeframe())}
          </button>
        </Show>
        <button ref={tfButton} class="tb" data-tip="Every timeframe" onPointerDown={(e) => e.stopPropagation()} onClick={() => setTfMenu((v) => (v === null ? tfButton.offsetLeft : null))}>
          <Icon name="chevron" size={11} />
        </button>
        <span class="sep" />
        <button
          class="tb"
          data-tip="Copy a 4K picture of this chart · alt K — hold shift to save the file"
          aria-label="Copy a picture of this chart"
          onClick={(e) => capture(e.shiftKey)}
        >
          <Icon name="camera" size={13} />
        </button>
        <span class="sep" />
        <button class="tb" data-tip="Add an indicator" onClick={() => setOverlay({ kind: 'indicators', panelId: id })}>
          <Icon name="indicators" size={13} /> Indicators
          <Show when={studyCount() > 0}>
            <span class="dim">{studyCount()}</span>
          </Show>
        </button>
        <span class="sep" />
        <select data-tip="Candles, or TPO profiles — the candles are the brackets, the split is how long a profile runs" onChange={(e) => patchPanel(id, { tpo: e.currentTarget.value })}>
          <option value="" selected={!split()}>
            Candles
          </option>
          <For each={TPO_SPLITS.filter((s) => tfLonger(s.value, timeframe()))}>
            {(s) => (
              <option value={s.value} selected={split() === s.value}>
                TPO · {s.label}
              </option>
            )}
          </For>
        </select>
        <Show when={tpoShown()}>
          <button
            ref={tpoButton}
            class="tb"
            data-tip="TPO quick settings"
            onPointerDown={(e) => e.stopPropagation()}
            onClick={() => setTpoMenu((v) => (v === null ? tpoButton.offsetLeft : null))}
          >
            <Icon name="chevron" size={11} />
          </button>
        </Show>
        <span class="spacer" />
        <LiveBadge market={market()} live={status().live} price={price()} />
      </div>

      <Show when={tpoMenu()} keyed>
        {(left) => (
          <div class="menu" style={{ left: `${Math.max(0, Math.min(left - 170, box.clientWidth - 200))}px`, top: '25px', 'min-width': '220px' }} onPointerDown={(e) => e.stopPropagation()}>
            {/* Exocharts' quick access menu, in its order; kept inside the panel. */}
            <For
              each={[
                { value: 'mono', label: 'TPO Mono Color' },
                { value: 'heatmap', label: 'TPO HeatMap' },
                { value: 'candle', label: 'TPO Candle Up/Down' },
              ]}
            >
              {(mode) => (
                <button classList={{ cur: tpoValue(props.panel.tpoSettings, 'colorMode') === mode.value }} onClick={() => tpoSet('colorMode', mode.value)}>
                  {mode.label}
                </button>
              )}
            </For>
            <hr />
            <For
              each={[
                { key: 'expand', label: 'Expand clusters' },
                { key: 'letters', label: 'Show TPO letters' },
                { key: 'ib', label: 'Initial balance' },
                { key: 'sellTail', label: 'Sell tail' },
                { key: 'buyTail', label: 'Buy tail' },
                { key: 'singlePrints', label: 'Single prints' },
                { key: 'pocRay', label: 'POC ray' },
              ]}
            >
              {(item) => (
                <button onClick={() => tpoSet(item.key, tpoOn(item.key) ? '0' : '1')}>
                  {item.label}
                  <span class={tpoOn(item.key) ? 'bull' : 'dim'}>{tpoOn(item.key) ? 'on' : 'off'}</span>
                </button>
              )}
            </For>
            <button onClick={() => tpoSet('fading', tpoValue(props.panel.tpoSettings, 'fading') === 'off' ? 'time' : 'off')}>
              Add cluster fading
              <span class={tpoValue(props.panel.tpoSettings, 'fading') !== 'off' ? 'bull' : 'dim'}>{tpoValue(props.panel.tpoSettings, 'fading') !== 'off' ? 'on' : 'off'}</span>
            </button>
            <hr />
            <button
              onClick={() => {
                setTpoMenu(null);
                setOverlay({ kind: 'tpoSettings', panelId: id });
              }}
            >
              TPO settings…
            </button>
          </div>
        )}
      </Show>

      <Show when={tfMenu()} keyed>
        {(left) => (
          <div class="menu" style={{ left: `${left}px`, top: '25px', 'min-width': '0' }} onPointerDown={(e) => e.stopPropagation()}>
            <For each={[0, 1, 2, 3, 4]}>
              {(group) => (
                <div class="group">
                  <For each={TIMEFRAMES.filter((t) => t.group === group)}>
                    {(t) => (
                      <button
                        class="num"
                        classList={{ cur: t.value === timeframe() }}
                        data-tip={t.live ? t.name : `${t.name} — folded from shorter bars; closed bars only`}
                        onClick={() => {
                          setPanelTimeframe(id, t.value);
                          setTfMenu(null);
                        }}
                      >
                        {t.label}
                      </button>
                    )}
                  </For>
                </div>
              )}
            </For>
          </div>
        )}
      </Show>

      <div class="chartwrap">
        <div class="rail">
          <For each={TOOLS}>
            {(t) => (
              <button
                classList={{ on: tool() === t.kind }}
                disabled={t.kind !== '' && offline() !== null}
                data-tip={t.kind !== '' && offline() ? `${t.title.split(' — ')[0]} — unavailable: drawings are saved on the tradoor server (${offline()})` : toolTip(t)}
                onClick={() => setTool(tool() === t.kind ? '' : t.kind)}
              >
                <Icon name={t.icon} />
              </button>
            )}
          </For>
          <hr />
          <button class="magnet" classList={{ on: magnet() !== 'off', strong: magnet() === 'strong' }} data-tip={MAGNET_TIP[magnet()]} onClick={cycleMagnet}>
            <Icon name="magnet" />
          </button>
          <hr />
          <button
            class="del"
            disabled={selected() === null}
            data-tip={selected() === null ? 'Select a drawing on the chart first' : 'Delete the selected drawing (Delete)'}
            onClick={() => {
              const s = selected();
              if (s !== null) void deleteDrawing(s);
            }}
          >
            <Icon name="trash" />
          </button>
          <button class="del" disabled={drawings().length === 0} data-tip={`Remove all ${drawings().length} drawings on this market, on every timeframe`} onClick={() => void clearDrawings()}>
            <Icon name="eraser" />
          </button>
          <Show when={drawings().length > 0}>
            <span class="count num">{drawings().length}</span>
          </Show>
        </div>

        <div
          ref={box}
          class="canvasbox"
          style={{ cursor: cursor() }}
          onPointerDown={(e) => {
            if (e.button === 2) return;
            box.setPointerCapture(e.pointerId);
            pointer('down', e);
          }}
          onPointerMove={(e) => pointer('move', e)}
          onPointerUp={(e) => {
            if (box.hasPointerCapture(e.pointerId)) box.releasePointerCapture(e.pointerId);
            if (e.button !== 2) pointer('up', e);
          }}
          onPointerLeave={(e) => pointer('leave', e)}
          onDblClick={(e) => pointer('dblclick', e)}
          onContextMenu={(e) => {
            e.preventDefault();
            const p = local(e);
            setMenu({ x: Math.max(0, Math.min(p.x, box.clientWidth - 220)), y: Math.max(0, Math.min(p.y, box.clientHeight - 120)) });
          }}
        >
          <Show when={tool() !== '' || selected() !== null || drawError()}>
            <div class="hint">
              {tool() ? TOOL_STEPS[tool()] : selected() !== null ? 'Drawing selected · double-click for settings · Delete removes it, esc deselects' : ''}
              <Show when={drawError()}>{(err) => <span class="warn"> ▲ {err()}</span>}</Show>
            </div>
          </Show>
          <Show when={editing() === null && selectedDrawing()}>
            {(d) => <DrawingToolbar drawing={d()} onRestyle={(patch) => restyle(d(), patch)} onSettings={() => setEditing(d().id)} onDelete={() => void deleteDrawing(d().id)} />}
          </Show>
          <Show when={clock()}>
            {(c) => (
              <button
                class="alert-add"
                style={{ left: `${axisX() - 22}px`, top: `${c().y - 9}px` }}
                data-tip={`Add alert at ${formatPrice(c().price, digitsFor(market(), c().price))}`}
                onPointerEnter={() => setOverClock(c())}
                onPointerLeave={() => setOverClock(null)}
                onPointerDown={(e) => e.stopPropagation()}
                onClick={(e) => {
                  e.stopPropagation();
                  addAlertAt(c().price);
                  setOverClock(null);
                }}
              >
                <Icon name="alarm" size={13} />
              </button>
            )}
          </Show>
          <Show when={hoveredFlow()}>
            {(h) => (
              <div class="flow-label" classList={{ bull: h().mark.deltaUsd > 0, bear: h().mark.deltaUsd < 0 }} style={{ left: `${h().x + 12}px`, top: `${h().y - 30}px` }}>
                <span class="dim">Flow</span>
                <b>
                  {h().mark.deltaUsd > 0 ? '+' : '-'}${formatTurnover(Math.abs(h().mark.deltaUsd))}
                </b>
                <span class="dim">{h().mark.deltaUsd > 0 ? 'net bought in a minute' : 'net sold in a minute'}</span>
                <span>{formatClock(h().mark.at)}</span>
                <span class="dim">{formatPrice(h().mark.price, digitsFor(market(), h().mark.price))}</span>
              </div>
            )}
          </Show>
          <Show when={hoveredAlert()}>
            {(h) => (
              <div
                class="alert-label"
                style={{ right: `${(box?.clientWidth ?? 0) - axisX() + 22}px`, top: `${h().y - 11}px` }}
                onPointerEnter={() => setOverAlert({ id: h().alert.id, y: h().y })}
                onPointerLeave={() => setOverAlert(null)}
                onPointerDown={(e) => e.stopPropagation()}
              >
                <span>{titleOf(h().alert)}</span>
                <button
                  data-tip="Delete this alert"
                  aria-label="Delete this alert"
                  onClick={(e) => {
                    e.stopPropagation();
                    setOverAlert(null);
                    deleteAlert(h().alert.id).catch((err) => toast({ tone: 'warn', title: 'Alert not deleted', body: errorText(err) }));
                  }}
                >
                  <Icon name="trash" size={12} />
                </button>
              </div>
            )}
          </Show>
          <div class="legend">
            <div class="line">
              <button
                class="fold"
                classList={{ shut: props.panel.legendShut }}
                data-tip={props.panel.legendShut ? 'Show the layers' : 'Hide the layers'}
                aria-label={props.panel.legendShut ? 'Show the layers' : 'Hide the layers'}
                onClick={() => patchPanel(id, { legendShut: !props.panel.legendShut })}
              >
                <Icon name="chevron" size={11} stroke={2.2} />
              </button>
              <span class="title">{symbolOf(market())}</span>
              <span class="dim">
                {tfLabel(timeframe())}
                {split() ? ` · TPO split by ${TPO_SPLITS.find((t) => t.value === split())?.label.toLowerCase()}` : ''} · {venueLabel(market())}
              </span>
              <Show when={legend()}>
                {(l) => (
                  <>
                    <span class="num muted">{l().ohlc}</span>
                    <span class="num" classList={{ bull: l().up, bear: !l().up }}>
                      {l().change}
                    </span>
                    <span class="num dim">{l().volume}</span>
                  </>
                )}
              </Show>
            </div>
            <Show when={!props.panel.legendShut}>
              <For each={LAYERS.filter((layer) => !(tpoShown() && layer.candles))}>{(layer) => <LayerRow label={layer.label} off={hidden().has(layer.id)} onToggle={() => toggleLayer(layer.id)} />}</For>
            </Show>
            <For each={props.panel.legendShut ? [] : studies()}>
              {(s) => (
                <LayerRow
                  label={s.label}
                  off={s.hidden}
                  onToggle={() => toggleLayer(s.id)}
                  onRemove={() => removeStudy(s.id)}
                  onSettings={s.id.split(':')[0] === 'oi' ? () => setStudyEditing(s.id) : undefined}
                  values={s.hidden ? [] : s.lines.map((line) => ({ color: line.color, text: () => legend()?.values[line.id] ?? '—' }))}
                  stats={s.hidden ? [] : s.stats}
                />
              )}
            </For>
          </div>
          <Show when={menu()}>
            {(m) => (
              <div class="menu" style={{ left: `${m().x}px`, top: `${m().y}px` }} onPointerDown={(e) => e.stopPropagation()} onContextMenu={(e) => e.preventDefault()}>
                <button
                  onClick={() => {
                    send({ t: 'resetView', id: sid });
                    setMenu(null);
                  }}
                >
                  Reset chart view<span class="dim">auto-scale</span>
                </button>
                <Show when={!autoScale()}>
                  <button
                    onClick={() => {
                      send({ t: 'autoScale', id: sid });
                      setMenu(null);
                    }}
                  >
                    Auto-scale price<span class="dim">follow the bars</span>
                  </button>
                </Show>
                <button
                  onClick={() => {
                    patchPanel(id, { log: !props.panel.log });
                    setMenu(null);
                  }}
                >
                  {props.panel.log ? 'Linear price scale' : 'Logarithmic price scale'}
                </button>
                <button
                  onClick={() => {
                    setMenu(null);
                    setOverlay({ kind: 'indicators', panelId: id });
                  }}
                >
                  Add indicator…
                </button>
                <hr />
                <button
                  disabled={offline() !== null}
                  onClick={() => {
                    setMenu(null);
                    setTool('horizontal');
                  }}
                >
                  Draw a level<span class="dim">tool</span>
                </button>
              </div>
            )}
          </Show>
        </div>
      </div>

      <Show when={studyEditing()} keyed>
        {(studyId) => <OpenInterestSettings id={studyId} onApply={(next) => replaceStudy(studyId, next)} onClose={() => setStudyEditing(null)} />}
      </Show>

      {/* Keyed on the id alone: each previewed change must not remount the dialog. */}
      <Show when={editing()} keyed>
        {(editId) => {
          const d = untrack(() => drawings().find((x) => x.id === editId));
          return (
            <Show when={d}>
              {(drawing) => (
                <DrawingSettings
                  drawing={drawing()}
                  precision={untrack(() => price()?.precision ?? 2)}
                  onPreview={(patch) => previewDrawing(editId, patch)}
                  onCommit={(patch) => commitDrawing(drawing(), patch)}
                  onCancel={() => setEditing(null)}
                />
              )}
            </Show>
          );
        }}
      </Show>

      <div class="rangebar">
        <For each={RANGES}>
          {(r) => (
            <button class="tb num" data-tip={r.days === null ? 'Every loaded bar' : `Zoom to the last ${r.label}`} onClick={() => showRange(r.days)}>
              {r.label}
            </button>
          )}
        </For>
        <span class="spacer" />
        <Show when={status().error}>{(err) => <span class="warn" data-tip={err()}>▲ {err()} · </span>}</Show>
        <span class="dim num" data-tip={status().source === 'archive' ? 'Stored history, with the exchange filling the newest bars' : 'Candles from the exchange; this market is not archived here'}>
          {status().loading ? 'loading… · ' : ''}
          {status().bars.toLocaleString('en-US')} bars{status().source ? ` · ${status().source}` : ''} · {zoneLabel()}
        </span>
        <span class="sep" />
        <button
          class="tb"
          classList={{ on: autoScale() }}
          data-tip="Auto scale — the price axis follows the bars on screen. Dragging the chart up or down, or dragging the price axis, turns it off; click to turn it back on."
          onClick={() => send({ t: 'autoScale', id: sid })}
        >
          AUTO
        </button>
        <button class="tb" classList={{ on: !props.panel.log }} data-tip="Linear price axis" onClick={() => patchPanel(id, { log: false })}>
          LIN
        </button>
        <button class="tb" classList={{ on: props.panel.log }} data-tip="Logarithmic price axis — equal percentage moves take equal space" onClick={() => patchPanel(id, { log: true })}>
          LOG
        </button>
        <button class="tb" data-tip="Reset view — the last bars, every axis autoscaling" onClick={() => send({ t: 'resetView', id: sid })}>
          <Icon name="reset" size={11} />
        </button>
      </div>
    </div>
  );
}

function LayerRow(props: {
  label: string;
  off: boolean;
  onToggle(): void;
  onRemove?: () => void;
  onSettings?: () => void;
  values?: { color: string; text: () => string }[];
  stats?: { label: string; value: string }[];
}) {
  return (
    <div class="layer" classList={{ off: props.off }}>
      <span class="name">{props.label}</span>
      <For each={props.values ?? []}>
        {(v) => (
          <span class="val" style={{ color: v.color }}>
            {v.text()}
          </span>
        )}
      </For>
      <For each={props.stats ?? []}>
        {(s) => (
          <span class="dim">
            {s.label} <span class="val muted">{s.value}</span>
          </span>
        )}
      </For>
      {/* Only the buttons keep the press: a drag that starts on the legend text still pans. */}
      <span class="btns" onPointerDown={(e) => e.stopPropagation()}>
        <Show when={props.onSettings}>
          {(open) => (
            <button data-tip="Settings" onClick={() => open()()}>
              <Icon name="settings" size={12} />
            </button>
          )}
        </Show>
        <button data-tip={props.off ? 'Show' : 'Hide'} onClick={() => props.onToggle()}>
          <Icon name={props.off ? 'eyeOff' : 'eye'} size={12} />
        </button>
        <Show when={props.onRemove}>
          {(remove) => (
            <button data-tip="Remove" onClick={() => remove()()}>
              <Icon name="close" size={12} />
            </button>
          )}
        </Show>
      </span>
    </div>
  );
}

/**
 * Stream state and the last price. The decimals only ever grow, so the number
 * keeps its width as prints land on round prices, and it holds the colour of
 * the last change through unchanged prints.
 */
function LiveBadge(props: { market: string; live: LiveStatus; price: LastPrice | null }) {
  let digits = 0;
  createEffect(
    on(
      () => props.market,
      () => {
        digits = 0;
      },
    ),
  );
  // The same move the tab title's arrow shows.
  const direction = () => props.price?.direction ?? 0;
  const text = () => {
    const p = props.price;
    if (!p || !Number.isFinite(p.price)) return '';
    digits = Math.min(MAX_PRICE_DECIMALS, Math.max(digits, p.precision, decimalsOf(p.price)));
    return formatPrice(p.price, digits);
  };
  const state = () => LIVE[props.live];
  return (
    <span class="live" data-tip={state().title}>
      <span class={`state ${state().tone}`} classList={{ pulse: props.live === 'connecting' || props.live === 'retrying' }}>
        {state().label}
      </span>
      <span class="num" classList={{ bull: direction() > 0, bear: direction() < 0, muted: direction() === 0 }}>
        {text()}
      </span>
    </span>
  );
}
