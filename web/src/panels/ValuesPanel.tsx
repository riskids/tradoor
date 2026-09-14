/**
 * Indicator values on the last closed bar, and the prior session's profile
 * levels — to be compared number by number against TradingView. Parameters
 * match its defaults (RSI 14, StochRSI 14/14/3/3, UTC).
 */
import { batch, createEffect, createSignal, For, on, onCleanup, Show } from 'solid-js';
import { call } from '../engine/client';
import type { IndicatorValues } from '../engine/protocol';
import { errorText } from '../lib/api';
import { formatPrice, formatUtc } from '../lib/format';
import { tfName } from '../lib/timeframes';
import type { ValuesPanelState } from '../workspace/model';
import { marketOf, patchPanel, setPanelTimeframe, timeframeOf } from '../workspace/store';
import { MarketButton, SESSION_NAME, TimeframeSelect } from './common';

export function ValuesPanel(props: { panel: ValuesPanelState }) {
  const id = props.panel.id;
  const market = () => marketOf(props.panel);
  const timeframe = () => timeframeOf(props.panel);
  const [values, setValues] = createSignal<IndicatorValues | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [loading, setLoading] = createSignal(false);

  let request = 0;
  const load = async () => {
    const req = ++request;
    setLoading(true);
    try {
      const v = await call('values', { market: market(), timeframe: timeframe(), basis: props.panel.basis });
      if (req === request) {
        batch(() => {
          setValues(v);
          setError(null);
        });
      }
    } catch (e) {
      if (req === request) setError(errorText(e));
    } finally {
      if (req === request) setLoading(false);
    }
  };

  createEffect(
    on([market, timeframe, () => props.panel.basis], () => {
      setValues(null);
      void load();
    }),
  );
  const timer = setInterval(() => void load(), 30_000);
  onCleanup(() => clearInterval(timer));

  const rows = (v: IndicatorValues) => [
    { label: 'CLOSE', value: v.close, digits: v.precision },
    { label: 'EMA 12', value: v.ema12, digits: v.precision },
    { label: 'EMA 25', value: v.ema25, digits: v.precision },
    { label: 'EMA 200', value: v.ema200, digits: v.precision },
    { label: 'RSI 14', value: v.rsi14, digits: 2 },
    { label: 'STOCHRSI K', value: v.stochK, digits: 2 },
    { label: 'STOCHRSI D', value: v.stochD, digits: 2 },
  ];

  return (
    <div class="panelcol">
      <div class="toolbar">
        <MarketButton panelId={id} market={market()} />
        <span class="sep" />
        <TimeframeSelect value={timeframe()} onChange={(tf) => setPanelTimeframe(id, tf)} />
        <span class="sep" />
        <span class="caps">Profile&nbsp;</span>
        <button class="tb" classList={{ on: props.panel.basis === 'volume' }} title="Levels weighted by volume traded at each price" onClick={() => patchPanel(id, { basis: 'volume' })}>
          Volume
        </button>
        <button class="tb" classList={{ on: props.panel.basis === 'tpo' }} title="Levels weighted by time spent at each price" onClick={() => patchPanel(id, { basis: 'tpo' })}>
          TPO
        </button>
        <span class="spacer" />
        <Show when={loading()}>
          <span class="dim pulse">●</span>
        </Show>
      </div>
      <div class="scroll">
        <Show when={values()} fallback={<div class="empty" classList={{ warn: !!error() }}>{error() ? `▲ ${error()}` : loading() ? 'Computing…' : 'No closed candles yet.'}</div>}>
          {(v) => (
            <>
              <div class="subhead">
                <span class="caps">Last closed bar</span>
                <span class="dim num">
                  {tfName(timeframe())} · {formatUtc(v().time)}
                </span>
              </div>
              <table class="grid">
                <tbody>
                  <For each={rows(v())}>
                    {(r) => (
                      <tr>
                        <td>{r.label}</td>
                        <td>
                          {r.value === null ? (
                            <span class="warn" title="Not enough history for this period — unknown, not zero">
                              no history
                            </span>
                          ) : (
                            formatPrice(r.value, r.digits)
                          )}
                        </td>
                      </tr>
                    )}
                  </For>
                </tbody>
              </table>
              <Show when={v().profile} fallback={<div class="note">{tfName(timeframe())} has no market profile — profiles describe intraday, daily and weekly sessions.</div>}>
                {(p) => (
                  <>
                    <div class="subhead">
                      <span class="caps">Market profile · {p().basis === 'tpo' ? 'TPO (time)' : 'Volume'}</span>
                      <span class="dim">
                        prior {SESSION_NAME[p().session] ?? p().session} session · from {p().from} candles
                      </span>
                    </div>
                    <Show when={p().covered > 0} fallback={<div class="note warn">▲ No prior session could be built from the {p().from} candles held.</div>}>
                      <table class="grid">
                        <tbody>
                          <For each={[['VAH', p().vah], ['POC', p().poc], ['VAL', p().val]] as const}>
                            {([label, value]) => (
                              <tr>
                                <td>{label}</td>
                                <td>{value === null ? <span class="warn">no session</span> : formatPrice(value, v().precision)}</td>
                              </tr>
                            )}
                          </For>
                        </tbody>
                      </table>
                    </Show>
                  </>
                )}
              </Show>
              <div class="note">Computed in this browser by the same Rust engine that draws the chart. Compare against TradingView with RSI 14, StochRSI 14/14/3/3 and the chart set to UTC.</div>
            </>
          )}
        </Show>
      </div>
    </div>
  );
}
