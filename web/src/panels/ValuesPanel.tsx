/**
 * A market at a glance, for deciding rather than for checking the engine: where
 * price sits in its day and against its usual range, how it has done over the
 * weeks, momentum on every timeframe at once, the levels around it with their
 * distance, and — for a perpetual — open interest and liquidations.
 *
 * Every indicator is computed on closed bars by the same Rust engine that draws
 * the chart, with TradingView's defaults (RSI 14, StochRSI 14/14/3/3, UTC days).
 */
import { createEffect, createMemo, createSignal, For, on, onCleanup, Show, type JSX } from 'solid-js';
import { call } from '../engine/client';
import { coinOf } from '../engine/open-interest';
import type { DailyBars, IndicatorValues } from '../engine/protocol';
import { api, errorText, type LiquidationStats } from '../lib/api';
import { watchDay, type Day } from '../lib/day-feed';
import { formatPct, formatPrice, formatTurnover, formatVolume } from '../lib/format';
import { serverTicker } from '../lib/liquidation-venues';
import { isPerp, symbolOf } from '../lib/markets';
import { atr, DAY, performance, priorLevels, type Level } from '../lib/snapshot';
import { tfLabel } from '../lib/timeframes';
import type { ValuesPanelState } from '../workspace/model';
import { marketOf, patchPanel, setPanelTimeframe, timeframeOf } from '../workspace/store';
import { MarketButton, SESSION_NAME, TimeframeSelect } from './common';

/** Momentum is read on these, shortest first. */
const FRAMES = ['15m', '1h', '4h', '1d', '1w'];
/** Enough bars for EMA 200 and RSI to settle, without paging far back. */
const FRAME_BARS = 600;
const REFRESH_MS = 60_000;
const DAYS = 400;

interface Frame {
  tf: string;
  v: IndicatorValues | null;
}

interface OpenInterest {
  coins: number;
  change: number | null;
}

/** Runs `load` now, whenever `key` changes, and every `ms`; stale answers are dropped. */
function poll<K, T>(key: () => K, load: (key: K) => Promise<T>, ms: number): [() => T | null, () => string | null] {
  const [value, setValue] = createSignal<T | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  let req = 0;
  const run = (k: K) => {
    const mine = ++req;
    load(k).then(
      (v) => {
        if (mine !== req) return;
        setValue(() => v);
        setError(null);
      },
      (e) => mine === req && setError(errorText(e)),
    );
  };
  createEffect(
    on(key, (k) => {
      setValue(null);
      run(k);
    }),
  );
  const timer = setInterval(() => run(key()), ms);
  onCleanup(() => clearInterval(timer));
  return [value, error];
}

export function ValuesPanel(props: { panel: ValuesPanelState }) {
  const id = props.panel.id;
  const market = () => marketOf(props.panel);
  const timeframe = () => timeframeOf(props.panel);

  const [day, setDay] = createSignal<Day | null>(null);
  createEffect(
    on(market, (m) => {
      setDay(null);
      onCleanup(watchDay(m, setDay));
    }),
  );

  const [daily, dailyError] = poll(market, (m) => call('daily', { market: m, days: DAYS }), 5 * REFRESH_MS);
  const [frames] = poll(
    market,
    (m) => Promise.all(FRAMES.map((tf) => call('values', { market: m, timeframe: tf, basis: 'volume', profile: false, bars: FRAME_BARS }).then((v): Frame => ({ tf, v }), (): Frame => ({ tf, v: null })))),
    REFRESH_MS,
  );
  const [session] = poll(() => `${market()}|${timeframe()}|${props.panel.basis}`, () => call('values', { market: market(), timeframe: timeframe(), basis: props.panel.basis }), REFRESH_MS);
  const [liquidations] = poll(market, (m) => (isPerp(m) ? api.liquidations(serverTicker(symbolOf(m))) : Promise.resolve(null)), REFRESH_MS);
  const [openInterest] = poll(market, (m) => (isPerp(m) ? loadOpenInterest(coinOf(symbolOf(m))) : Promise.resolve(null)), REFRESH_MS);

  const d = (): DailyBars | null => daily();
  const digits = () => d()?.precision ?? session()?.precision ?? 2;
  const price = () => day()?.close ?? (d() && d()!.close.length ? d()!.close[d()!.close.length - 1]! : null);
  const today = () => day()?.openTime ?? Math.floor(Date.now() / DAY) * DAY;
  const px = (v: number) => formatPrice(v, digits());

  const dayAtr = createMemo(() => (d() ? atr(d()!) : null));

  const levels = createMemo(() => {
    const out: Level[] = [];
    if (day()) out.push({ label: 'Today open', price: day()!.open });
    if (d()) out.push(...priorLevels(d()!, today()));
    const p = session()?.profile;
    if (p && p.covered > 0) {
      const name = SESSION_NAME[p.session] ?? p.session;
      for (const [label, v] of [['VAH', p.vah], ['POC', p.poc], ['VAL', p.val]] as const) if (v !== null) out.push({ label: `${label} · prior ${name}`, price: v });
    }
    for (const f of frames() ?? []) if ((f.tf === '4h' || f.tf === '1d') && f.v?.ema200 != null) out.push({ label: `EMA 200 · ${tfLabel(f.tf)}`, price: f.v.ema200 });
    return out.sort((a, b) => b.price - a.price);
  });

  return (
    <div class="panelcol">
      <div class="toolbar">
        <MarketButton panelId={id} market={market()} />
      </div>
      <div class="scroll vx">
        <Show when={price() !== null} fallback={<div class="empty" classList={{ warn: !!dailyError() }}>{dailyError() ? `▲ ${dailyError()}` : 'Loading…'}</div>}>
          {/* ------------------------------------------------ today */}
          <div class="vx-price">
            <span class="num big">{px(price()!)}</span>
            <Show when={day()}>
              {(t) => {
                const chg = () => ((t().close - t().open) / t().open) * 100;
                return (
                  <span class="num" classList={{ bull: chg() > 0, bear: chg() < 0 }} data-tip="Since 00:00 UTC, as TradingView counts the day">
                    {formatPct(chg())} today
                  </span>
                );
              }}
            </Show>
            <span class="spacer" />
            <Show when={day()}>{(t) => <span class="dim num" data-tip="Volume since 00:00 UTC">Vol {formatVolume(t().volume)}</span>}</Show>
          </div>
          <Show when={day()}>
            {(t) => {
              const span = () => t().high - t().low;
              const at = () => (span() > 0 ? ((price()! - t().low) / span()) * 100 : 50);
              const used = () => (dayAtr() ? (span() / dayAtr()!) * 100 : null);
              return (
                <div class="vx-range">
                  <div class="vx-bar">
                    <span class="fill" style={{ width: `${at()}%` }} />
                    <span class="mark" style={{ left: `${at()}%` }} />
                  </div>
                  <div class="vx-legend num">
                    <span>L {px(t().low)}</span>
                    <span>H {px(t().high)}</span>
                  </div>
                  <Show when={used() !== null}>
                    <div class="vx-atr" data-tip="Today's high-to-low range against the average daily range of the last 14 days">
                      Day range {px(t().high - t().low)} · <b classList={{ warn: used()! >= 100 }}>{used()!.toFixed(0)}%</b> of ATR 14D {px(dayAtr()!)}
                    </div>
                  </Show>
                </div>
              );
            }}
          </Show>

          {/* ------------------------------------------------ performance */}
          <Show when={d()}>
            {(dd) => (
              <div class="vx-perf">
                <For each={performance(dd(), today(), price()!)}>
                  {(p) => (
                    <div classList={{ bull: (p.pct ?? 0) > 0, bear: (p.pct ?? 0) < 0 }}>
                      <span class="caps">{p.label}</span>
                      <span class="num">{p.pct === null ? '—' : formatPct(p.pct)}</span>
                    </div>
                  )}
                </For>
              </div>
            )}
          </Show>

          {/* ------------------------------------------------ momentum */}
          <div class="subhead">
            <span class="caps">Momentum</span>
            <span class="dim">last closed bar</span>
          </div>
          <table class="vx-table">
            <thead>
              <tr>
                <th />
                <th data-tip="RSI 14 — above 70 overbought, below 30 oversold">RSI</th>
                <th data-tip="Stochastic RSI %K, 14/14/3/3 — above 80 overbought, below 20 oversold">Stoch</th>
                <th data-tip="Close against EMA 200: above is an uptrend">EMA 200</th>
                <th data-tip="EMA 12 over EMA 25 is bullish, under it bearish">12/25</th>
              </tr>
            </thead>
            <tbody>
              <For each={frames() ?? FRAMES.map((tf): Frame => ({ tf, v: null }))}>
                {(f) => {
                  const trend = () => (f.v?.ema200 ? ((f.v.close - f.v.ema200) / f.v.ema200) * 100 : null);
                  const cross = () => (f.v?.ema12 != null && f.v.ema25 != null ? f.v.ema12 > f.v.ema25 : null);
                  return (
                    <tr>
                      <td class="tf">{tfLabel(f.tf)}</td>
                      <td class="num">{zone(f.v?.rsi14 ?? null, 30, 70)}</td>
                      <td class="num">{zone(f.v?.stochK ?? null, 20, 80)}</td>
                      <td class="num" classList={{ bull: (trend() ?? 0) > 0, bear: (trend() ?? 0) < 0 }}>
                        {trend() === null ? <span class="dim">{frames() ? '—' : '…'}</span> : formatPct(trend())}
                      </td>
                      <td classList={{ bull: cross() === true, bear: cross() === false }}>{cross() === null ? <span class="dim">—</span> : cross() ? '▲ bull' : '▼ bear'}</td>
                    </tr>
                  );
                }}
              </For>
            </tbody>
          </table>

          {/* ------------------------------------------------ levels */}
          <Show when={levels().length > 0}>
            <div class="subhead vx-levelhead">
              <span class="caps">Levels</span>
              <span class="spacer" />
              <span class="dim" data-tip="The value area of the prior session of this timeframe's profile">
                profile
              </span>
              <TimeframeSelect value={timeframe()} onChange={(tf) => setPanelTimeframe(id, tf)} />
              <button class="tb" classList={{ on: props.panel.basis === 'volume' }} data-tip="Value area weighted by volume traded at each price" onClick={() => patchPanel(id, { basis: 'volume' })}>
                Vol
              </button>
              <button class="tb" classList={{ on: props.panel.basis === 'tpo' }} data-tip="Value area weighted by time spent at each price" onClick={() => patchPanel(id, { basis: 'tpo' })}>
                TPO
              </button>
            </div>
            <table class="vx-table vx-levels">
              <tbody>
                <For each={withPrice(levels(), price()!)}>
                  {(l) =>
                    l === null ? (
                      <tr class="here">
                        <td>▸ Price</td>
                        <td class="num">{px(price()!)}</td>
                        <td />
                      </tr>
                    ) : (
                      <tr>
                        <td>{l.label}</td>
                        <td class="num">{px(l.price)}</td>
                        <td class="num" classList={{ bull: l.price > price()!, bear: l.price < price()! }}>
                          {formatPct(((l.price - price()!) / price()!) * 100)}
                        </td>
                      </tr>
                    )
                  }
                </For>
              </tbody>
            </table>
          </Show>

          {/* ------------------------------------------------ derivatives */}
          <Show when={isPerp(market())}>
            <div class="subhead">
              <span class="caps">Derivatives</span>
              <span class="dim">all exchanges</span>
            </div>
            <Show when={openInterest()}>
              {(oi) => (
                <div class="vx-row">
                  <span class="caps">Open interest</span>
                  <span class="num">
                    {formatVolume(oi().coins)} {coinOf(symbolOf(market()))} · ${formatTurnover(oi().coins * price()!)}
                  </span>
                  <span class="num" classList={{ bull: (oi().change ?? 0) > 0, bear: (oi().change ?? 0) < 0 }} data-tip="Change over the last 24 hours">
                    {oi().change === null ? '' : `${formatPct(oi().change)} 24h`}
                  </span>
                </div>
              )}
            </Show>
            <Liquidations stats={liquidations()} />
          </Show>
        </Show>
      </div>
    </div>
  );
}

/** A reading coloured where it is stretched: low as bullish, high as bearish. */
function zone(v: number | null, low: number, high: number): JSX.Element {
  if (v === null) return <span class="dim">—</span>;
  return <span classList={{ bull: v <= low, bear: v >= high }}>{v.toFixed(1)}</span>;
}

/** Levels high to low, with null where price sits among them. */
function withPrice(levels: Level[], price: number): (Level | null)[] {
  const at = levels.findIndex((l) => l.price < price);
  const out: (Level | null)[] = [...levels];
  out.splice(at === -1 ? out.length : at, 0, null);
  return out;
}

function Liquidations(props: { stats: LiquidationStats | null }) {
  return (
    <Show when={props.stats}>
      {(s) => {
        const st = s();
        if (!st.tracked) return <div class="note">Liquidation totals are kept for {st.tickers.join(', ')} only.</div>;
        return (
          <table class="vx-table">
            <thead>
              <tr>
                <th>Liquidated</th>
                <th>Longs</th>
                <th>Shorts</th>
              </tr>
            </thead>
            <tbody>
              <For each={st.windows.filter((w) => w.window !== '12h')}>
                {(w) => (
                  <tr>
                    <td class="tf">{w.window}</td>
                    <td class="num bear">${formatTurnover(w.longUsd)}</td>
                    <td class="num bull">${formatTurnover(w.shortUsd)}</td>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        );
      }}
    </Show>
  );
}

/** The coin's open interest over every exchange the server records, now and 24 hours back. */
async function loadOpenInterest(coin: string): Promise<OpenInterest | null> {
  const hour = 3_600_000;
  const from = Date.now() - 25 * hour;
  const res = await fetch(`/api/open-interest?coin=${encodeURIComponent(coin)}&step=${hour}&from=${from}`);
  if (!res.ok) return null;
  const r = (await res.json()) as { time: number[]; open: number[]; close: number[] };
  const n = r.time.length;
  if (n === 0) return null;
  const back = r.time.findIndex((t) => t >= Date.now() - 24 * hour);
  // Only a full day's history makes a 24-hour change.
  const then = back > 0 ? r.open[back] : null;
  const now = r.close[n - 1]!;
  return { coins: now, change: then ? ((now - then) / then) * 100 : null };
}
