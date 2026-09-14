/**
 * Time Price Opportunity: one letter per bracket, stacked at the prices it
 * traded. Its own canvas and layout — x is how many brackets traded at a price,
 * not when — with sessions side by side and the price axis pinned right.
 */
import { batch, createEffect, createSignal, on, onCleanup, Show } from 'solid-js';
import { call } from '../engine/client';
import type { TpoData, TpoSession } from '../engine/protocol';
import { errorText } from '../lib/api';
import { formatPrice } from '../lib/format';
import { tfName } from '../lib/timeframes';
import type { TpoPanelState } from '../workspace/model';
import { marketOf, patchPanel, setPanelTimeframe, timeframeOf } from '../workspace/store';
import { hasProfile, MarketButton, SESSION_NAME, TimeframeSelect } from './common';

const ROW_H = 11;
const CHAR_W = 8;
const LABEL_H = 22;
const GAP = 14;
const IB_W = 6;
const FONT = '10px ui-monospace, SFMono-Regular, Menlo, Consolas, monospace';
const LETTERS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz';
const POC = '#ffb03a';
const VALUE_AREA = '#4fd1c5';

interface Box {
  minIndex: number;
  maxIndex: number;
  rows: number;
  columns: { x: number; width: number }[];
  width: number;
  height: number;
}

function layout(sessions: readonly TpoSession[]): Box | null {
  let minIndex = Number.POSITIVE_INFINITY;
  let maxIndex = Number.NEGATIVE_INFINITY;
  for (const s of sessions) {
    for (const r of s.rows) {
      minIndex = Math.min(minIndex, r.index);
      maxIndex = Math.max(maxIndex, r.index);
    }
  }
  if (!Number.isFinite(minIndex)) return null;
  const columns: { x: number; width: number }[] = [];
  let x = 0;
  for (const s of sessions) {
    let widest = 1;
    for (const r of s.rows) widest = Math.max(widest, r.periods.length);
    const width = Math.max(IB_W + widest * CHAR_W + 4, 46);
    columns.push({ x, width });
    x += width + GAP;
  }
  const rows = maxIndex - minIndex + 1;
  return { minIndex, maxIndex, rows, columns, width: Math.max(x - GAP, 0), height: rows * ROW_H + LABEL_H };
}

function prepare(canvas: HTMLCanvasElement, width: number, height: number, background: string): CanvasRenderingContext2D | null {
  const dpr = devicePixelRatio || 1;
  canvas.width = Math.ceil(width * dpr);
  canvas.height = Math.ceil(height * dpr);
  canvas.style.width = `${width}px`;
  canvas.style.height = `${height}px`;
  const ctx = canvas.getContext('2d');
  if (!ctx) return null;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.fillStyle = background;
  ctx.fillRect(0, 0, width, height);
  ctx.font = FONT;
  ctx.textBaseline = 'middle';
  return ctx;
}

function palette() {
  const style = getComputedStyle(document.documentElement);
  const token = (name: string, fallback: string) => style.getPropertyValue(name).trim() || fallback;
  return {
    bg: token('--surface', '#0d100e'),
    text: token('--text', '#dfe6e0'),
    dim: token('--muted', '#7c847d'),
    border: token('--border', '#232823'),
    single: token('--bear', '#ff4d5f'),
    ib: token('--info', '#5ab6ff'),
  };
}

const rowOf = (data: TpoData, price: number): number => Math.floor((price - data.gridLow) / data.rowSize);

function sessionLabel(start: number, session: string): string {
  const iso = new Date(start).toISOString();
  return session === '1M' ? iso.slice(0, 7) : iso.slice(5, 10);
}

function draw(profile: HTMLCanvasElement, axis: HTMLCanvasElement, data: TpoData, box: Box): void {
  const c = palette();
  const ctx = prepare(profile, box.width, box.height, c.bg);
  if (!ctx) return;
  const yOf = (index: number) => (box.maxIndex - index) * ROW_H;

  data.sessions.forEach((s, i) => {
    const col = box.columns[i]!;
    const vaLow = rowOf(data, s.val);
    const vaHigh = rowOf(data, s.vah - data.rowSize / 2);
    const pocRow = rowOf(data, s.poc);
    const singles = new Set(s.singlePrints);
    for (const row of s.rows) {
      const y = yOf(row.index);
      const inValue = row.index >= vaLow && row.index <= vaHigh;
      if (inValue) {
        ctx.fillStyle = 'rgba(79, 209, 197, 0.10)';
        ctx.fillRect(col.x, y, col.width, ROW_H);
      }
      if (row.index === pocRow) {
        ctx.fillStyle = 'rgba(255, 176, 58, 0.20)';
        ctx.fillRect(col.x, y, col.width, ROW_H);
      }
      // One string per row: thousands of per-letter calls stutter.
      let text = '';
      for (const p of row.periods) text += LETTERS[p % LETTERS.length];
      ctx.fillStyle = singles.has(row.index) ? c.single : row.index === pocRow ? POC : inValue ? c.text : c.dim;
      ctx.fillText(text, col.x + IB_W, y + ROW_H / 2);
    }
    if (s.ib) {
      const top = yOf(rowOf(data, Math.max(s.ib[0], s.ib[1])));
      const bottom = yOf(rowOf(data, Math.min(s.ib[0], s.ib[1]))) + ROW_H;
      ctx.fillStyle = c.ib;
      ctx.fillRect(col.x, top, 2, bottom - top);
    }
    ctx.save();
    ctx.beginPath();
    ctx.rect(col.x - GAP / 2, box.height - LABEL_H, col.width + GAP, LABEL_H);
    ctx.clip();
    ctx.fillStyle = c.dim;
    ctx.fillText(sessionLabel(s.start, data.session), col.x, box.height - LABEL_H / 2);
    ctx.restore();
  });

  const highest = formatPrice(data.gridLow + box.maxIndex * data.rowSize, data.precision);
  const axisWidth = Math.ceil(Math.max(72, ctx.measureText(`VAH ${highest}`).width + 16));
  const actx = prepare(axis, axisWidth, box.height, c.bg);
  if (!actx) return;
  actx.strokeStyle = c.border;
  actx.beginPath();
  actx.moveTo(0.5, 0);
  actx.lineTo(0.5, box.height - LABEL_H);
  actx.stroke();
  const step = Math.max(1, Math.round(box.rows / 14));
  actx.fillStyle = c.dim;
  for (let index = box.minIndex; index <= box.maxIndex; index += step) {
    actx.fillText(formatPrice(data.gridLow + index * data.rowSize, data.precision), 8, yOf(index) + ROW_H / 2);
  }
  // Only the newest session's levels: the others describe sessions scrolled past.
  const last = data.sessions[data.sessions.length - 1];
  if (!last) return;
  for (const m of [
    { price: last.vah, color: VALUE_AREA, label: 'VAH' },
    { price: last.poc, color: POC, label: 'POC' },
    { price: last.val, color: VALUE_AREA, label: 'VAL' },
  ]) {
    const y = yOf(rowOf(data, m.price)) + ROW_H / 2;
    actx.fillStyle = c.bg;
    actx.fillRect(1, y - 5, axisWidth - 1, 10);
    actx.fillStyle = m.color;
    actx.fillRect(1, y - 5, 3, 10);
    actx.fillText(`${m.label} ${formatPrice(m.price, data.precision)}`, 8, y);
  }
}

function TpoCanvas(props: { data: TpoData }) {
  let scroller!: HTMLDivElement;
  let profile!: HTMLCanvasElement;
  let axis!: HTMLCanvasElement;
  let placed = '';
  createEffect(() => {
    const data = props.data;
    const box = layout(data.sessions);
    if (!box) return;
    draw(profile, axis, data, box);
    // Open on the newest session around its point of control — once per
    // window, not on every refresh the reader has scrolled away from.
    const key = `${data.session}|${data.sessions[0]?.start}|${data.rowSize}`;
    if (key === placed) return;
    placed = key;
    const last = data.sessions[data.sessions.length - 1]!;
    scroller.scrollLeft = scroller.scrollWidth;
    scroller.scrollTop = Math.max(0, (box.maxIndex - rowOf(data, last.poc)) * ROW_H - scroller.clientHeight / 2);
  });
  return (
    <div ref={scroller} class="tpo-scroll">
      <div class="tpo-inner">
        <canvas ref={profile} />
        <canvas ref={axis} class="tpo-axis" />
      </div>
    </div>
  );
}

export function TpoPanel(props: { panel: TpoPanelState }) {
  const id = props.panel.id;
  const market = () => marketOf(props.panel);
  const timeframe = () => timeframeOf(props.panel);
  const [data, setData] = createSignal<TpoData | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [loading, setLoading] = createSignal(false);

  let request = 0;
  const load = async () => {
    const req = ++request;
    if (!hasProfile(timeframe())) {
      batch(() => {
        setData(null);
        setLoading(false);
      });
      return;
    }
    setLoading(true);
    try {
      const d = await call('tpo', { market: market(), timeframe: timeframe(), sessions: props.panel.sessions });
      if (req === request) {
        batch(() => {
          setData(d);
          setError(null);
        });
      }
    } catch (e) {
      if (req === request) setError(errorText(e));
    } finally {
      if (req === request) setLoading(false);
    }
  };

  createEffect(
    on([market, timeframe, () => props.panel.sessions], () => {
      setData(null);
      void load();
    }),
  );
  const timer = setInterval(() => void load(), 60_000);
  onCleanup(() => clearInterval(timer));

  const emptyText = () => {
    if (error()) return `▲ ${error()}`;
    if (!hasProfile(timeframe())) return `${tfName(timeframe())} has no TPO — profiles are built for intraday, daily and weekly sessions.`;
    if (loading() && !data()) return 'Building TPO profiles…';
    return `▲ ${data()?.reason ?? 'no session had enough brackets to profile'}`;
  };

  return (
    <div class="panelcol">
      <div class="toolbar">
        <MarketButton panelId={id} market={market()} />
        <span class="sep" />
        <TimeframeSelect value={timeframe()} allow={hasProfile} onChange={(tf) => setPanelTimeframe(id, tf)} />
        <span class="sep" />
        <span class="caps">Sessions&nbsp;</span>
        <select title="Sessions drawn" onChange={(e) => patchPanel(id, { sessions: Number(e.currentTarget.value) })}>
          {[6, 12, 24, 36].map((n) => (
            <option value={n} selected={n === props.panel.sessions}>
              {n}
            </option>
          ))}
        </select>
        <span class="spacer" />
        <Show when={data()}>
          {(d) => (
            <span class="dim">
              {SESSION_NAME[d().session] ?? d().session} · one letter per {d().from} bracket
            </span>
          )}
        </Show>
        <Show when={loading()}>
          <span class="dim pulse">&nbsp;●</span>
        </Show>
      </div>
      <Show when={(data()?.sessions.length ?? 0) > 0 && data()} fallback={<div class="empty" classList={{ warn: !!error() || (!!data() && !loading()) }}>{emptyText()}</div>}>
        {(d) => (
          <>
            <TpoCanvas data={d()} />
            <div class="tpo-legend">
              <span>
                <span class="muted">A, B, C…</span> one {d().from} bracket each
              </span>
              <span>
                <span style={{ color: POC }}>█</span> POC
              </span>
              <span>
                <span style={{ color: VALUE_AREA }}>█</span> value area 70%
              </span>
              <span>
                <span class="bear">█</span> single prints
              </span>
              <span>
                <span style={{ color: 'var(--info)' }}>▌</span> initial balance
              </span>
              <span class="spacer" />
              <span class="num">
                {d().sessions.length} sessions · row {formatPrice(d().rowSize, d().precision)}
              </span>
            </div>
          </>
        )}
      </Show>
    </div>
  );
}
