/**
 * The chart's own colours, as TradingView lays its "Chart settings" dialog
 * out: a tab down the left for each group, the swatches for it on the right.
 * Every change applies at once; Defaults puts the open tab back to its own.
 */
import { createSignal, For, Show } from 'solid-js';
import { CHART_THEME, FONT_PX } from '../lib/theme';
import { doc, setThemeValue } from '../workspace/store';
import { ColorPicker } from './ColorPicker';

const TABS: { id: 'candles' | 'canvas' | 'scales' | 'studies' | 'drawings'; label: string }[] = [
  { id: 'candles', label: 'Symbol' },
  { id: 'canvas', label: 'Canvas' },
  { id: 'scales', label: 'Scales' },
  { id: 'studies', label: 'Studies' },
  { id: 'drawings', label: 'Drawings' },
];

const byName = (name: string) => CHART_THEME.find((t) => t.name === name)!;

/** A theme colour, through `ColorPicker`: swatch, name, then ↺ once it differs from the default. */
function ThemeSwatch(props: { name: string; label: string; fallback: string }) {
  const value = () => doc.theme[props.name] ?? props.fallback;
  return (
    <span class="swatch">
      <ColorPicker value={value()} fallback={props.fallback} title={props.label} onChange={(v) => setThemeValue(props.name, v)} />
      <span class="name">{props.label}</span>
      <Show when={doc.theme[props.name]}>
        <button class="dim" data-tip="Back to the default" onClick={() => setThemeValue(props.name, null)}>
          ↺
        </button>
      </Show>
    </span>
  );
}

/** One row: a label, then one or two colour pickers beside it. */
function Pair(props: { label: string; names: string[] }) {
  return (
    <div class="set-row">
      <span class="label">{props.label}</span>
      <div class="grow swatches tight">
        <For each={props.names}>{(name) => <ThemeSwatch name={name} label={byName(name).label} fallback={byName(name).value} />}</For>
      </div>
    </div>
  );
}

const [tab, setTab] = createSignal<(typeof TABS)[number]['id']>('candles');

export function ChartSettings(props: { onClose(): void }) {
  const resetTab = () => {
    const names = CHART_THEME.filter((t) => t.group === tab()).map((t) => t.name);
    if (tab() === 'scales') names.push(FONT_PX.name);
    for (const name of names) setThemeValue(name, null);
  };

  return (
    <div class="backdrop" onPointerDown={() => props.onClose()}>
      <div
        class="dialog chart-settings"
        tabIndex={-1}
        ref={(el) => queueMicrotask(() => el.focus())}
        onPointerDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key !== 'Escape') return;
          e.stopPropagation();
          props.onClose();
        }}
      >
        <header>
          <span class="title caps">Chart settings</span>
          <span class="spacer" />
          <button class="x" onClick={() => props.onClose()}>
            esc ✕
          </button>
        </header>
        <div class="set">
          <nav class="set-nav">
            <For each={TABS}>
              {(t) => (
                <button classList={{ on: tab() === t.id }} onClick={() => setTab(t.id)}>
                  <span>{t.label}</span>
                </button>
              )}
            </For>
          </nav>
          <div class="set-body">
            <Show when={tab() === 'candles'}>
              <h3>Candles</h3>
              <Pair label="Body" names={['bull', 'bear']} />
              <Pair label="Borders" names={['borderBull', 'borderBear']} />
              <Pair label="Wick" names={['wickBull', 'wickBear']} />
              <h3>Volume</h3>
              <Pair label="Volume" names={['volumeBull', 'volumeBear']} />
            </Show>
            <Show when={tab() === 'canvas'}>
              <For each={CHART_THEME.filter((t) => t.group === 'canvas')}>{(t) => <Pair label={t.label} names={[t.name]} />}</For>
            </Show>
            <Show when={tab() === 'scales'}>
              <For each={CHART_THEME.filter((t) => t.group === 'scales')}>{(t) => <Pair label={t.label} names={[t.name]} />}</For>
              <div class="set-row">
                <span class="label">Font size</span>
                <input
                  class="box num"
                  type="number"
                  min={8}
                  max={20}
                  aria-label="Axis font size, in pixels"
                  value={doc.theme[FONT_PX.name] ?? FONT_PX.value}
                  onChange={(e) => {
                    const px = Number(e.currentTarget.value);
                    setThemeValue(FONT_PX.name, px >= 8 && px <= 20 ? String(px) : null);
                  }}
                />
              </div>
            </Show>
            <Show when={tab() === 'studies'}>
              <For each={CHART_THEME.filter((t) => t.group === 'studies')}>{(t) => <Pair label={t.label} names={[t.name]} />}</For>
            </Show>
            <Show when={tab() === 'drawings'}>
              <For each={CHART_THEME.filter((t) => t.group === 'drawings')}>{(t) => <Pair label={t.label} names={[t.name]} />}</For>
            </Show>
          </div>
        </div>
        <footer>
          <button class="dbtn" onClick={resetTab}>
            Defaults
          </button>
          <span class="spacer" />
          <button class="dbtn primary" onClick={() => props.onClose()}>
            Ok
          </button>
        </footer>
      </div>
    </div>
  );
}
