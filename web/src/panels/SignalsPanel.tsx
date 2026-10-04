/**
 * What the server's screener found: three drives on four-hour bars, either
 * way, among the two hundred busiest perpetuals on Binance and Bybit — those
 * whose Stochastic RSI has turned first, then those still waiting for it.
 *
 * A row opens its market on the chart at four hours, with the pattern drawn
 * on it until it is closed or kept (ChartPanel). Telegram, when this browser
 * asks, hears of each pattern once, when it turns.
 *
 * The record beside the list says how they went: the same rules replayed over
 * months of history, the live patterns since the screener began, and the
 * latest of those one by one — as trades with three take-profits and a stop.
 */
import { batch, createEffect, createSignal, For, onCleanup, Show } from 'solid-js';
import { setShownPattern, shownPattern } from '../app/ui';
import { api, errorText, type Signal, type SignalGroup, type SignalOutcome, type SignalPlanRow, type SignalPoint, type SignalsState, type SignalStats, type SignalTouchGroup } from '../lib/api';
import { useDismiss } from '../lib/dismiss';
import { VenueTag } from '../lib/exchanges';
import { formatPrice } from '../lib/format';
import { Icon } from '../lib/icons';
import { symbolOf } from '../lib/markets';
import { formatDateTime, formatHm } from '../lib/timezone';
import { toast } from '../lib/toasts';
import type { SignalsPanelState } from '../workspace/model';
import { patchPanel, setPanelMarket, setPanelTimeframe, targetChart } from '../workspace/store';

/** How often the list is asked for again; the scan itself runs every four hours. */
const POLL_MS = 60_000;
const SIDES = [
  { value: 'all', label: 'All', tip: 'Both ways' },
  { value: 'bear', label: 'Bear', tip: 'Three higher highs, then Stochastic RSI turning down from over 80' },
  { value: 'bull', label: 'Bull', tip: 'Three lower lows, then Stochastic RSI turning up from under 20' },
] as const;

/** A price to five figures: a pattern's levels carry no exchange's tick. */
const price = (v: number) => formatPrice(v, Math.min(8, Math.max(2, 4 - Math.floor(Math.log10(Math.abs(v) || 1)))));
/** `12m`, `5h`, `2d`: how long since. */
const since = (at: number, now: number) => {
  const minutes = Math.max(0, Math.floor((now - at) / 60_000));
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  return hours < 48 ? `${hours}h` : `${Math.floor(hours / 24)}d`;
};
const FOUR_HOURS = 4 * 3_600_000;
/** Any pattern a row can open: the list's, or the record's. */
interface Openable {
  market: string;
  side: 'bear' | 'bull';
  drives: SignalPoint[];
  score: number;
  divergence: boolean;
  stage?: 'line';
  levels?: { tp: number[]; sl: number } | null;
  enteredAt?: number;
}
/** A touch as a pattern to draw: its two drives, and the third where price reached the line, on that bar. */
const touchPattern = (t: { market: string; side: 'bear' | 'bull'; drives: SignalPoint[]; touchedAt: number; touchPrice: number; levels?: { tp: number[]; sl: number } | null }): Openable => ({
  market: t.market,
  side: t.side,
  drives: [...t.drives, { at: Math.floor(t.touchedAt / FOUR_HOURS) * FOUR_HOURS, price: t.touchPrice }],
  score: 0,
  divergence: false,
  stage: 'line',
  levels: t.levels,
  enteredAt: Math.floor(t.touchedAt / FOUR_HOURS) * FOUR_HOURS,
});
const isShown = (s: Openable) => {
  const p = shownPattern();
  return p !== null && p.market === s.market && p.drives[2]?.at === s.drives[2]?.at;
};
/** Whether a played pattern has a move to show: decided, or open with bars since. */
const moved = (s: { outcome: SignalOutcome | null; best: number | null; worst: number | null }) => s.outcome !== null || (s.best ?? 0) !== 0 || (s.worst ?? 0) !== 0;
const pct = (v: number | null | undefined) => (v === null || v === undefined ? '—' : `${v > 0 ? '+' : ''}${v.toFixed(1)}%`);
const share = (part: number, n: number) => (n === 0 ? '—' : `${Math.round((part / n) * 100)}%`);
const BECAME: Record<string, string> = { turned: 'became three drives, turned', pattern: 'became three drives', none: 'no third drive' };
const GROUP_LABEL: Record<SignalGroup['group'], string> = { all: 'All', '70': 'Score 70+', '50': 'Score 50–69', '0': 'Score under 50', div: 'RSI divergence' };
const OUTCOME: Record<SignalOutcome, [string, string, string]> = {
  tp3: ['✓', 'TP3', 'All three take-profits'],
  tp2: ['✓', 'TP2', 'Two take-profits, the rest out at the entry or after four days'],
  tp1: ['✓', 'TP1', 'The first take-profit, the rest out at the entry or after four days'],
  stop: ['✗', 'Stop', 'Went past the third drive first'],
  expired: ['–', 'Neither', 'Neither in four days'],
};
/** The trade's prices, for a tooltip. */
const levelsText = (lv: { tp: number[]; sl: number } | null | undefined) =>
  lv ? `TP ${lv.tp.map(price).join(' / ')} · SL ${price(lv.sl)}` : '';
/** The trade the record counts, said once. */
const TRADE = 'TP 3 / 6 / 9% from the entry, SL 3.5% past the third drive · half off at TP1 and the stop to the entry, a quarter at each of the others · four days';

export function SignalsPanel(props: { panel: SignalsPanelState }) {
  const [state, setState] = createSignal<SignalsState | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [now, setNow] = createSignal(Date.now());

  const load = async () => {
    try {
      setState(await api.signals());
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
    setNow(Date.now());
  };
  void load();
  const timer = setInterval(() => void load(), POLL_MS);
  onCleanup(() => clearInterval(timer));

  const signals = () => (state()?.signals ?? []).filter((s) => props.panel.side === 'all' || s.side === props.panel.side);
  const ended = () => (state()?.ended ?? []).filter((s) => props.panel.side === 'all' || s.side === props.panel.side);
  const touches = () => (state()?.touches ?? []).filter((s) => props.panel.side === 'all' || s.side === props.panel.side);
  const [telegramMenu, setTelegramMenu] = createSignal(false);
  useDismiss(telegramMenu, () => setTelegramMenu(false));
  /** Why a pattern left the list: how it played out, or that it had been news for two days. */
  const why = (s: Signal): string => {
    if (s.outcome === 'stop') return 'hit the stop';
    if (s.outcome === 'tp3' || s.outcome === 'tp2' || s.outcome === 'tp1') return `reached ${s.outcome.toUpperCase()}`;
    if (s.outcome === 'expired') return 'neither in four days';
    return s.turnedAt !== null && s.lastSeen - s.turnedAt >= 44 * 3_600_000 ? 'two days since it turned' : 'no longer three drives';
  };
  const sides = (): ('bear' | 'bull')[] => (props.panel.side === 'all' ? ['bear', 'bull'] : [props.panel.side]);

  // The record, read when it is looked at and again with the list.
  const [stats, setStats] = createSignal<SignalStats | null>(null);
  const loadStats = async () => {
    try {
      setStats(await api.signalStats());
    } catch (e) {
      setError(errorText(e));
    }
  };
  createEffect(() => {
    if (props.panel.view !== 'record') return;
    void loadStats();
    const again = setInterval(() => void loadStats(), POLL_MS);
    onCleanup(() => clearInterval(again));
  });
  const recent = () => (stats()?.recent ?? []).filter((s) => props.panel.side === 'all' || s.side === props.panel.side);

  /** The market on the chart this panel drives, at four hours, with the pattern drawn on it. */
  const open = (s: Openable) => {
    const target = props.panel.link ? props.panel : targetChart();
    if (!target) return;
    batch(() => {
      setShownPattern({ market: s.market, side: s.side, drives: s.drives, score: s.score, divergence: s.divergence, stage: s.stage, levels: s.levels ?? undefined, enteredAt: s.enteredAt });
      setPanelMarket(target.id, s.market);
      setPanelTimeframe(target.id, '4h');
    });
  };

  const setNotify = async (patch: { notify?: boolean; touch?: boolean }) => {
    try {
      await api.signalsNotify(patch);
      await load();
      if ((patch.notify || patch.touch) && !state()?.linked) toast({ tone: 'warn', title: 'No Telegram chat linked', body: 'Link one in Settings → Telegram, and these will reach it.' });
    } catch (e) {
      toast({ tone: 'warn', title: 'Not saved', body: errorText(e) });
    }
  };

  return (
    <div class="panelcol sig">
      <div class="toolbar">
        <span class="caps" style={{ padding: '0 4px' }}>
          Signal
        </span>
        <span class="dim what">Three drives · 4h</span>
        <span class="sep" />
        <button class="tb" classList={{ on: props.panel.view !== 'record' }} data-tip="The patterns now" onClick={() => patchPanel(props.panel.id, { view: 'list' })}>
          Signals
        </button>
        <button class="tb" classList={{ on: props.panel.view === 'record' }} data-tip="How they went: the rules replayed over months of history, and the live patterns since" onClick={() => patchPanel(props.panel.id, { view: 'record' })}>
          Record
        </button>
        <span class="sep" />
        <For each={SIDES}>
          {(s) => (
            <button class="tb" classList={{ on: props.panel.side === s.value }} data-tip={s.tip} onClick={() => patchPanel(props.panel.id, { side: s.value })}>
              {s.label}
            </button>
          )}
        </For>
        <span class="spacer" />
        <Show when={state()?.telegram}>
          <button
            class="tb"
            classList={{ on: state()?.notify || state()?.notifyTouch }}
            data-tip="What reaches you on Telegram"
            onPointerDown={(e) => e.stopPropagation()}
            onClick={() => setTelegramMenu((v) => !v)}
          >
            <Icon name="alarm" size={12} /> Telegram
          </button>
        </Show>
      </div>
      <Show when={telegramMenu()}>
        <div class="menu sig-tg" onPointerDown={(e) => e.stopPropagation()}>
          <button classList={{ cur: state()?.notify }} data-tip="Once per scan, the patterns whose Stochastic RSI turned" onClick={() => void setNotify({ notify: !state()?.notify })}>
            <span>When a pattern turns</span>
            <span class="num">{state()?.notify ? 'on' : 'off'}</span>
          </button>
          <button classList={{ cur: state()?.notifyTouch }} data-tip="At once, when live price reaches the line of two drives — before the bars confirm a third" onClick={() => void setNotify({ touch: !state()?.notifyTouch })}>
            <span>When price reaches the line</span>
            <span class="num">{state()?.notifyTouch ? 'on' : 'off'}</span>
          </button>
        </div>
      </Show>
      <Show when={props.panel.view === 'record'}>
        <Record stats={stats()} sides={sides()} recent={recent()} error={error()} onOpen={open} />
      </Show>
      <Show when={props.panel.view !== 'record'}>
      <div class="sig-sub">
        <Show when={state()?.scannedAt} fallback={<span>{error() ?? 'The first scan runs a minute after the next four-hour close.'}</span>}>
          {(at) => (
            <span>
              Scanned {formatHm(at())} · next {formatHm(state()!.nextAt)} · {state()!.markets} coins
            </span>
          )}
        </Show>
        <span class="dim">{signals().length} found</span>
      </div>
      <div class="scroll">
        <For each={touches()}>
          {(t) => {
            const p = touchPattern(t);
            return (
              <button
                class="sig-row"
                classList={{ shown: isShown(p) }}
                data-tip={`Price reached the line through two drives at ${price(t.touchPrice)} — a third drive as it happens, not yet confirmed by closed bars. ${levelsText(t.levels)}`}
                onClick={() => open(p)}
              >
                <span class={`sig-state ${t.side} line`}>
                  {t.side === 'bear' ? '▼' : '▲'}
                  <span class="word"> AT LINE</span>
                </span>
                <span class="sig-coin">
                  <b>{symbolOf(t.market)}</b>
                  <VenueTag market={t.market} />
                </span>
                <span class="sig-drives num">{p.drives.map((d) => price(d.price)).join(' → ')}</span>
                <span class="sig-score num dim">—</span>
                <span class="sig-age num dim">{since(t.touchedAt, now())}</span>
              </button>
            );
          }}
        </For>
        <For each={signals()} fallback={<Show when={state()?.scannedAt}>{<div class="empty">No three drives on four-hour bars right now.</div>}</Show>}>
          {(s) => {
            const turned = () => s.turnedAt !== null;
            return (
              <button
                class="sig-row"
                classList={{ shown: isShown(s) }}
                data-tip={`Drives at ${s.drives.map((d) => price(d.price)).join(', ')} · ${turned() ? `Stochastic RSI turned at ${price(s.turnPrice ?? s.price)}` : 'waiting for Stochastic RSI to turn'}${s.divergence ? ' · RSI weaker at the third drive' : ''}${s.levels ? ` · ${levelsText(s.levels)}` : ''} — open it on the chart`}
                onClick={() => open({ ...s, enteredAt: s.turnedAt ?? undefined })}
              >
                <span class={`sig-state ${s.side}`} classList={{ turned: turned() }}>
                  {s.side === 'bear' ? '▼' : '▲'}
                  <span class="word"> {turned() ? 'TRIGGERED' : 'FORMING'}</span>
                </span>
                <span class="sig-coin">
                  <b>{symbolOf(s.market)}</b>
                  <VenueTag market={s.market} />
                </span>
                <span class="sig-drives num">{s.drives.map((d) => price(d.price)).join(' → ')}</span>
                <span class="sig-score num">
                  <Show when={s.outcome}>{(o) => <span class={`sig-out ${o()}`} data-tip={`${OUTCOME[o()][2]} · ${pct(s.ret)}`}>{OUTCOME[o()][0]}</span>}</Show>
                  <Show when={s.divergence}>
                    <span class="div" data-tip="RSI weaker at the third drive than at the second">DIV</span>
                  </Show>
                  {s.score}
                </span>
                <span class="sig-age num dim">{since(s.turnedAt ?? s.drives[2]!.at, now())}</span>
              </button>
            );
          }}
        </For>
        <Show when={ended().length > 0}>
          <div class="sig-cap">
            <b>Ended</b>
            <span class="dim">left the list in the last day</span>
          </div>
          <For each={ended()}>
            {(s) => (
              <button class="sig-row ended" classList={{ shown: isShown(s) }} data-tip={`${why(s)}${s.ret !== null && s.outcome ? ` · ${pct(s.ret)}` : ''} — open it on the chart`} onClick={() => open({ ...s, enteredAt: s.turnedAt ?? undefined })}>
                <span class={`sig-out ${s.outcome ?? 'gone'}`}>
                  {s.outcome ? OUTCOME[s.outcome][0] : '·'}
                  <span class="word"> {s.outcome ? OUTCOME[s.outcome][1].toUpperCase() : 'ENDED'}</span>
                </span>
                <span class="sig-coin">
                  <span class={s.side}>{s.side === 'bear' ? '▼' : '▲'}</span>
                  <b>{symbolOf(s.market)}</b>
                  <VenueTag market={s.market} />
                </span>
                <span class="sig-drives dim">{why(s)}</span>
                <span class="sig-score num">{s.score}</span>
                <span class="sig-age num dim">{since(s.lastSeen, now())}</span>
              </button>
            )}
          </For>
        </Show>
      </div>
      </Show>
    </div>
  );
}

/** The record: the history replayed, the live patterns since, and the latest one by one. */
/** The plans the replay compares: a letter, the take-profits, the stop. */
const PLANS: Record<SignalPlanRow['plan'], [string, string, string]> = {
  fixed: ['A', 'TP 3/6/9%', 'SL HH+3.5%'],
  fib: ['B', 'TP Fib .382/.618/1', 'SL Fib 1.272'],
  'fib-atr': ['C', 'TP Fib .382/.618/1', 'SL HH+1 ATR'],
  'from-entry': ['D', 'TP 3/6/9%', 'SL entry+3.5%'],
};
const CUTS = [
  { value: 'all', label: 'All' },
  { value: '70', label: 'Score 70+' },
  { value: 'div', label: 'RSI divergence' },
] as const;

/** Each plan replayed over the same patterns and touches, side by side, with both halves of the period. */
function Compare(props: { rows: SignalPlanRow[]; sides: ('bear' | 'bull')[] }) {
  const [cut, setCut] = createSignal<'all' | '70' | 'div'>('all');
  const table = (kind: 'pattern' | 'touch') => (
    <table class="grid sig-table sig-compare">
      <thead>
        <tr>
          <th>Plan</th>
          <th />
          <th class="num">n</th>
          <th class="num">TP1</th>
          <th class="num">TP2</th>
          <th class="num">TP3</th>
          <th class="num">SL</th>
          <th class="num" data-tip="The average result over the whole position">Avg</th>
          <th class="num" data-tip="The average in the first half of the period, and in the second: a plan worth keeping holds up in both">Halves</th>
        </tr>
      </thead>
      <tbody>
        <For each={props.rows.filter((r) => r.kind === kind && r.group === (kind === 'touch' ? 'all' : cut()) && props.sides.includes(r.side))}>
          {(r) => (
            <tr classList={{ head: r.side === props.sides[0] }}>
              <td data-tip={`${PLANS[r.plan][1]} · ${PLANS[r.plan][2]}`}>
                <Show when={r.side === props.sides[0]}>
                  <b>{PLANS[r.plan][0]}</b> <span class="dim">{PLANS[r.plan][1]} · {PLANS[r.plan][2]}</span>
                  {r.plan === 'fixed' ? <span class="badge">live</span> : null}
                </Show>
              </td>
              <td>
                <span class={r.side}>{r.side === 'bear' ? '▼' : '▲'}</span>
              </td>
              <td class="num">{r.n}</td>
              <td class="num bull">{share(r.tp1, r.n)}</td>
              <td class="num bull">{share(r.tp2, r.n)}</td>
              <td class="num bull">{share(r.tp3, r.n)}</td>
              <td class="num bear">{share(r.stop, r.n)}</td>
              <td class="num" classList={{ bull: r.ret > 0, bear: r.ret < 0 }}>{r.n ? pct(r.ret) : '—'}</td>
              <td class="num dim">
                <span classList={{ bull: r.halves[0] > 0, bear: r.halves[0] < 0 }}>{pct(r.halves[0])}</span> / <span classList={{ bull: r.halves[1] > 0, bear: r.halves[1] < 0 }}>{pct(r.halves[1])}</span>
              </td>
            </tr>
          )}
        </For>
      </tbody>
    </table>
  );
  return (
    <>
      <div class="sig-cap">
        <b>Plans compared</b>
        <span class="dim">the same patterns, each way of setting the take-profits and the stop</span>
        <span class="spacer" />
        <For each={CUTS}>
          {(c) => (
            <button class="tb" classList={{ on: cut() === c.value }} onClick={() => setCut(c.value)}>
              {c.label}
            </button>
          )}
        </For>
      </div>
      {table('pattern')}
      <div class="sig-cap">
        <b>Plans compared · at the line</b>
        <span class="dim">entered at the touch; A and D are the same there</span>
      </div>
      {table('touch')}
    </>
  );
}

function Record(props: { stats: SignalStats | null; sides: ('bear' | 'bull')[]; recent: SignalStats['recent']; error: string | null; onOpen(s: Openable): void }) {
  const day = (ms: number) => formatDateTime(ms).slice(0, 10);
  return (
    <div class="scroll sig-record">
      <Show when={props.stats} fallback={<div class="empty">{props.error ?? 'Reading the record…'}</div>}>
        {(st) => (
          <>
            <div class="sig-cap">
              <b>Replayed</b>
              <Show when={st().backtest} fallback={<span class="dim">the history is replayed a few minutes after the server starts</span>}>
                {(b) => (
                  <span class="dim">
                    {day(b().from)} – {day(b().to)} · {b().markets} coins · the same rules, bar by bar, seeing nothing ahead
                  </span>
                )}
              </Show>
            </div>
            <div class="sig-note dim">{TRADE}</div>
            <Show when={st().backtest}>{(b) => <Table groups={b().groups} sides={props.sides} />}</Show>
            <Show when={st().backtest?.variants}>{(rows) => <Compare rows={rows()} sides={props.sides} />}</Show>
            <div class="sig-cap">
              <b>Live</b>
              <span class="dim">
                {st().live.since ? `since ${day(st().live.since!)}` : 'from the first pattern that turns'}
                {st().live.open ? ` · ${st().live.open} still open` : ''}
              </span>
            </div>
            <Show when={st().live.groups.some((g) => g.n > 0)} fallback={<div class="sig-note dim">None has played out yet: a pattern is given {st().windowBars / 6} days.</div>}>
              <Table groups={st().live.groups} sides={props.sides} />
            </Show>
            <div class="sig-cap">
              <b>At the line · replayed</b>
              <span class="dim">price reaching the line of two drives, entered there: TP 3 / 6 / 9% and SL 3.5% from the touch</span>
            </div>
            <Show when={st().touches.replayed.length > 0} fallback={<div class="sig-note dim">Replayed with the patterns a few minutes after the server starts.</div>}>
              <TouchTable groups={st().touches.replayed} sides={props.sides} />
            </Show>
            <div class="sig-cap">
              <b>At the line · live</b>
              <span class="dim">
                {st().touches.live.since ? `since ${day(st().touches.live.since!)}` : 'from the first touch'}
                {st().touches.live.open ? ` · ${st().touches.live.open} still open` : ''}
              </span>
            </div>
            <Show when={st().touches.live.groups.some((g) => g.n > 0)} fallback={<div class="sig-note dim">None has played out yet.</div>}>
              <TouchTable groups={st().touches.live.groups} sides={props.sides} />
            </Show>
            <For each={st().touches.recent.filter((t) => props.sides.includes(t.side))}>
              {(t) => (
                <button class="sig-row played" classList={{ shown: isShown(touchPattern(t)) }} data-tip={`${t.became ? BECAME[t.became] : 'not yet known'} — open it on the chart`} onClick={() => props.onOpen(touchPattern(t))}>
                  <span class={`sig-out ${t.outcome ?? 'open'}`} data-tip={t.outcome ? TOUCH_OUTCOME[t.outcome] : 'Still open'}>
                    {t.outcome ? `${OUTCOME[t.outcome][0]} ${OUTCOME[t.outcome][1]}` : '… Open'}
                  </span>
                  <span class="sig-coin">
                    <span class={t.side}>{t.side === 'bear' ? '▼' : '▲'}</span>
                    <b>{symbolOf(t.market)}</b>
                    <VenueTag market={t.market} />
                  </span>
                  <span class="sig-drives dim">{t.became ? BECAME[t.became] : '—'}</span>
                  <span class="sig-score num" classList={{ bull: (t.ret ?? 0) > 0, bear: (t.ret ?? 0) < 0 }}>
                    {t.outcome ? pct(t.ret) : '—'}
                  </span>
                  <span class="sig-age num dim">{day(t.touchedAt).slice(5)}</span>
                </button>
              )}
            </For>
            <div class="sig-cap">
              <b>Latest</b>
              <span class="dim">entered at the close the scan read when it turned</span>
            </div>
            <For each={props.recent} fallback={<div class="sig-note dim">No pattern has turned yet.</div>}>
              {(s) => (
                <button class="sig-row played" classList={{ shown: isShown(s) }} data-tip="Open it on the chart" onClick={() => props.onOpen(s)}>
                  <span class={`sig-out ${s.outcome ?? 'open'}`} data-tip={s.outcome ? OUTCOME[s.outcome][2] : 'Still open'}>
                    {s.outcome ? `${OUTCOME[s.outcome][0]} ${OUTCOME[s.outcome][1]}` : '… Open'}
                  </span>
                  <span class="sig-coin">
                    <span class={s.side}>{s.side === 'bear' ? '▼' : '▲'}</span>
                    <b>{symbolOf(s.market)}</b>
                    <VenueTag market={s.market} />
                  </span>
                  <span class="sig-drives num dim" data-tip="Best and worst on the way, the pattern's way">
                    {moved(s) ? `${pct(s.best)} / ${pct(s.worst)}` : '—'}
                  </span>
                  <span class="sig-score num" classList={{ bull: (s.ret ?? 0) > 0, bear: (s.ret ?? 0) < 0 }}>
                    {moved(s) ? pct(s.ret) : '—'}
                  </span>
                  <span class="sig-age num dim">{day(s.entryAt).slice(5)}</span>
                </button>
              )}
            </For>
          </>
        )}
      </Show>
    </div>
  );
}

const TOUCH_OUTCOME: Record<SignalOutcome, string> = {
  tp3: 'All three take-profits',
  tp2: 'Two take-profits',
  tp1: 'The first take-profit',
  stop: 'The stop, 3.5% past the touch, before the first take-profit',
  expired: 'Nothing in four days',
};

function TouchTable(props: { groups: SignalTouchGroup[]; sides: ('bear' | 'bull')[] }) {
  return (
    <table class="grid sig-table">
      <thead>
        <tr>
          <th />
          <th class="num" data-tip="Times price reached a line">Touches</th>
          <th class="num" data-tip="The two drives became three within four days">3 drives</th>
          <th class="num" data-tip="Reached the first take-profit, 3% from the touch">TP1</th>
          <th class="num" data-tip="Reached the second, 6%">TP2</th>
          <th class="num" data-tip="Reached the third, 9%">TP3</th>
          <th class="num" data-tip="The stop, 3.5% past the touch, before the first take-profit">SL</th>
          <th class="num" data-tip="The average result over the whole position">Avg</th>
        </tr>
      </thead>
      <tbody>
        <For each={props.groups.filter((g) => props.sides.includes(g.side))}>
          {(g) => (
            <tr class="head">
              <td>
                <span class={g.side}>{g.side === 'bear' ? '▼ Bearish' : '▲ Bullish'}</span>
              </td>
              <td class="num">{g.n}</td>
              <td class="num" data-tip={`Of them, Stochastic RSI turned: ${share(g.turned, g.n)}`}>{share(g.pattern, g.n)}</td>
              <td class="num bull">{share(g.tp1, g.n)}</td>
              <td class="num bull">{share(g.tp2, g.n)}</td>
              <td class="num bull">{share(g.tp3, g.n)}</td>
              <td class="num bear" data-tip={`Nothing in four days: ${share(g.expired, g.n)}`}>{share(g.stop, g.n)}</td>
              <td class="num" classList={{ bull: g.ret > 0, bear: g.ret < 0 }} data-tip={`At best ${pct(g.best)}, at worst ${pct(g.worst)} on the way, on average`}>
                {g.n ? pct(g.ret) : '—'}
              </td>
            </tr>
          )}
        </For>
      </tbody>
    </table>
  );
}

function Table(props: { groups: SignalGroup[]; sides: ('bear' | 'bull')[] }) {
  return (
    <table class="grid sig-table">
      <thead>
        <tr>
          <th />
          <th class="num" data-tip="Patterns that turned">Patterns</th>
          <th class="num" data-tip="Reached the first take-profit, 3% from the entry">TP1</th>
          <th class="num" data-tip="Reached the second, 6%">TP2</th>
          <th class="num" data-tip="Reached the third, 9%">TP3</th>
          <th class="num" data-tip="The stop, 3.5% past the third drive, before the first take-profit">SL</th>
          <th class="num" data-tip="The average result over the whole position: half at TP1, a quarter at each of the others, the rest at the entry, the stop, or the close four days on">Avg</th>
        </tr>
      </thead>
      <tbody>
        <For each={props.groups.filter((g) => props.sides.includes(g.side))}>
          {(g) => (
            <tr classList={{ head: g.group === 'all' }}>
              <td>
                <Show when={g.group === 'all'} fallback={<span class="dim">{GROUP_LABEL[g.group]}</span>}>
                  <span class={g.side}>{g.side === 'bear' ? '▼ Bearish' : '▲ Bullish'}</span>
                </Show>
              </td>
              <td class="num">{g.n}</td>
              <td class="num bull">{share(g.tp1, g.n)}</td>
              <td class="num bull">{share(g.tp2, g.n)}</td>
              <td class="num bull">{share(g.tp3, g.n)}</td>
              <td class="num bear" data-tip={`Nothing in four days: ${share(g.expired, g.n)}`}>{share(g.stop, g.n)}</td>
              <td class="num" classList={{ bull: g.ret > 0, bear: g.ret < 0 }} data-tip={`At best ${pct(g.best)}, at worst ${pct(g.worst)} on the way, on average`}>
                {g.n ? pct(g.ret) : '—'}
              </td>
            </tr>
          )}
        </For>
      </tbody>
    </table>
  );
}
