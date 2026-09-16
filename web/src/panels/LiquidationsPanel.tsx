/**
 * Liquidations as they land: the exchange closing someone out, newest first.
 * The tape lives in this tab only — nothing is stored, so a reload starts it
 * empty. Perpetuals only; spot has no positions to liquidate.
 */
import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js';
import { StreamHub } from '../engine/streams';
import { catalog } from '../lib/api';
import { demoHit, demoRate } from '../lib/demo';
import { ExchangeLogo, exchangeOfVenue } from '../lib/exchanges';
import { formatPrice, formatTurnover, priceDecimals } from '../lib/format';
import { isPerp, parseMarket, symbolOf } from '../lib/markets';
import type { LiquidationsPanelState } from '../workspace/model';
import { marketOf, patchPanel } from '../workspace/store';
import { MarketButton } from './common';

/** Liquidations are rare and small, so the page keeps its own socket. */
const hub = new StreamHub();

/** Rows held before the oldest drop off — a long enough tape to scroll back. */
const MAX_ROWS = 300;

interface Hit {
  key: number;
  time: number;
  price: number;
  qty: number;
  usd: number;
  /** A long was liquidated: the exchange sold someone out. */
  long: boolean;
}

const clock = (ms: number): string => new Date(ms).toISOString().slice(11, 19);

export function LiquidationsPanel(props: { panel: LiquidationsPanelState }) {
  const id = props.panel.id;
  const market = () => marketOf(props.panel);
  const [hits, setHits] = createSignal<Hit[]>([]);
  const [live, setLive] = createSignal(false);

  const perp = () => isPerp(market());
  // From the market's own last price: a liquidation's price carries no tick of
  // its own, and a demo hit carries a float's worth of noise.
  const [tick, setTick] = createSignal(2);
  createEffect(() => {
    const m = market();
    void catalog().then((c) => setTick(priceDecimals(c.byMarket.get(m)?.lastPrice ?? 0)));
  });
  const min = () => props.panel.minUsd;
  const shown = () => hits().filter((h) => h.usd >= min());

  createEffect(() => {
    const ref = parseMarket(market());
    setHits([]);
    if (!ref || !isPerp(market())) {
      setLive(false);
      return;
    }
    let key = 0;
    const add = (hit: { time: number; price: number; qty: number; long: boolean }) => {
      setHits((list) => [{ key: key++, ...hit, usd: hit.price * hit.qty }, ...list].slice(0, MAX_ROWS));
    };

    // Development only: the same invented hits the chart's feed shows.
    if (demoRate() > 0) {
      let last = 0;
      void catalog().then((c) => {
        last = c.byMarket.get(market())?.lastPrice ?? 0;
      });
      const timer = setInterval(() => {
        if (last > 0) add({ time: Date.now(), ...demoHit(last) });
      }, 60_000 / demoRate());
      setLive(true);
      onCleanup(() => clearInterval(timer));
      return;
    }

    const off = hub.subscribe(
      ref.venue,
      { kind: 'liquidations', symbol: ref.symbol },
      {
        message: (msg) => {
          if (msg.kind !== 'liquidation') return;
          add({ time: msg.time, price: msg.price, qty: msg.qty, long: msg.long });
        },
        state: (s) => setLive(s === 'live'),
      },
    );
    onCleanup(off);
  });

  const total = () => shown().reduce((sum, h) => sum + h.usd, 0);
  const longs = () => shown().reduce((sum, h) => (h.long ? sum + h.usd : sum), 0);

  return (
    <div class="panelcol">
      <div class="toolbar">
        <MarketButton panelId={id} market={market()} />
        <span class="sep" />
        <span class="caps">Min&nbsp;</span>
        <For each={[0, 1_000, 10_000, 100_000]}>
          {(v) => (
            <button class="tb" classList={{ on: min() === v }} title={v === 0 ? 'Every liquidation' : `Only hits of $${formatTurnover(v)} and up`} onClick={() => patchPanel(id, { minUsd: v })}>
              {v === 0 ? 'All' : `$${formatTurnover(v)}`}
            </button>
          )}
        </For>
        <span class="spacer" />
        <Show when={perp() && live()}>
          <span class="state caps">live</span>
        </Show>
      </div>

      <Show when={perp()} fallback={<div class="empty">{symbolOf(market())} is a spot market — only perpetuals have positions to liquidate.</div>}>
        <div class="subhead">
          <span class="caps">
            {shown().length} hit{shown().length === 1 ? '' : 's'} this session
          </span>
          <span class="dim num">
            <span class="bear">${formatTurnover(longs())}</span> longs · <span class="bull">${formatTurnover(total() - longs())}</span> shorts
          </span>
        </div>
        <div class="scroll">
          <Show when={shown().length > 0} fallback={<div class="empty">Waiting for the first liquidation. Nothing is kept: a reload starts this tape over.</div>}>
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
                <For each={shown()}>
                  {(h) => (
                    <tr classList={{ big: h.usd >= 100_000 }}>
                      <td class="num dim">{clock(h.time)}</td>
                      <td class={h.long ? 'bear' : 'bull'}>
                        <ExchangeLogo exchange={exchangeOfVenue(parseMarket(market())?.venue ?? 'binance')} size={11} />
                        {h.long ? 'LONG' : 'SHORT'}
                      </td>
                      <td class="num">{formatPrice(h.price, tick())}</td>
                      <td class="num">${formatTurnover(h.usd)}</td>
                    </tr>
                  )}
                </For>
              </tbody>
            </table>
          </Show>
        </div>
      </Show>
    </div>
  );
}
