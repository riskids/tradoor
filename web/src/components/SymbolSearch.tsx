/**
 * The symbol picker: every market on every venue, searched locally from one
 * catalogue response. Opened by clicking a symbol or simply by typing; the
 * keystroke that opened it is already in the box.
 */
import { createEffect, createMemo, createResource, createSignal, For, on, onMount, Show } from 'solid-js';
import { catalog, errorText, type Product } from '../lib/api';
import { formatPct, formatPrice, formatTurnover, priceDecimals } from '../lib/format';
import { ExchangeLogo, exchangeOfVenue, LIVE_EXCHANGES, type ExchangeId } from '../lib/exchanges';
import { isMarket } from '../lib/markets';

type Kind = 'all' | 'perp' | 'spot';

/** Exact symbol, then base, then prefixes, then substrings; ties by 24h turnover.
 *  Words like "bybit" or "perp" in the query filter instead of matching. */
function rank(products: readonly Product[], query: string, kindFilter: Kind, exchangeFilter: ExchangeId | null): Product[] {
  let exchange: string | null = exchangeFilter;
  let kind = kindFilter;
  const terms: string[] = [];
  for (const word of query.toUpperCase().split(/[\s/:-]+/).filter(Boolean)) {
    if (word === 'BINANCE' || word === 'BYBIT') exchange = word.toLowerCase();
    else if (word === 'PERP' || word === 'SPOT') kind = word.toLowerCase() as Kind;
    else terms.push(word.replace(/[^A-Z0-9]/g, ''));
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
    scored.push([score, p.quoteVolume24h ?? 0, p]);
  }
  scored.sort((a, b) => b[0] - a[0] || b[1] - a[1]);
  return scored.slice(0, 100).map((s) => s[2]);
}

export function SymbolSearch(props: { initial: string; title?: string; current?: string; onSelect(market: string): void; onClose(): void }) {
  const [query, setQuery] = createSignal(props.initial);
  const [kind, setKind] = createSignal<Kind>('all');
  const [exchange, setExchange] = createSignal<ExchangeId | null>(null);
  const [cursor, setCursor] = createSignal(0);
  const [markets] = createResource(() => catalog());
  const products = () => (markets.state === 'ready' ? markets()!.products : []);
  const rows = createMemo(() => rank(products(), query(), kind(), exchange()));
  const typed = () => {
    const q = query().trim();
    return isMarket(q) ? q : null;
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
    const n = rows().length;
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      if (n > 0) setCursor((c) => (c + (e.key === 'ArrowDown' ? 1 : -1) + n) % n);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const market = rows()[cursor()]?.market ?? typed();
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
            placeholder="ticker — BTC, ETHUSDT, sol perp, bybit btc…"
            spellcheck={false}
            autocomplete="off"
          />
          <div class="chips">
            <For each={LIVE_EXCHANGES}>
              {(x) => (
                <button class="xchip" classList={{ on: exchange() === x.id }} title={exchange() === x.id ? `Showing ${x.name} only — click for every exchange` : `${x.name} only`} onClick={() => setExchange((e) => (e === x.id ? null : x.id))}>
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
              <Show when={rows().length > 0} fallback={<div class="empty">Nothing matches “{query()}”.</div>}>
                <For each={rows()}>
                  {(p, i) => (
                    <div class="row" classList={{ cur: i() === cursor() }} data-cur={i() === cursor() ? '' : undefined} onMouseMove={() => setCursor(i())} onClick={() => props.onSelect(p.market)}>
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
