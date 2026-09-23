/**
 * Markets worth a glance: last price and the day's change, streamed. Clicking a
 * row drives the linked group, or the last chart used. The list never reorders
 * itself — knowing where each symbol sits is the point of a fixed column.
 */
import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js';
import { createStore } from 'solid-js/store';
import { setOverlay } from '../app/ui';
import { nativeOf } from '../lib/native';
import { StreamHub } from '../engine/streams';
import { catalog, type Catalog } from '../lib/api';
import { formatPct, formatPrice, formatTurnover, priceDecimals } from '../lib/format';
import { Icon } from '../lib/icons';
import { ExchangeLogo, exchangeOfVenue } from '../lib/exchanges';
import { isPerp, parseMarket, symbolOf, venueLabel } from '../lib/markets';
import type { WatchlistPanelState } from '../workspace/model';
import { doc, marketOf, patchPanel, setLinkMarket, setPanelMarket, targetChart } from '../workspace/store';

/** Tickers are few and slow, so the page keeps its own sockets off the engine thread. */
const hub = new StreamHub();

interface Ticker {
  last: number;
  changePct: number | null;
  quoteVolume: number | null;
}

export function WatchlistPanel(props: { panel: WatchlistPanelState }) {
  const id = props.panel.id;
  const [tickers, setTickers] = createStore<Record<string, Ticker>>({});
  const [products, setProducts] = createSignal<Catalog | null>(null);
  const [dragged, setDragged] = createSignal<string | null>(null);

  catalog().then(setProducts, () => {});

  // Seed from the catalogue so rows are not blank until the first tick.
  createEffect(() => {
    const c = products();
    if (!c) return;
    for (const m of props.panel.markets) {
      const p = c.byMarket.get(m);
      if (p?.lastPrice != null && !tickers[m]) setTickers(m, { last: p.lastPrice, changePct: p.changePct, quoteVolume: p.quoteVolume24h });
    }
  });

  createEffect(() => {
    let live = true;
    const offs: (() => void)[] = [];
    for (const m of props.panel.markets) {
      const ref = parseMarket(m);
      if (!ref) continue;
      // The exchange's own name for it, which may take the catalogue to know.
      void nativeOf(m).then((symbol) => {
        if (!live) return;
        offs.push(
          hub.subscribe(
            ref.venue,
            { kind: 'ticker', symbol },
            {
              message: (msg) => {
                if (msg.kind === 'ticker') setTickers(m, { last: msg.last, changePct: msg.changePct, quoteVolume: msg.quoteVolume });
              },
              state: () => {},
            },
          ),
        );
      });
    }
    onCleanup(() => {
      live = false;
      for (const off of offs.splice(0)) off();
    });
  });

  const current = (): string | null => {
    if (props.panel.link) return doc.links[props.panel.link].market;
    const chart = targetChart();
    return chart ? marketOf(chart) : null;
  };

  const open = (market: string) => {
    if (props.panel.link) {
      setLinkMarket(props.panel.link, market);
      return;
    }
    const chart = targetChart();
    if (chart) setPanelMarket(chart.id, market);
  };

  const remove = (market: string) => patchPanel(id, { markets: props.panel.markets.filter((m) => m !== market) });

  const moveBefore = (market: string, before: string) => {
    if (market === before) return;
    const list = props.panel.markets.filter((m) => m !== market);
    list.splice(list.indexOf(before), 0, market);
    patchPanel(id, { markets: list });
  };

  const digits = (market: string, last: number) => products()?.byMarket.get(market)?.priceDecimals ?? priceDecimals(last);

  return (
    <div class="panelcol wl">
      <div class="toolbar">
        <span class="caps" style={{ padding: '0 4px' }}>
          Watchlist
        </span>
        <span class="dim">{props.panel.markets.length}</span>
        <span class="spacer" />
        <button class="iconbtn" data-tip="Add a market" onClick={() => setOverlay({ kind: 'symbol', panelId: id, initial: '' })}>
          <Icon name="plus" size={13} />
        </button>
      </div>
      <div class="wl-head">
        <span>Symbol</span>
        <span class="r">Last</span>
        <span class="r chg">Chg%</span>
      </div>
      <div class="scroll">
        <For each={props.panel.markets}>
          {(market) => {
            const t = () => tickers[market];
            return (
              <div
                class="wl-row"
                classList={{ active: market === current() }}
                draggable={true}
                data-tip={`${symbolOf(market)} · ${venueLabel(market)}${t()?.quoteVolume ? ` · 24h turnover ${formatTurnover(t()!.quoteVolume)}` : ''}`}
                onClick={() => open(market)}
                onDragStart={(e) => {
                  e.dataTransfer?.setData('text/plain', market);
                  setDragged(market);
                }}
                onDragEnd={() => setDragged(null)}
                onDragOver={(e) => {
                  if (dragged()) e.preventDefault();
                }}
                onDrop={(e) => {
                  e.preventDefault();
                  const m = dragged();
                  if (m) moveBefore(m, market);
                  setDragged(null);
                }}
              >
                <span class="sym">
                  <ExchangeLogo exchange={exchangeOfVenue(parseMarket(market)?.venue ?? 'binance')} size={13} />
                  <span class="name">{symbolOf(market)}</span>
                  <span class="kind">{isPerp(market) ? 'PERP' : 'SPOT'}</span>
                </span>
                <span class="num r muted">{t() ? formatPrice(t()!.last, digits(market, t()!.last)) : '—'}</span>
                <span class="num r chg" classList={{ bull: (t()?.changePct ?? 0) > 0, bear: (t()?.changePct ?? 0) < 0, dim: !t() }}>
                  {formatPct(t()?.changePct)}
                </span>
                <button
                  class="x"
                  data-tip="Remove from the watchlist"
                  onClick={(e) => {
                    e.stopPropagation();
                    remove(market);
                  }}
                >
                  <Icon name="close" size={10} stroke={2.4} />
                </button>
              </div>
            );
          }}
        </For>
        <Show when={props.panel.markets.length === 0}>
          <div class="empty">Empty. Add a market with + above.</div>
        </Show>
      </div>
    </div>
  );
}
