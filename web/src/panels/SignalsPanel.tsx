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
import { api, errorText, type Signal, type SignalCut, type SignalGroup, type SignalLimit, type SignalOutcome, type SignalPlanRow, type SignalPoint, type SignalPush, type SignalsState, type SignalStats, type SignalTouchGroup } from '../lib/api';
import { useDismiss } from '../lib/dismiss';
import { VenueTag } from '../lib/exchanges';
import { formatPrice, formatTurnover } from '../lib/format';
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
  limit?: SignalLimit | null;
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
const GROUP_LABEL: Record<SignalCut, string> = {
  all: 'All',
  '70': 'Score 70+',
  '50': 'Score 50–69',
  '0': 'Score under 50',
  '55': 'Score 55+',
  '40': 'Score 40–54',
  u40: 'Score under 40',
  div: 'RSI divergence',
  ddiv: 'Delta divergence',
  '70ddiv': 'Score 70+ & Δ div',
  '55ddiv': 'Score 55+ & Δ div',
  dagainst: 'Third drive sold',
};
const OUTCOME: Record<SignalOutcome, [string, string, string]> = {
  tp3: ['✓', 'TP3', 'All three take-profits'],
  tp2: ['✓', 'TP2', 'Up to the second take-profit, the rest out at the entry or after four days'],
  tp1: ['✓', 'TP1', 'The first take-profit, the rest out at the entry or after four days'],
  stop: ['✗', 'Stop', 'Went past the third drive first'],
  expired: ['–', 'Neither', 'No take-profit reached after the entry and no loss: four days passed, or the stop had moved to the entry'],
};
/** Why a pattern left the list, as the scan that no longer saw it said. */
const ENDED: Record<string, string> = {
  outdone: 'closed past the third drive',
  broken: 'closed through the line',
  played: 'reached TP1 before the turn',
  waited: 'no turn in two days',
  lapsed: 'two days since it turned',
  unread: 'its coin left the 200 screened',
  replaced: 'an older pattern on the coin listed instead',
};
/** `10-06 04:00`: a day and time in this browser's zone. */
const when = (ms: number) => formatDateTime(ms).slice(5, 16);
/** A limit entry in a line: where it waits, or what became of it. */
const limitText = (l: SignalLimit) => {
  if (l.outcome === 'missed') return `${price(l.price)} · missed`;
  if (l.filledAt === null) return `${price(l.price)} · until ${l.until ? when(l.until) : '—'}`;
  const filled = `${price(l.entry ?? l.price)} · filled ${when(l.filledAt)}`;
  return l.outcome ? `${filled} · ${OUTCOME[l.outcome][1]} ${pct(l.ret)}` : `${filled} · open`;
};
/** The trade's prices, for a tooltip. */
const levelsText = (lv: { tp: number[]; sl: number } | null | undefined) =>
  lv ? `TP ${lv.tp.map(price).join(' / ')} · SL ${price(lv.sl)}` : '';
/** `+$1.2M`, `−$350K`: a push in dollars, buyers positive. */
const usd = (v: number) => `${v < 0 ? '−' : '+'}$${formatTurnover(Math.abs(v))}`;
/** The pushes for a tooltip: each in dollars, and how much of what traded takers bought. */
const pushesText = (deltas: SignalPush[] | null) =>
  deltas ? `Delta on the push into each drive: ${deltas.map((p) => `${usd(p.usd)} (${Math.round((1 + p.share) * 50)}% bought)`).join(', ')}` : 'No delta: Bybit does not say who took each trade';
/** The trade the record counts, said once. */
const TRADE = 'Plan C: TP at 38.2 / 61.8 / 100% back down the third drive\'s leg, SL an ATR past the third drive · half off at TP1 and the stop to the entry, a quarter at each of the others · four days · a take-profit counts only when price reaches it after the entry; Win is closed in profit';
/** The limit entry, said once. */
const LIMIT = 'A limit set when a scan first sees the pattern, half way back from the low since the third drive toward it (the high, for the bullish side), waiting two days; off if price reaches TP1 first. The same plan C once filled.';

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
    if (s.outcome === 'expired') return 'neither take-profit nor stop';
    const ended = s.endedWhy ? ENDED[s.endedWhy] : undefined;
    if (ended) return ended;
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
      const limit = s.limit ? { price: s.limit.price, from: s.limit.setAt, until: s.limit.until ?? s.limit.setAt } : undefined;
      setShownPattern({ market: s.market, side: s.side, drives: s.drives, score: s.score, divergence: s.divergence, stage: s.stage, levels: (s.levels ?? s.limit?.levels) ?? undefined, enteredAt: s.enteredAt ?? s.limit?.setAt, limit });
      setPanelMarket(target.id, s.market);
      setPanelTimeframe(target.id, '4h');
    });
  };

  const setNotify = async (patch: { notify?: boolean; touch?: boolean; limits?: boolean }) => {
    try {
      await api.signalsNotify(patch);
      await load();
      if ((patch.notify || patch.touch || patch.limits) && !state()?.linked) toast({ tone: 'warn', title: 'No Telegram chat linked', body: 'Link one in Settings → Telegram, and these will reach it.' });
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
            classList={{ on: state()?.notify || state()?.notifyTouch || state()?.notifyLimits }}
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
          <button classList={{ cur: state()?.notify }} data-tip="Once per scan, the patterns scoring 70 or more whose Stochastic RSI turned" onClick={() => void setNotify({ notify: !state()?.notify })}>
            <span>When a pattern scoring 70+ turns</span>
            <span class="num">{state()?.notify ? 'on' : 'off'}</span>
          </button>
          <button classList={{ cur: state()?.notifyTouch }} data-tip="At once, when live price reaches the line of two drives — before the bars confirm a third" onClick={() => void setNotify({ touch: !state()?.notifyTouch })}>
            <span>When price reaches the line</span>
            <span class="num">{state()?.notifyTouch ? 'on' : 'off'}</span>
          </button>
          <button
            classList={{ cur: state()?.notifyLimits }}
            data-tip="Once per scan, the limit entry for each bearish pattern first seen scoring 55 or more before the turn, with delta divergence — the entry the replay bore out best"
            onClick={() => void setNotify({ limits: !state()?.notifyLimits })}
          >
            <span>Limit entry for a bearish 55+ with Δ div</span>
            <span class="num">{state()?.notifyLimits ? 'on' : 'off'}</span>
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
                data-tip={`Price reached the line through two drives at ${price(t.touchPrice)} — a third drive as it happens, not yet confirmed by closed bars. Drives at ${t.drives.map((d) => price(d.price)).join(', ')} · ${pushesText(t.deltas)} · ${levelsText(t.levels)}`}
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
                <Pushes deltas={t.deltas} />
                <span class="sig-score num dim">
                  <Show when={t.deltaDivergence} fallback="—">
                    <span class="div delta" data-tip="Delta divergence: takers bought less of the volume on the push into the line so far than into the second drive">
                      ΔDIV
                    </span>
                  </Show>
                </span>
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
                data-tip={`Drives at ${s.drives.map((d) => price(d.price)).join(', ')} · ${pushesText(s.deltas)} · ${turned() ? `Stochastic RSI turned at ${price(s.turnPrice ?? s.price)}` : 'waiting for Stochastic RSI to turn'}${s.divergence ? ' · RSI weaker at the third drive' : ''}${s.levels ? ` · ${levelsText(s.levels)}` : ''} — open it on the chart`}
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
                <Pushes deltas={s.deltas} />
                <span class="sig-score num">
                  <Show when={s.outcome}>{(o) => <span class={`sig-out ${o()}`} data-tip={`${OUTCOME[o()][2]} · ${pct(s.ret)}`}>{OUTCOME[o()][0]}</span>}</Show>
                  <Show when={s.deltaDivergence}>
                    <span class="div delta" data-tip={`Delta divergence: takers bought less of the volume on the push into the third drive than into the second${s.deltaAgainst ? ', and the third drive\'s own bar was sold more than bought' : ''}`}>
                      ΔDIV
                    </span>
                  </Show>
                  <Show when={s.divergence}>
                    <span class="div" data-tip="RSI weaker at the third drive than at the second">DIV</span>
                  </Show>
                  {s.score}
                </span>
                <span class="sig-age num dim">{since(s.turnedAt ?? s.drives[2]!.at, now())}</span>
                <Show when={s.limit}>
                  {(l) => (
                    <span class="sig-limit num" classList={{ gone: l().outcome === 'missed' }} data-tip={`${LIMIT}${l().levels ? ` · ${levelsText(l().levels)}` : ''}`}>
                      <span class="tag">LIMIT</span> {limitText(l())}
                    </span>
                  )}
                </Show>
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

/** The delta on the push into each drive, in place of the drives' prices:
 *  buyers' dollars green, sellers' red; a touch's third push is the one so far. */
function Pushes(props: { deltas: SignalPush[] | null }) {
  return (
    <span class="sig-drives num">
      <Show when={props.deltas} fallback="—">
        {(deltas) => (
          <For each={deltas()}>
            {(p, i) => (
              <>
                <Show when={i() > 0}>
                  <span class="dim"> → </span>
                </Show>
                <span class={p.usd < 0 ? 'sold' : 'bought'}>{usd(p.usd)}</span>
              </>
            )}
          </For>
        )}
      </Show>
    </span>
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
  { value: 'ddiv', label: 'Δ divergence' },
  { value: '70ddiv', label: '70+ & Δ div' },
] as const;

/** Each plan replayed over the same patterns and touches, side by side, with both halves of the period. */
function Compare(props: { rows: SignalPlanRow[]; sides: ('bear' | 'bull')[] }) {
  const [cut, setCut] = createSignal<(typeof CUTS)[number]['value']>('all');
  /** The same cut of the limits: their score bands are fifteen lower, set before the turn. */
  const cutOf = (kind: 'pattern' | 'touch' | 'limit'): SignalCut => (kind === 'touch' ? 'all' : kind === 'limit' ? ({ '70': '55', '70ddiv': '55ddiv' } as const)[cut() as '70'] ?? cut() : cut());
  const table = (kind: 'pattern' | 'touch' | 'limit') => (
    <table class="grid sig-table sig-compare">
      <thead>
        <tr>
          <th>Plan</th>
          <th />
          <th class="num">n</th>
          <th class="num" data-tip="Closed in profit">Win</th>
          <th class="num">TP1</th>
          <th class="num">TP2</th>
          <th class="num">TP3</th>
          <th class="num">SL</th>
          <th class="num" data-tip="The average result over the whole position">Avg</th>
          <th class="num" data-tip="The average in the first half of the period, and in the second: a plan worth keeping holds up in both">Halves</th>
        </tr>
      </thead>
      <tbody>
        <For each={props.rows.filter((r) => r.kind === kind && r.group === cutOf(kind) && props.sides.includes(r.side))}>
          {(r) => (
            <tr classList={{ head: r.side === props.sides[0] }}>
              <td data-tip={`${PLANS[r.plan][1]} · ${PLANS[r.plan][2]}`}>
                <Show when={r.side === props.sides[0]}>
                  <b>{PLANS[r.plan][0]}</b> <span class="dim">{PLANS[r.plan][1]} · {PLANS[r.plan][2]}</span>
                  {r.plan === 'fib-atr' ? <span class="badge">live</span> : null}
                </Show>
              </td>
              <td>
                <span class={r.side}>{r.side === 'bear' ? '▼' : '▲'}</span>
              </td>
              <td class="num">{r.n}</td>
              <td class="num">{share(r.win, r.n)}</td>
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
        <b>Plans compared · limit entry</b>
        <span class="dim">filled limits; their score bands fifteen lower, set before the turn</span>
      </div>
      {table('limit')}
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
              <b>Limit entry · replayed</b>
              <span class="dim">the same patterns, entered at a limit instead of at the turn</span>
            </div>
            <div class="sig-note dim">{LIMIT}</div>
            <Show when={st().backtest?.limits.some((g) => g.seen > 0)} fallback={<div class="sig-note dim">Replayed with the patterns a few minutes after the server starts.</div>}>
              <Table groups={st().backtest!.limits} sides={props.sides} limits />
            </Show>
            <div class="sig-cap">
              <b>Limit entry · live</b>
              <span class="dim">
                patterns first seen since {day(st().limits.since)}
                {st().limits.open ? ` · ${st().limits.open} still open` : ''}
              </span>
            </div>
            <Show when={st().limits.groups.some((g) => g.seen > 0)} fallback={<div class="sig-note dim">None has played out yet.</div>}>
              <Table groups={st().limits.groups} sides={props.sides} limits />
            </Show>
            <div class="sig-cap">
              <b>At the line · replayed</b>
              <span class="dim">price reaching the line of two drives, entered there: the touch is the third drive so far</span>
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
  stop: 'The stop, an ATR past the touch, before the first take-profit',
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
          <th class="num" data-tip="Closed in profit">Win</th>
          <th class="num" data-tip="Reached the first take-profit">TP1</th>
          <th class="num" data-tip="Reached the second">TP2</th>
          <th class="num" data-tip="Reached the third">TP3</th>
          <th class="num" data-tip="The stop, before the first take-profit">SL</th>
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
              <td class="num">{share(g.win, g.n)}</td>
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

function Table(props: { groups: SignalGroup[]; sides: ('bear' | 'bull')[]; limits?: boolean }) {
  return (
    <table class="grid sig-table">
      <thead>
        <tr>
          <th />
          <Show when={props.limits} fallback={<th class="num" data-tip="Patterns that turned">Patterns</th>}>
            <th class="num" data-tip="Limits set, one per pattern">Set</th>
            <th class="num" data-tip="Price came back to the limit within two days, before reaching TP1">Filled</th>
          </Show>
          <th class="num" data-tip="Closed in profit, of those entered">Win</th>
          <th class="num" data-tip="Reached the first take-profit after the entry">TP1</th>
          <th class="num" data-tip="Reached the second">TP2</th>
          <th class="num" data-tip="Reached the third">TP3</th>
          <th class="num" data-tip="The stop, before the first take-profit">SL</th>
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
              <Show when={props.limits} fallback={<td class="num">{g.n}</td>}>
                <td class="num">{g.seen}</td>
                <td class="num" data-tip={`${g.n} of ${g.seen}`}>{share(g.n, g.seen)}</td>
              </Show>
              <td class="num">{share(g.win, g.n)}</td>
              <td class="num bull">{share(g.tp1, g.n)}</td>
              <td class="num bull">{share(g.tp2, g.n)}</td>
              <td class="num bull">{share(g.tp3, g.n)}</td>
              <td class="num bear" data-tip={`Neither take-profit nor stop: ${share(g.expired, g.n)}`}>{share(g.stop, g.n)}</td>
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
