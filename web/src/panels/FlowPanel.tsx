/**
 * Net taker flow: what the buyers took less what the sellers took, on every
 * market the server follows, with the minutes that ran hard enough to be worth
 * a word beneath them.
 *
 * The list is the panel; what counts as hard — a size per market, and whether
 * Telegram hears about it — sits under the gear, since it is set once and read
 * every minute after.
 */
import { createSignal, For, onCleanup, Show } from 'solid-js';
import { api, errorText, type FlowEvent, type FlowMarket, type FlowState } from '../lib/api';
import { ExchangeLogo, exchangeOfVenue } from '../lib/exchanges';
import { useDismiss } from '../lib/dismiss';
import { formatPrice, formatTurnover, priceDecimals } from '../lib/format';
import { Icon } from '../lib/icons';
import { parseMarket, symbolOf, venueLabel } from '../lib/markets';
import { readUsd, shortUsd } from '../lib/flow';
import { formatClock } from '../lib/timezone';
import { toast } from '../lib/toasts';
import type { FlowPanelState } from '../workspace/model';

/** How often the flow is asked for again: the server weighs it this often too. */
const POLL_MS = 5_000;

const money = (v: number): string => `${v < 0 ? '-' : '+'}$${formatTurnover(Math.abs(v))}`;
const tone = (v: number): string => (v > 0 ? 'bull' : v < 0 ? 'bear' : 'dim');
/** `BTC` — the coin the followed markets share, whatever each quotes it in. */
const coinOf = (market: string): string => symbolOf(market).replace(/(USDT|USDC|USD)$/, '');

/** `Binance Perp` — which exchange and which kind, since all of them are one coin. */
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
  const [state, setState] = createSignal<FlowState | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [settings, setSettings] = createSignal(false);
  const [editing, setEditing] = createSignal<string | null>(null);
  useDismiss(settings, () => setSettings(false));

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
  /** The markets being weighed for this reader: the rest are only watched. */
  const told = () => markets().filter((m) => m.notify).length;

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
      <div class="toolbar">
        <span class="caps" style={{ padding: '0 4px' }}>
          Flow
        </span>
        <span class="dim">{coinOf(markets()[0]?.market ?? '')}</span>
        <span class="spacer" />
        <button
          class="iconbtn"
          classList={{ on: settings() }}
          data-tip="What counts as a big minute, and where to be told"
          onPointerDown={(e) => e.stopPropagation()}
          onClick={() => setSettings((v) => !v)}
        >
          <Icon name="settings" size={13} />
        </button>
      </div>

      <Show when={settings()}>
        <div class="menu fl-menu" onPointerDown={(e) => e.stopPropagation()}>
          <div class="cap">Tell me when a minute’s flow passes</div>
          <For each={markets()} fallback={<div class="empty">{error() ?? 'This server follows no market’s flow.'}</div>}>
            {(m) => (
              <div class="row">
                <Venue market={m.market} />
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
          <div class="cap note">A market stays quiet after a crossing until its flow falls back to half that size, so one push does not ring over.</div>
        </div>
      </Show>

      <div class="flow-table">
        <div class="flow-head">
          <span>{coinOf(markets()[0]?.market ?? '') || 'Market'} · net taker flow</span>
          <span class="r">1m</span>
          <span class="r">1h</span>
          <span class="r">4h</span>
        </div>
        <For each={markets()} fallback={<div class="empty">{error() ?? 'This server follows no market’s flow.'}</div>}>
          {(m) => (
            <div class="flow-line">
              <Venue market={m.market} />
              <span class={`num r ${tone(m.minute)}`}>{money(m.minute)}</span>
              <span class={`num r ${tone(m.h1)}`}>{money(m.h1)}</span>
              <span class={`num r ${tone(m.h4)}`}>{money(m.h4)}</span>
            </div>
          )}
        </For>
      </div>

      <div class="flow-sub">
        <span>Crossings</span>
        <span class="dim">{told() ? `${told()} market${told() > 1 ? 's' : ''} watched · 48h` : 'none set'}</span>
      </div>
      <div class="scroll">
        <For
          each={events()}
          fallback={<div class="empty">{error() ?? (told() ? 'Nothing has crossed yet.' : 'No size is set yet. Open the gear to say what counts as a big minute.')}</div>}
        >
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
      </div>
    </div>
  );
}
