/**
 * Net taker flow: what the buyers took less what the sellers took, on every
 * market the server follows one coin on, with the minutes that ran hard enough
 * to be worth a word beneath them.
 *
 * BTC and ETH are always followed, for everyone. A signed-in reader adds up to
 * seven coins of their own from here, each for a day or a week from when they
 * last asked, or for as long as they leave it, until they stop it with the
 * chip's ×. The list is theirs alone; a coin two readers follow is still
 * recorded once. Anyone drags the chips into the order they like.
 *
 * The list is the panel; what counts as hard — a size per market, and whether
 * Telegram hears about it — sits under the gear, since it is set once and read
 * every minute after.
 */
import { createEffect, createSignal, For, on, onCleanup, Show } from 'solid-js';
import { api, errorText, type FlowCoin, type FlowEvent, type FlowMarket, type FlowSpan, type FlowState } from '../lib/api';
import { ExchangeLogo, exchangeOfVenue } from '../lib/exchanges';
import { useDismiss } from '../lib/dismiss';
import { formatPrice, formatTurnover, priceDecimals } from '../lib/format';
import { Icon } from '../lib/icons';
import { parseMarket, venueLabel } from '../lib/markets';
import { setOverlay } from '../app/ui';
import { readUsd, shortUsd } from '../lib/flow';
import { formatClock, formatDateTime } from '../lib/timezone';
import { toast } from '../lib/toasts';
import type { FlowPanelState } from '../workspace/model';
import { patchPanel } from '../workspace/store';

/** How often the flow is asked for again: the server weighs it this often too. */
const POLL_MS = 5_000;
/** How long the pointer rests on a chip before it shows it can be dragged. */
const GRAB_MS = 700;

const money = (v: number): string => `${v < 0 ? '-' : '+'}$${formatTurnover(Math.abs(v))}`;
const tone = (v: number): string => (v > 0 ? 'bull' : v < 0 ? 'bear' : 'dim');
/** What is left of a coin's time: `6d`, `23h`, `40m`. */
const left = (until: number, now: number): string => {
  const ms = Math.max(0, until - now);
  if (ms >= 2 * 86_400_000) return `${Math.round(ms / 86_400_000)}d`;
  return ms >= 3_600_000 ? `${Math.round(ms / 3_600_000)}h` : `${Math.ceil(ms / 60_000)}m`;
};
/** When it lapses: the hour today or tomorrow, the day and hour further off. */
const lapses = (until: number, now: number): string => (until - now > 86_400_000 ? formatDateTime(until).slice(5, 16) : formatClock(until));
/** `ETH` from `eth` or `ETHUSDT`: a coin as the server names it. */
const coinFrom = (text: string): string => text.trim().toUpperCase().replace(/^(.+?)(USDT|USDC|USD)$/, '$1');
const ALWAYS: FlowCoin = { coin: 'BTC', until: null, fixed: true };
/** Recorded always, nobody's to stop: BTC and ETH, as the server says. */
const fixed = (c: FlowCoin): boolean => c.fixed === true || c.coin === ALWAYS.coin;
/** How long a reader can follow a coin, as the buttons and the notice say it. */
const SPANS: { span: FlowSpan; label: string; told: string }[] = [
  { span: 'day', label: '24h', told: 'for 24 hours' },
  { span: 'week', label: '7d', told: 'for 7 days' },
  { span: 'always', label: 'Always', told: 'until you stop it' },
];

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

  const [now, setNow] = createSignal(Date.now());
  const [adding, setAdding] = createSignal(false);
  const [busy, setBusy] = createSignal(false);

  const coin = () => props.panel.coin || 'BTC';
  // Only the latest ask is shown: a slow answer for the coin just left must not
  // land over the one just chosen.
  let asked = 0;
  const load = async () => {
    const n = ++asked;
    try {
      const next = await api.flow(coin());
      if (n !== asked) return;
      setState(next);
      setNow(Date.now());
      setError(null);
    } catch (e) {
      if (n === asked) setError(errorText(e));
    }
  };
  createEffect(on(coin, () => void load()));
  const timer = setInterval(() => void load(), POLL_MS);
  onCleanup(() => clearInterval(timer));

  /** The coin shown: the one chosen, or BTC once that one has lapsed. */
  const shown = () => state()?.coin ?? coin();
  /** The coins followed, in this panel's order: those it has not placed come last, in the server's. */
  const coins = (): FlowCoin[] => {
    const order = props.panel.order ?? [];
    const list = state()?.coins ?? [ALWAYS];
    const rank = (c: FlowCoin) => (order.includes(c.coin) ? order.indexOf(c.coin) : order.length + list.indexOf(c));
    return [...list].sort((a, b) => rank(a) - rank(b));
  };
  const recording = () => coins().find((c) => c.coin === shown() && !fixed(c));
  const choose = (name: string) => patchPanel(props.panel.id, { coin: name });
  /** This reader's own coins, and how many they may have. */
  const mine = () => coins().filter((c) => !fixed(c)).length;
  const room = () => state()?.perReader ?? 7;
  /** The `+`: the box to name a coin, or what stands in the way of one. */
  const add = () => {
    if (!state()?.signedIn) {
      toast({ tone: 'info', title: 'Sign in to follow more coins', body: 'BTC and ETH are here for everyone. Sign in with Telegram in Settings to follow up to seven coins of your own.', action: () => setOverlay({ kind: 'settings' }) });
      return;
    }
    if (mine() >= room()) {
      toast({ tone: 'warn', title: `You follow ${room()} coins already`, body: 'Stop one with its × to add another.' });
      return;
    }
    setAdding(true);
  };

  // Dragging a chip along the toolbar: where it would land, as an index among the chips.
  const [drag, setDrag] = createSignal<{ coin: string; to: number } | null>(null);
  let chips!: HTMLSpanElement;
  /** A press that moved was a drag: the click that ends it does not choose. */
  let draggedAt = 0;
  const landing = (x: number): number =>
    [...chips.querySelectorAll<HTMLElement>('.fl-chip')].filter((el) => {
      const r = el.getBoundingClientRect();
      return r.left + r.width / 2 < x;
    }).length;
  // The hand that says a chip can be dragged comes only once the pointer rests
  // on it: passing over to click one should not look like picking it up.
  const [grab, setGrab] = createSignal<string | null>(null);
  let grabTimer: ReturnType<typeof setTimeout> | undefined;
  const rest = (coin: string | null) => {
    clearTimeout(grabTimer);
    setGrab(null);
    if (coin) grabTimer = setTimeout(() => setGrab(coin), GRAB_MS);
  };
  onCleanup(() => clearTimeout(grabTimer));
  const press = (e: PointerEvent, coin: string) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest('.x')) return;
    const startX = e.clientX;
    let moved = false;
    const move = (ev: PointerEvent) => {
      if (!moved && Math.abs(ev.clientX - startX) < 5) return;
      moved = true;
      if (!drag()) document.documentElement.style.cursor = 'grabbing';
      setDrag({ coin, to: landing(ev.clientX) });
    };
    const up = () => {
      removeEventListener('pointermove', move);
      removeEventListener('pointerup', up);
      removeEventListener('pointercancel', up);
      const d = drag();
      setDrag(null);
      document.documentElement.style.cursor = '';
      if (!moved || !d) return;
      draggedAt = performance.now();
      const list = coins().map((c) => c.coin);
      const from = list.indexOf(d.coin);
      list.splice(from, 1);
      list.splice(d.to > from ? d.to - 1 : d.to, 0, d.coin);
      patchPanel(props.panel.id, { order: list });
    };
    addEventListener('pointermove', move);
    addEventListener('pointerup', up);
    addEventListener('pointercancel', up);
  };

  /** Record a coin for the span asked for, counted from now if it already is. */
  const follow = async (text: string, span: FlowSpan = 'day') => {
    const name = coinFrom(text);
    if (!name) return;
    setBusy(true);
    try {
      const followed = await api.flowFollow(name, span);
      setAdding(false);
      const told = SPANS.find((s) => s.span === span)!.told;
      toast({ tone: 'info', title: `Following ${followed.coin}'s flow ${told}`, body: `On ${followed.markets.length} market${followed.markets.length === 1 ? '' : 's'}` });
      if (followed.coin === coin()) await load();
      else choose(followed.coin);
    } catch (e) {
      toast({ tone: 'warn', title: `${name} not followed`, body: errorText(e) });
    } finally {
      setBusy(false);
    }
  };

  const stop = async (name: string) => {
    if (!confirm(`Stop following ${name}'s flow? What is recorded so far is kept for two days.`)) return;
    try {
      await api.flowUnfollow(name);
      if (coin() === name) choose('BTC');
      else await load();
    } catch (e) {
      toast({ tone: 'warn', title: `${name} still followed`, body: errorText(e) });
    }
  };

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
        <span class="fl-chips" ref={chips}>
          <For each={coins()}>
            {(c, i) => (
              <span
                class="fl-chip"
                classList={{
                  on: c.coin === shown(),
                  paused: c.recorded === false,
                  dragging: drag()?.coin === c.coin,
                  grab: grab() === c.coin,
                  before: drag()?.to === i(),
                  after: drag()?.to === coins().length && i() === coins().length - 1,
                }}
                onPointerDown={(e) => press(e, c.coin)}
                onPointerEnter={(e) => e.pointerType === 'mouse' && rest(c.coin)}
                onPointerLeave={() => rest(null)}
              >
                <button
                  class="tb fl-coin"
                  classList={{ on: c.coin === shown() }}
                  data-tip={
                    c.recorded === false
                      ? 'Not recorded yet: the server records as many coins as it can, and this one starts once there is room'
                      : c.until !== null
                        ? `Followed until ${lapses(c.until, now())} · drag to reorder`
                        : fixed(c)
                          ? 'Always recorded · drag to reorder'
                          : 'Followed until you stop it · drag to reorder'
                  }
                  onClick={() => {
                    if (performance.now() - draggedAt > 300) choose(c.coin);
                  }}
                >
                  {c.coin}
                  <Show when={c.until} fallback={fixed(c) ? null : <span class="left">∞</span>}>
                    {(until) => <span class="left">{left(until(), now())}</span>}
                  </Show>
                </button>
                <Show when={!fixed(c)}>
                  <button class="x" aria-label={`Stop following ${c.coin}`} data-tip={`Stop following ${c.coin}`} onClick={() => void stop(c.coin)}>
                    <Icon name="close" size={9} stroke={2.4} />
                  </button>
                </Show>
              </span>
            )}
          </For>
        </span>
        <Show when={state()?.watching}>
          <Show
            when={adding()}
            fallback={
              <button
                class="iconbtn"
                data-tip={state()?.signedIn ? `Follow another coin's flow · ${mine()} of ${room()}` : 'Follow another coin: sign in with Telegram first'}
                onClick={add}
              >
                <Icon name="plus" size={13} />
              </button>
            }
          >
            <form
              class="fl-add"
              onSubmit={(e) => {
                e.preventDefault();
                // Enter in the box is the first button's: a day.
                const span = ((e.submitter as HTMLButtonElement | null)?.value || 'day') as FlowSpan;
                void follow((e.currentTarget.elements.namedItem('coin') as HTMLInputElement).value, span);
              }}
            >
              <input
                name="coin"
                class="box"
                placeholder="ETH"
                maxLength={24}
                autocomplete="off"
                ref={(el) => queueMicrotask(() => el.focus())}
                onKeyDown={(e) => {
                  if (e.key === 'Escape') setAdding(false);
                }}
                onBlur={(e) => {
                  if (!e.currentTarget.value.trim() && !busy()) setAdding(false);
                }}
              />
              <For each={SPANS}>
                {(s, i) => (
                  <button class="tb" classList={{ on: i() === 0 }} type="submit" value={s.span} disabled={busy()} data-tip={`Follow it ${s.told}`}>
                    {s.label}
                  </button>
                )}
              </For>
            </form>
          </Show>
        </Show>
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

      <Show when={recording()}>
        {(c) => (
          <div class="fl-note">
            <span class="grow">
              {c().until === null ? `You follow ${c().coin} until you stop it` : `You follow ${c().coin} for ${left(c().until!, now())} more`}
            </span>
            <span class="dim">Keep</span>
            <For each={SPANS}>
              {(s) => (
                <button
                  class="tb"
                  classList={{ on: s.span === 'always' && c().until === null }}
                  data-tip={s.span === 'always' ? 'Follow it until you stop it' : `Follow it ${s.told} from now`}
                  disabled={busy()}
                  onClick={() => void follow(c().coin, s.span)}
                >
                  {s.label}
                </button>
              )}
            </For>
            <button class="tb danger" data-tip="Stop following it now" onClick={() => void stop(c().coin)}>
              Stop
            </button>
          </div>
        )}
      </Show>

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
          <span>{shown()} · net taker flow</span>
          <span class="r">1m</span>
          <span class="r">1h</span>
          <span class="r">4h</span>
          <span class="r">12h</span>
          <span class="r">24h</span>
        </div>
        <For each={markets()} fallback={<div class="empty">{error() ?? 'This server follows no market’s flow.'}</div>}>
          {(m) => (
            <div class="flow-line">
              <Venue market={m.market} />
              <span class={`num r ${tone(m.minute)}`}>{money(m.minute)}</span>
              <span class={`num r ${tone(m.h1)}`}>{money(m.h1)}</span>
              <span class={`num r ${tone(m.h4)}`}>{money(m.h4)}</span>
              <span class={`num r ${tone(m.h12)}`}>{money(m.h12)}</span>
              <span class={`num r ${tone(m.day)}`}>{money(m.day)}</span>
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
