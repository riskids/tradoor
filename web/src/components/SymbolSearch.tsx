/**
 * The symbol picker: every market on every venue, searched locally from one
 * catalogue response. Opened by clicking a symbol or simply by typing; the
 * keystroke that opened it is already in the box.
 */
import { createEffect, createMemo, createResource, createSignal, For, on, onMount, Show } from 'solid-js';
import { catalog, errorText, type Product } from '../lib/api';
import { formatPct, formatPrice, formatTurnover, priceDecimals } from '../lib/format';
import { ExchangeLogo, exchangeOfVenue, LIVE_EXCHANGES, type ExchangeId } from '../lib/exchanges';
import { isChartMarket, isMarket, ratioOf, venueLabel } from '../lib/markets';

type Kind = 'all' | 'perp' | 'spot';

const DOLLARS = new Set(['USDT', 'USDC', 'USD', 'FDUSD', 'BUSD']);

/** Exact symbol, then base, then prefixes, then substrings; ties by 24h turnover.
 *  Words like "bybit" or "perp" in the query filter instead of matching, and
 *  TradingView's spelling works too: `BYBIT:XPLUSDT.P` is Bybit's perpetual. */
function rank(products: readonly Product[], query: string, kindFilter: Kind, exchangeFilter: ExchangeId | null): Product[] {
  let exchange: string | null = exchangeFilter;
  let kind = kindFilter;
  const terms: string[] = [];
  for (let word of query.toUpperCase().split(/[\s/:-]+/).filter(Boolean)) {
    if (word.endsWith('.P')) {
      kind = 'perp';
      word = word.slice(0, -2);
    }
    if (LIVE_EXCHANGES.some((e) => e.id === word.toLowerCase())) exchange = word.toLowerCase();
    else if (word === 'PERP' || word === 'SPOT') kind = word.toLowerCase() as Kind;
    // TradingView's index and other prefixes name no exchange of ours.
    else if (word !== 'CRYPTO' && word !== 'INDEX') terms.push(word.replace(/[^A-Z0-9]/g, ''));
  }
  const needle = terms.join('');
  const scored: [number, number, Product][] = [];
  for (const p of products) {
    if (kind !== 'all' && p.kind !== kind) continue;
    if (exchange && !p.venue.startsWith(exchange)) continue;
    let score = 1;
    if (needle) {
      if (p.symbol === needle) score = 100;
      else if (p.base === needle) score = 90;
      else if (p.symbol.startsWith(needle)) score = 70;
      else if (p.base.startsWith(needle)) score = 60;
      else if (p.symbol.includes(needle)) score = 30;
      else continue;
    }
    // Turnover is counted in the quote: a BTC/IDR market's runs to trillions
    // of rupiah, so among equals a dollar-quoted market comes first.
    scored.push([score + (DOLLARS.has(p.quote) ? 0.5 : 0), p.quoteVolume24h ?? 0, p]);
  }
  scored.sort((a, b) => b[0] - a[0] || b[1] - a[1]);
  return scored.slice(0, 100).map((s) => s[2]);
}

/** A quote currency on its own: `BTC/USDT` is the pair being asked for, not BTC priced in USDT. */
const QUOTES = new Set(['USDT', 'USDC', 'USD', 'BUSD', 'FDUSD', 'TUSD', 'DAI', 'EUR']);

/**
 * `xpl bybit / btc binance spot`: each side searched on its own, its words
 * filtering only it, and the best of each made into one market priced in the
 * other. Null unless both sides find a market, and neither is a bare quote.
 */
function ratio(products: readonly Product[], query: string): [Product, Product] | null {
  const sides = query.split('/');
  if (sides.length !== 2) return null;
  const pick = (side: string) => {
    const bare = side.trim().toUpperCase().replace(/^[A-Z]+:/, '').replace(/\.P$/, '');
    if (bare.length < 2 || QUOTES.has(bare)) return null;
    return rank(products, side, 'all', null)[0] ?? null;
  };
  const a = pick(sides[0]!);
  const b = pick(sides[1]!);
  return a && b && a.market !== b.market ? [a, b] : null;
}

const lastRatio = ([a, b]: [Product, Product]): number | null => (a.lastPrice && b.lastPrice ? a.lastPrice / b.lastPrice : null);

export function SymbolSearch(props: { initial: string; title?: string; current?: string; ratios?: boolean; onSelect(market: string): void; onClose(): void }) {
  const [query, setQuery] = createSignal(props.initial);
  const [kind, setKind] = createSignal<Kind>('all');
  const [exchange, setExchange] = createSignal<ExchangeId | null>(null);
  const [cursor, setCursor] = createSignal(0);
  const [markets] = createResource(() => catalog());
  const products = () => (markets.state === 'ready' ? markets()!.products : []);
  const rows = createMemo(() => rank(products(), query(), kind(), exchange()));
  /** The ratio the query spells, first in the list when there is one. */
  const pair = createMemo(() => (props.ratios ? ratio(products(), query()) : null));
  const first = () => (pair() ? 1 : 0);
  const typed = () => {
    const q = query().trim();
    return (props.ratios ? isChartMarket(q) : isMarket(q)) ? q : null;
  };
  const pick = (i: number) => {
    const p = pair();
    if (p && i === 0) return ratioOf(p[0].market, p[1].market);
    return rows()[i - first()]?.market ?? null;
  };

  let input!: HTMLInputElement;
  let list!: HTMLDivElement;
  onMount(() => {
    input.focus();
    // Caret at the end, never select-all: the next keystroke must extend the query.
    input.setSelectionRange(input.value.length, input.value.length);
  });
  createEffect(on([query, kind, exchange], () => setCursor(0), { defer: true }));
  createEffect(() => {
    cursor();
    list.querySelector('[data-cur]')?.scrollIntoView({ block: 'nearest' });
  });

  const onKeyDown = (e: KeyboardEvent) => {
    const n = rows().length + first();
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (n > 0) setCursor((c) => (c + (e.key === 'ArrowDown' ? 1 : -1) + n) % n);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const market = pick(cursor()) ?? typed();
      if (market) props.onSelect(market);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      props.onClose();
    } else if (e.key === 'Tab') {
      e.preventDefault();
      setKind((k) => (k === 'all' ? 'perp' : k === 'perp' ? 'spot' : 'all'));
    }
  };

  const unavailable = () => (markets.state === 'ready' ? markets()!.venues.filter((v) => v.count === 0).map((v) => v.label) : []);

  return (
    <div class="backdrop" onPointerDown={() => props.onClose()}>
      <div class="dialog" onPointerDown={(e) => e.stopPropagation()} onKeyDown={onKeyDown}>
        <header>
          <span class="title caps">{props.title ?? 'Symbol'}</span>
          <input
            ref={input}
            class="q"
            value={query()}
            onInput={(e) => setQuery(e.currentTarget.value)}
            placeholder={props.ratios ? 'ticker — BTC, sol perp, bybit btc… or a ratio: xpl bybit / btc binance' : 'ticker — BTC, ETHUSDT, sol perp, bybit btc…'}
            spellcheck={false}
            autocomplete="off"
          />
          <div class="chips">
            <For each={LIVE_EXCHANGES}>
              {(x) => (
                <button class="xchip" classList={{ on: exchange() === x.id }} data-tip={exchange() === x.id ? `Showing ${x.name} only — click for every exchange` : `${x.name} only`} onClick={() => setExchange((e) => (e === x.id ? null : x.id))}>
                  <ExchangeLogo exchange={x} size={14} />
                </button>
              )}
            </For>
            <span class="sep-v" />
            <For each={['all', 'perp', 'spot'] as const}>
              {(k) => (
                <button classList={{ on: kind() === k }} onClick={() => setKind(k)}>
                  {k.toUpperCase()}
                </button>
              )}
            </For>
          </div>
          <button class="x" onClick={() => props.onClose()}>
            esc ✕
          </button>
        </header>
        <div ref={list} class="list">
          <Show when={markets.state !== 'errored'} fallback={<div class="empty warn">▲ The market list is unavailable: {errorText(markets.error)}. A market typed as venue:SYMBOL — binance-perp:BTCUSDT — still opens with Enter.</div>}>
            <Show when={markets.state === 'ready'} fallback={<div class="empty">Loading markets from every venue…</div>}>
              <Show when={pair()}>
                {(p) => (
                  <div class="row ratio" classList={{ cur: cursor() === 0 }} data-cur={cursor() === 0 ? '' : undefined} onMouseMove={() => setCursor(0)} onClick={() => props.onSelect(ratioOf(p()[0].market, p()[1].market))}>
                    <span class="sym">
                      {p()[0].symbol} / {p()[1].symbol}
                    </span>
                    <span class="dim grow">
                      {venueLabel(p()[0].market)} / {venueLabel(p()[1].market)}
                    </span>
                    <span class="venue" style={{ 'min-width': '84px' }}>
                      <ExchangeLogo exchange={exchangeOfVenue(p()[0].venue)} size={13} />
                      <span class="dim">/</span>
                      <ExchangeLogo exchange={exchangeOfVenue(p()[1].venue)} size={13} />
                    </span>
                    <span class="badge">RATIO</span>
                    <span class="col num muted">{formatPrice(lastRatio(p()), priceDecimals(lastRatio(p()) ?? 0))}</span>
                    <span class="col s" />
                    <span class="col s" />
                  </div>
                )}
              </Show>
              <Show when={rows().length > 0 || pair()} fallback={<div class="empty">Nothing matches “{query()}”.</div>}>
                <For each={rows()}>
                  {(p, i) => (
                    <div class="row" classList={{ cur: i() + first() === cursor() }} data-cur={i() + first() === cursor() ? '' : undefined} onMouseMove={() => setCursor(i() + first())} onClick={() => props.onSelect(p.market)}>
                      <span class="sym">{p.symbol}</span>
                      <span class="dim grow">
                        {p.base}/{p.quote}
                        <Show when={p.market === props.current}>
                          <span class="primary"> · on screen</span>
                        </Show>
                      </span>
                      <span class="venue" style={{ 'min-width': '84px' }}>
                        <ExchangeLogo exchange={exchangeOfVenue(p.venue)} size={13} />
                        <span class="venue-name">{exchangeOfVenue(p.venue).name}</span>
                      </span>
                      <span class="badge" classList={{ bull: p.kind === 'perp' }}>
                        {p.kind.toUpperCase()}
                      </span>
                      <span class="col num muted">{formatPrice(p.lastPrice, p.priceDecimals ?? priceDecimals(p.lastPrice))}</span>
                      <span class="col s num" classList={{ bull: (p.changePct ?? 0) > 0, bear: (p.changePct ?? 0) < 0 }}>
                        {formatPct(p.changePct)}
                      </span>
                      <span class="col s num dim">{formatTurnover(p.quoteVolume24h)}</span>
                    </div>
                  )}
                </For>
              </Show>
            </Show>
          </Show>
        </div>
        <footer>
          <span>
            {rows().length} of {products().length} markets
            {unavailable().length > 0 ? ` · ${unavailable().join(', ')} unavailable` : ''}
          </span>
          <span>↑↓ move · ↵ open · tab perp/spot · esc close</span>
        </footer>
      </div>
    </div>
  );
}
