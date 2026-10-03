/**
 * What the server's screener found: three drives on four-hour bars, either
 * way, among the two hundred busiest perpetuals on Binance and Bybit — those
 * whose Stochastic RSI has turned first, then those still waiting for it.
 *
 * A row opens its market on the chart at four hours, with the pattern drawn
 * on it until it is closed or kept (ChartPanel). Telegram, when this browser
 * asks, hears of each pattern once, when it turns.
 */
import { batch, createSignal, For, onCleanup, Show } from 'solid-js';
import { setShownPattern, shownPattern } from '../app/ui';
import { api, errorText, type Signal, type SignalsState } from '../lib/api';
import { VenueTag } from '../lib/exchanges';
import { formatPrice } from '../lib/format';
import { Icon } from '../lib/icons';
import { symbolOf } from '../lib/markets';
import { formatHm } from '../lib/timezone';
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
const isShown = (s: Signal) => {
  const p = shownPattern();
  return p !== null && p.market === s.market && p.drives[2]?.at === s.drives[2]?.at;
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

  /** The market on the chart this panel drives, at four hours, with the pattern drawn on it. */
  const open = (s: Signal) => {
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
      </div>
    </div>
  );
}
