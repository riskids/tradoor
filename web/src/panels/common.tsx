import { For } from 'solid-js';
import { setOverlay } from '../app/ui';
import { Icon } from '../lib/icons';
import { VenueTag } from '../lib/exchanges';
import { symbolOf } from '../lib/markets';
import { TIMEFRAMES } from '../lib/timeframes';

/** The ticker, which opens symbol search, and its venue — left out where a panel spans every venue. */
export function MarketButton(props: { panelId: string; market: string; venue?: boolean }) {
  return (
    <>
      <button class="symbol" title="Change symbol — or just start typing" onClick={() => setOverlay({ kind: 'symbol', panelId: props.panelId, initial: '' })}>
        {symbolOf(props.market)}
        <Icon name="chevron" size={10} />
      </button>
      {props.venue === false ? null : <VenueTag market={props.market} name />}
    </>
  );
}

export function TimeframeSelect(props: { value: string; onChange(tf: string): void; allow?: (tf: string) => boolean }) {
  return (
    <select title="Timeframe" onChange={(e) => props.onChange(e.currentTarget.value)}>
      <For each={TIMEFRAMES.filter((t) => props.allow?.(t.value) ?? true)}>
        {(t) => (
          <option value={t.value} selected={t.value === props.value}>
            {t.label}
          </option>
        )}
      </For>
    </select>
  );
}

export const SESSION_NAME: Record<string, string> = { '1d': 'daily', '1w': 'weekly', '1M': 'monthly' };
