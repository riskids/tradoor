/**
 * Liquidations for one ticker, gathered from every exchange that lists it: the
 * 1H, 4H, 12H and 24H totals the server keeps, each exchange's connection, and
 * the tape newest first. The tape belongs to the page, so switching tabs keeps
 * it; the totals belong to the server, so a reload keeps them too.
 */
import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js';
import { api, catalog, type LiquidationStats, type LiquidationWindow } from '../lib/api';
import { ExchangeLogo, exchangeOfVenue } from '../lib/exchanges';
import { formatPrice, formatTurnover, priceDecimals } from '../lib/format';
import { holdTape, releaseTape, type Tape, type TapeHit, type VenueStatus } from '../lib/liquidation-tape';
import { serverTicker } from '../lib/liquidation-venues';
import { parseMarket, symbolOf } from '../lib/markets';
import { formatClock, formatHm, zoneLabel } from '../lib/timezone';
import type { LiquidationsPanelState } from '../workspace/model';
import { marketOf, patchPanel } from '../workspace/store';
import { MarketButton } from './common';

/** How often the totals are asked for again. */
const POLL_MS = 5_000;

const STATUS_LABEL: Record<VenueStatus, string> = {
  connecting: 'Connecting',
  live: 'Connected',
  stalled: 'Stalled',
  retrying: 'Reconnecting',
  unlisted: 'Not listed',
};

const WINDOW_MS: Record<string, number> = { '1h': 3_600_000, '4h': 14_400_000, '12h': 43_200_000, '24h': 86_400_000 };

const usd = (v: number): string => `$${formatTurnover(v)}`;

export function LiquidationsPanel(props: { panel: LiquidationsPanelState }) {
  const id = props.panel.id;
  const market = () => marketOf(props.panel);
  const ticker = () => symbolOf(market());
  const [tape, setTape] = createSignal<Tape | null>(null);
  const [stats, setStats] = createSignal<LiquidationStats | null>(null);
  const [statsError, setStatsError] = createSignal(false);
  const [tick, setTick] = createSignal(2);

  // The tape outlives this panel: hold it while shown, release it when not.
  createEffect(() => {
    const held = holdTape(market());
    setTape(held);
    onCleanup(() => releaseTape(held));
  });

  createEffect(() => {
    const m = market();
    void catalog().then((c) => setTick(priceDecimals(c.byMarket.get(m)?.lastPrice ?? 0)));
  });

  // The totals, from the server's day of hits; the tape is seeded from it too.
  createEffect(() => {
    const t = ticker();
    const held = tape();
    setStats(null);
    let current = true;
    const load = async () => {
      try {
        const s = await api.liquidations(serverTicker(t));
        if (!current) return;
        setStats(s);
        setStatsError(false);
        if (s.tracked) held?.seed(s.recent);
      } catch {
        if (current) setStatsError(true);
      }
    };
    void load();
    const timer = setInterval(() => void load(), POLL_MS);
    onCleanup(() => {
      current = false;
      clearInterval(timer);
    });
  });

  // The exchange table is a hover: it opens on the logos and stays while the
  // pointer is over it, with a moment's grace to cross the gap between them.
  const [venuesOpen, setVenuesOpen] = createSignal(false);
  let closing: ReturnType<typeof setTimeout> | undefined;
  const showVenues = (open: boolean) => {
    clearTimeout(closing);
    if (open) setVenuesOpen(true);
    else closing = setTimeout(() => setVenuesOpen(false), 150);
  };
  onCleanup(() => clearTimeout(closing));

  const min = () => props.panel.minUsd;
  const hits = (): TapeHit[] => (tape()?.hits() ?? []).filter((h) => h.usd >= min());
  const perVenue = (venue: string) => hits().filter((h) => h.venue === venue);
  const sum = (rows: TapeHit[]) => rows.reduce((total, h) => total + h.usd, 0);

  const tracked = () => {
    const s = stats();
    return s?.tracked ? s : null;
  };
  /** The tickers the server does keep, when this is not one of them. */
  const untracked = () => {
    const s = stats();
    return s && !s.tracked ? s.tickers : null;
  };
  /** The server's sockets that are not up, named, since its totals miss them. */
  const serverGaps = () =>
    (tracked()?.venues ?? [])
      .filter((v) => v.state !== 'connected')
      .map((v) => `${exchangeOfVenue(v.venue).name} ${v.state === 'retrying' ? 'reconnecting' : v.state}`);

  return (
    <div class="panelcol liq">
      <div class="toolbar">
        <MarketButton panelId={id} market={market()} venue={false} />
        {/* Each exchange's mark, faded while it is not connected; the full status on hover. */}
        <button class="tb liq-venues" title="Exchanges" onMouseEnter={() => showVenues(true)} onMouseLeave={() => showVenues(false)} onClick={() => setVenuesOpen((v) => !v)}>
          <For each={(tape()?.venues() ?? []).filter((v) => v.market)}>
            {(v) => <span classList={{ down: (tape()?.status[v.venue] ?? 'connecting') !== 'live' }}><ExchangeLogo exchange={exchangeOfVenue(v.venue)} size={12} /></span>}
          </For>
        </button>
        <span class="spacer" />
        <select title="Smallest hit to show in the tape" onChange={(e) => patchPanel(id, { minUsd: Number(e.currentTarget.value) })}>
          <For each={[0, 1_000, 10_000, 100_000]}>
            {(v) => (
              <option value={v} selected={min() === v}>
                {v === 0 ? 'All sizes' : `≥ ${usd(v)}`}
              </option>
            )}
          </For>
        </select>
      </div>

      <Show
        when={tracked()}
        fallback={
          <div class="note" classList={{ warn: statsError() }}>
            {untracked()
              ? `The server keeps 24 hours for ${untracked()!.join(', ') || 'no ticker'}. ${ticker()} streams live below; add it to TRADOOR_LIQUIDATIONS to keep its totals.`
              : statsError()
                ? '▲ The server is not reachable, so there are no totals. The tape below still streams.'
                : 'Loading totals…'}
          </div>
        }
      >
        {(s) => (
          <>
            <div class="liq-totals">
              <For each={s().windows}>{(w) => <Total w={w} partial={s().coverageFrom > s().now - (WINDOW_MS[w.window] ?? 0)} />}</For>
            </div>
            {/* The longer windows are not whole until the server has listened that long. */}
            <Show when={s().coverageFrom > s().now - WINDOW_MS['24h']!}>
              <div class="note">
                Kept since {formatHm(s().coverageFrom)} {zoneLabel()} — a window marked * is still filling.
              </div>
            </Show>
            <Show when={serverGaps().length > 0}>
              <div class="note warn">▲ Server: {serverGaps().join(', ')} — totals miss those hits until it is back.</div>
            </Show>
          </>
        )}
      </Show>

      <Show when={venuesOpen()}>
        <div class="liq-venues-pop" onMouseEnter={() => showVenues(true)} onMouseLeave={() => showVenues(false)}>
          <table class="grid tape venues">
            <thead>
              <tr>
                <th>Exchange</th>
                <th>Status</th>
                <th>Hits</th>
              </tr>
            </thead>
            <tbody>
              <For each={tape()?.venues() ?? []}>
                {(v) => {
                  const exchange = exchangeOfVenue(v.venue);
                  const state = (): VenueStatus => tape()?.status[v.venue] ?? 'connecting';
                  return (
                    <tr classList={{ off: !v.market }}>
                      <td title={v.market ? `${exchange.name} ${symbolOf(v.market)} perpetual` : `${exchange.name} does not list ${ticker()}`}>
                        <ExchangeLogo exchange={exchange} size={12} />
                        <span>{exchange.name}</span>
                        {/* Named only where the exchange spells the ticker its own way. */}
                        <Show when={v.market && parseMarket(v.market) && symbolOf(v.market) !== ticker() ? symbolOf(v.market) : null}>{(sym) => <span class="dim">{sym()}</span>}</Show>
                      </td>
                      <td>
                        <span class={`conn ${state()}`}>{STATUS_LABEL[state()]}</span>
                      </td>
                      <td class="num">{v.market ? `${perVenue(v.venue).length} · ${usd(sum(perVenue(v.venue)))}` : '—'}</td>
                    </tr>
                  );
                }}
              </For>
            </tbody>
          </table>
        </div>
      </Show>

      <div class="scroll">
        <Show when={hits().length > 0} fallback={<div class="empty">Waiting for the next liquidation on any exchange.</div>}>
          <table class="grid tape">
            <thead>
              <tr>
                <th>Time</th>
                <th>Side</th>
                <th>Price</th>
                <th>Size</th>
              </tr>
            </thead>
            <tbody>
              <For each={hits()}>
                {(h) => (
                  <tr classList={{ big: h.usd >= 100_000 }}>
                    <td class="num dim">{formatClock(h.time)}</td>
                    <td class={h.long ? 'bear' : 'bull'} title={`${exchangeOfVenue(h.venue).name} · ${h.long ? 'a long' : 'a short'} closed out`}>
                      <ExchangeLogo exchange={exchangeOfVenue(h.venue)} size={11} />
                      {h.long ? 'LONG' : 'SHORT'}
                    </td>
                    <td class="num">{formatPrice(h.price, tick())}</td>
                    <td class="num">{usd(h.usd)}</td>
                  </tr>
                )}
              </For>
            </tbody>
          </table>
        </Show>
      </div>
    </div>
  );
}

/** One window: the total, how it splits between longs and shorts, and a bar of the split. */
function Total(props: { w: LiquidationWindow; partial: boolean }) {
  const total = () => props.w.longUsd + props.w.shortUsd;
  const longShare = () => (total() > 0 ? (props.w.longUsd / total()) * 100 : 50);
  return (
    <div class="win" title={`${props.w.longs} longs and ${props.w.shorts} shorts closed out in the last ${props.w.window}${props.partial ? ' — the server has not listened that long yet' : ''}`}>
      <span class="caps dim">
        {props.w.window.toUpperCase()}
        {props.partial ? '*' : ''}
      </span>
      <span class="num sum">{usd(total())}</span>
      <span class="num bear">L {usd(props.w.longUsd)}</span>
      <span class="num bull">S {usd(props.w.shortUsd)}</span>
      <span class="split">
        <span class="long" style={{ width: `${longShare()}%` }} />
      </span>
    </div>
  );
}
