/**
 * A link that opens a chart, as Telegram's messages carry them: `?signal=<id>`
 * opens that pattern drawn, at its own length; `?market=…&tf=…` a market, and
 * with `&touched=<ms>` the touch of a line there, drawn. Read once the layout
 * is in, then taken off the address so a reload does not open it again.
 */
import { batch } from 'solid-js';
import { api } from './api';
import { openPattern, signalPattern, touchPattern } from './patterns';
import { isChartMarket } from './markets';
import { setPanelMarket, setPanelTimeframe, targetChart } from '../workspace/store';

export async function followLink(): Promise<void> {
  const url = new URL(location.href);
  const id = Number(url.searchParams.get('signal'));
  const market = url.searchParams.get('market');
  const tf = url.searchParams.get('tf');
  const touched = Number(url.searchParams.get('touched'));
  for (const key of ['signal', 'market', 'tf', 'touched']) url.searchParams.delete(key);
  history.replaceState(history.state, '', url.pathname + url.search + url.hash);
  const target = targetChart();
  if (!target) return;
  if (id || touched) {
    try {
      const state = await api.signals();
      const pattern = id ? [...state.signals, ...state.ended].find((s) => s.id === id) : undefined;
      if (pattern) return openPattern(signalPattern(pattern), target, true);
      const touch = touched ? [...state.touches, ...state.endedTouches].find((t) => t.market === market && t.touchedAt === touched) : undefined;
      if (touch) return openPattern(touchPattern(touch), target, true);
    } catch {
      // Gone from the list, or the server not answering: the market is still opened below.
    }
  }
  if (market && isChartMarket(market)) {
    batch(() => {
      setPanelMarket(target.id, market);
      if (tf) setPanelTimeframe(target.id, tf);
    });
  }
}
