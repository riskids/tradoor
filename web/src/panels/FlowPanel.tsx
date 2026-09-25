/**
 * Net taker flow: what the buyers took less what the sellers took, on every
 * market the server follows. Spot and perpetual stand apart — the same number
 * means something different on each — so each keeps its own threshold, and the
 * crossings it made are listed newest first.
 */
import { createSignal, For, onCleanup, Show } from 'solid-js';
import { api, errorText, type FlowEvent, type FlowMarket, type FlowState } from '../lib/api';
import { ExchangeLogo, exchangeOfVenue } from '../lib/exchanges';
import { formatPrice, formatTurnover, priceDecimals } from '../lib/format';
import { parseMarket, symbolOf, venueLabel } from '../lib/markets';
import { readUsd, shortUsd } from '../lib/flow';
import { formatClock } from '../lib/timezone';
import { toast } from '../lib/toasts';
import type { FlowPanelState } from '../workspace/model';
import { patchPanel } from '../workspace/store';

/** How often the flow is asked for again: the server weighs it this often too. */
const POLL_MS = 5_000;

const money = (v: number): string => `${v < 0 ? '-' : '+'}$${formatTurnover(Math.abs(v))}`;
const tone = (v: number): string => (v > 0 ? 'bull' : v < 0 ? 'bear' : 'dim');

/** `Binance Perp` — which exchange and which kind, since all four are one coin. */
function Venue(props: { market: string }) {
  const ref = () => parseMarket(props.market);
  return (
    <span class="sym">
      <ExchangeLogo exchange={exchangeOfVenue(ref()?.venue ?? 'binance')} size={13} />
      <span class="name">{venueLabel(props.market)}</span>
    </span>
  );
}

export function FlowPanel(props: { panel: FlowPanelState }) {
  const id = props.panel.id;
  const [state, setState] = createSignal<FlowState | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [editing, setEditing] = createSignal<string | null>(null);

  const load = async () => {
    try {
      setState(await api.flow());
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
  };
  void load();
  const timer = setInterval(() => void load(), POLL_MS);
  onCleanup(() => clearInterval(timer));

  const markets = (): FlowMarket[] => state()?.markets ?? [];
  const events = (): FlowEvent[] => state()?.events ?? [];

  const save = async (market: FlowMarket, patch: { thresholdUsd?: number; notify?: boolean }) => {
    try {
      await api.flowWatch(market.market, { thresholdUsd: patch.thresholdUsd ?? market.thresholdUsd, notify: patch.notify ?? market.notify });
      await load();
    } catch (e) {
      toast({ tone: 'warn', title: 'Flow setting not saved', body: errorText(e) });
    }
  };

  const digits = (m: { price: number }) => priceDecimals(m.price);

  return (
    <div class="panelcol flow">
      <div class="al-tabs">
        <button class={props.panel.tab === 'crossings' ? 'on' : ''} onClick={() => patchPanel(id, { tab: 'crossings' })}>
          Crossings
        </button>
        <button class={props.panel.tab === 'markets' ? 'on' : ''} onClick={() => patchPanel(id, { tab: 'markets' })}>
          Markets
        </button>
      </div>

      <div class="flow-table">
        <div class="flow-head">
          <span>{symbolOf(markets()[0]?.market ?? '') || 'Market'} · net taker flow</span>
          <span class="r">1m</span>
          <span class="r">1h</span>
        </div>
        <For each={markets()}>
          {(m) => (
            <div class="flow-line">
              <Venue market={m.market} />
              <span class={`num r ${tone(m.minute)}`}>{money(m.minute)}</span>
              <span class={`num r ${tone(m.h1)}`}>{money(m.h1)}</span>
            </div>
          )}
        </For>
      </div>

      <div class="scroll">
        <Show when={props.panel.tab === 'markets'}>
          <For each={markets()} fallback={<div class="empty">{error() ?? 'This server follows no market’s flow.'}</div>}>
            {(m) => (
              <div class="flow-row">
                <Venue market={m.market} />
                <span class="grow" />
                <Show
                  when={editing() === m.market}
                  fallback={
                    <button class="box num" data-tip="The net flow in a minute that is worth a word" onClick={() => setEditing(m.market)}>
                      ${shortUsd(m.thresholdUsd)}
                    </button>
                  }
                >
                  <input
                    class="box num"
                    value={shortUsd(m.thresholdUsd)}
                    ref={(el) => queueMicrotask(() => el.select())}
                    onKeyDown={(e) => {
                      if (e.key === 'Escape') setEditing(null);
                      if (e.key === 'Enter') e.currentTarget.blur();
                    }}
                    onBlur={(e) => {
                      const usd = readUsd(e.currentTarget.value);
                      setEditing(null);
                      if (usd === null) toast({ tone: 'warn', title: 'Write a size like 5M or 750K' });
                      else if (usd !== m.thresholdUsd) void save(m, { thresholdUsd: usd });
                    }}
                  />
                </Show>
                <button
                  class="tb"
                  classList={{ on: m.notify }}
                  data-tip={m.notify ? 'Telling you when it crosses — click to stop' : 'Tell me when it crosses, here and on Telegram'}
                  onClick={() => void save(m, { notify: !m.notify })}
                >
                  {m.notify ? 'Notify' : 'Quiet'}
                </button>
              </div>
            )}
          </For>
          <div class="dim note">A crossing is the flow of one minute. After one, a market stays quiet until its flow falls back to half the threshold.</div>
        </Show>

        <Show when={props.panel.tab === 'crossings'}>
          <For each={events()} fallback={<div class="empty">{error() ?? 'Nothing has crossed yet. Set a threshold under Markets.'}</div>}>
            {(e) => (
              <div class="flow-event">
                <span class="dim num">{formatClock(e.at)}</span>
                <Venue market={e.market} />
                <span class="grow" />
                <span class={`num ${tone(e.deltaUsd)}`}>{money(e.deltaUsd)}</span>
                <span class="num dim">{formatPrice(e.price, digits(e))}</span>
              </div>
            )}
          </For>
        </Show>
      </div>
    </div>
  );
}
