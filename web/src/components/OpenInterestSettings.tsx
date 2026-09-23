/**
 * The aggregated open interest study's inputs, as Velo lays them out: which
 * venues are summed, coins or dollars, candles or a line. Its parameters are
 * its identity, so Ok swaps the study for one with the new ones.
 */
import { createSignal, For, onMount } from 'solid-js';
import { OI_ALL, OI_VENUES } from '../engine/open-interest';

const LABELS: Record<(typeof OI_VENUES)[number], string> = {
  'binance-usdt': 'Binance USDT Perp',
  'bybit-usdt': 'Bybit USDT Perp',
  'okx-usdt': 'OKX USDT Swap',
  'deribit-usd': 'Deribit USD Perp',
  'hyperliquid-usd': 'Hyperliquid USD Perp',
  'binance-usd': 'Binance USD Perp',
  'bybit-usd': 'Bybit USD Perp',
  'okx-usd': 'OKX USD Perp',
};

/** `oi:venues:measure:view`, filling what an older id leaves out. */
export function parseOi(id: string): { venues: number; measure: number; view: number } {
  const [, v, m, w] = id.split(':');
  return { venues: Number(v ?? OI_ALL) || OI_ALL, measure: Number(m ?? 0) === 1 ? 1 : 0, view: Number(w ?? 0) === 1 ? 1 : 0 };
}

export function OpenInterestSettings(props: { id: string; onApply(id: string): void; onClose(): void }) {
  const start = parseOi(props.id);
  const [venues, setVenues] = createSignal(start.venues);
  const [measure, setMeasure] = createSignal(start.measure);
  const [view, setView] = createSignal(start.view);
  let panel!: HTMLDivElement;
  onMount(() => queueMicrotask(() => panel.focus()));

  const ok = () => {
    props.onApply(`oi:${venues()}:${measure()}:${view()}`);
    props.onClose();
  };

  return (
    <div
      ref={panel}
      class="dsettings oi"
      tabIndex={-1}
      onPointerDown={(e) => e.stopPropagation()}
      onWheel={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === 'Escape') props.onClose();
        else if (e.key === 'Enter') ok();
      }}
    >
      <header>
        <span class="title">Aggregated Open Interest</span>
        <span class="spacer" />
        <button class="x" data-tip="Cancel (esc)" onClick={() => props.onClose()}>
          ✕
        </button>
      </header>
      <nav class="dtabs">
        <button class="on">Inputs</button>
      </nav>
      <div class="dbody">
        <For each={OI_VENUES}>
          {(v, i) => (
            <label class="dcheck">
              <input
                type="checkbox"
                checked={(venues() & (1 << i())) !== 0}
                onChange={(e) => {
                  const next = e.currentTarget.checked ? venues() | (1 << i()) : venues() & ~(1 << i());
                  // At least one venue: an empty sum is not a study.
                  if (next === 0) e.currentTarget.checked = true;
                  else setVenues(next);
                }}
              />
              {LABELS[v]}
            </label>
          )}
        </For>
        <div class="drow">
          <span class="dlabel">Measure</span>
          <div class="dctl">
            <select class="dinput" value={String(measure())} onChange={(e) => setMeasure(Number(e.currentTarget.value))}>
              <option value="0">Coins</option>
              <option value="1">USD</option>
            </select>
          </div>
        </div>
        <div class="drow">
          <span class="dlabel">View</span>
          <div class="dctl">
            <select class="dinput" value={String(view())} onChange={(e) => setView(Number(e.currentTarget.value))}>
              <option value="0">Candles</option>
              <option value="1">Line</option>
            </select>
          </div>
        </div>
        <p class="dnote">Recorded by the tradoor server every 15 seconds. Binance and Bybit are backfilled for the last month and OKX for five days. Before a venue's own record begins — Deribit, Hyperliquid and Binance USD count from when recording started — it is estimated at the share it held when it began.</p>
      </div>
      <footer>
        <button
          class="dbtn"
          onClick={() => {
            setVenues(OI_ALL);
            setMeasure(0);
            setView(0);
          }}
        >
          Defaults
        </button>
        <span class="spacer" />
        <button class="dbtn" onClick={() => props.onClose()}>
          Cancel
        </button>
        <button class="dbtn primary" onClick={ok}>
          Ok
        </button>
      </footer>
    </div>
  );
}
