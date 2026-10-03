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
 * latest of those one by one — target or stop first, or neither in four days.
 */
import { batch, createEffect, createSignal, For, onCleanup, Show } from 'solid-js';
import { setShownPattern, shownPattern } from '../app/ui';
import { api, errorText, type Signal, type SignalGroup, type SignalOutcome, type SignalPoint, type SignalsState, type SignalStats } from '../lib/api';
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
/** `5h`, `2d`: how long since. */
const since = (at: number, now: number) => {
  const hours = Math.max(0, Math.floor((now - at) / 3_600_000));
  return hours < 48 ? `${hours}h` : `${Math.floor(hours / 24)}d`;
};
/** Any pattern a row can open: the list's, or the record's. */
interface Openable {
  market: string;
  side: 'bear' | 'bull';
  drives: SignalPoint[];
  score: number;
  divergence: boolean;
}
const isShown = (s: Openable) => {
  const p = shownPattern();
  return p !== null && p.market === s.market && p.drives[2]?.at === s.drives[2]?.at;
};
/** Whether a played pattern has a move to show: decided, or open with bars since. */
const moved = (s: { outcome: SignalOutcome | null; best: number | null; worst: number | null }) => s.outcome !== null || (s.best ?? 0) !== 0 || (s.worst ?? 0) !== 0;
const pct = (v: number | null | undefined) => (v === null || v === undefined ? '—' : `${v > 0 ? '+' : ''}${v.toFixed(1)}%`);
const share = (part: number, n: number) => (n === 0 ? '—' : `${Math.round((part / n) * 100)}%`);
const GROUP_LABEL: Record<SignalGroup['group'], string> = { all: 'All', '70': 'Score 70+', '50': 'Score 50–69', '0': 'Score under 50', div: 'RSI divergence' };
const OUTCOME: Record<SignalOutcome, [string, string, string]> = {
  target: ['✓', 'Target', 'Reached the pullback before the third drive first'],
  stop: ['✗', 'Stop', 'Went past the third drive first'],
  expired: ['–', 'Neither', 'Neither in four days'],
  passed: ['·', 'Passed', 'Already past its target when it turned'],
};

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
  /** Why a pattern left the list: how it played out, or that it had been news for two days. */
  const why = (s: Signal): string => {
    if (s.outcome === 'stop') return 'went past the third drive';
    if (s.outcome === 'target') return 'reached its target';
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
      setShownPattern({ market: s.market, side: s.side, drives: s.drives, score: s.score, divergence: s.divergence });
      setPanelMarket(target.id, s.market);
      setPanelTimeframe(target.id, '4h');
    });
  };

  const setNotify = async (notify: boolean) => {
    try {
      await api.signalsNotify(notify);
      await load();
      if (notify && !state()?.linked) toast({ tone: 'warn', title: 'No Telegram chat linked', body: 'Link one in Settings → Telegram, and turned patterns will reach it.' });
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
            classList={{ on: state()?.notify }}
            data-tip={state()?.notify ? 'Telling you on Telegram when a pattern turns — click to stop' : 'Tell me on Telegram when a pattern turns'}
            onClick={() => void setNotify(!state()?.notify)}
          >
            <Icon name="alarm" size={12} /> Telegram {state()?.notify ? 'on' : 'off'}
          </button>
        </Show>
      </div>
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
        <For each={signals()} fallback={<Show when={state()?.scannedAt}>{<div class="empty">No three drives on four-hour bars right now.</div>}</Show>}>
          {(s) => {
            const turned = () => s.turnedAt !== null;
            return (
              <button
                class="sig-row"
                classList={{ shown: isShown(s) }}
                data-tip={`Drives at ${s.drives.map((d) => price(d.price)).join(', ')} · ${turned() ? `Stochastic RSI turned at ${price(s.turnPrice ?? s.price)}` : 'waiting for Stochastic RSI to turn'}${s.divergence ? ' · RSI weaker at the third drive' : ''} — open it on the chart`}
                onClick={() => open(s)}
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
              <button class="sig-row ended" classList={{ shown: isShown(s) }} data-tip={`${why(s)}${s.ret !== null && s.outcome ? ` · ${pct(s.ret)}` : ''} — open it on the chart`} onClick={() => open(s)}>
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
            <Show when={st().backtest}>{(b) => <Table groups={b().groups} sides={props.sides} />}</Show>
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

function Table(props: { groups: SignalGroup[]; sides: ('bear' | 'bull')[] }) {
  return (
    <table class="grid sig-table">
      <thead>
        <tr>
          <th />
          <th class="num" data-tip="Patterns that turned">Patterns</th>
          <th class="num" data-tip="Reached the pullback before the third drive before going past the third drive">Target</th>
          <th class="num" data-tip="Went past the third drive first">Stop</th>
          <th class="num" data-tip="Neither within four days">Neither</th>
          <th class="num" data-tip="The average move to the exit, the pattern's way">Avg</th>
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
              <td class="num bull">{share(g.target, g.n)}</td>
              <td class="num bear">{share(g.stop, g.n)}</td>
              <td class="num dim">{share(g.expired, g.n)}</td>
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
