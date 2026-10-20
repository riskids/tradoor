/**
 * The strategy a trade was taken on, picked from cards that sketch it — the
 * cards of rsalianto's journal (components/apps/trading-journal/strategies.tsx),
 * and Three Drives, the screener's own — or written in under Other.
 */
import { createSignal, For, type JSX, Show } from 'solid-js';
import { Icon } from '../lib/icons';

const Sketch = (props: { children: JSX.Element }) => (
  <svg viewBox="0 0 60 36" fill="none" stroke="currentColor" stroke-linecap="round" stroke-linejoin="round">
    {props.children}
  </svg>
);
const Dot = (props: { x: number; y: number; r?: number; o?: number }) => <circle cx={props.x} cy={props.y} r={props.r ?? 2} fill="currentColor" stroke-width="0" opacity={props.o} />;
const Guide = (props: { d: string; o?: number; w?: number; dash?: string }) => (
  <path d={props.d} stroke-width={props.w ?? 1} stroke-dasharray={props.dash ?? '3 2'} opacity={props.o ?? 0.45} />
);
const Line = (props: { points: string }) => <polyline points={props.points} stroke-width="1.5" />;

export const STRATEGY_CARDS: { id: string; label: string; sketch: () => JSX.Element }[] = [
  {
    id: 'Breakout',
    label: 'Breakout',
    sketch: () => (
      <Sketch>
        <Guide d="M2 22H58" />
        <Line points="2,30 10,28 18,30 24,28 30,30 36,22 44,13 52,7 58,5" />
        <Dot x={36} y={22} />
      </Sketch>
    ),
  },
  {
    id: 'Trend Following',
    label: 'Trend',
    sketch: () => (
      <Sketch>
        <Guide d="M2,32 Q20,24 36,16 Q50,9 58,5" />
        <Line points="2,32 8,28 12,30 20,22 24,24 30,16 34,18 42,10 46,12 54,5 58,5" />
      </Sketch>
    ),
  },
  {
    id: 'Mean Reversion',
    label: 'Mean Rev',
    sketch: () => (
      <Sketch>
        <Guide d="M2 18H58" />
        <Line points="2,18 10,10 16,6 22,10 30,18 36,26 42,30 48,26 54,18 58,18" />
        <Dot x={16} y={6} />
        <Dot x={42} y={30} />
      </Sketch>
    ),
  },
  {
    id: 'Support/Resistance',
    label: 'S / R',
    sketch: () => (
      <Sketch>
        <Guide d="M2 26H58" />
        <Line points="2,14 8,20 12,26 18,16 24,26 30,12 36,26 42,16 48,26 54,14 58,12" />
        <For each={[12, 24, 36, 48]}>{(x) => <Dot x={x} y={26} r={1.5} />}</For>
      </Sketch>
    ),
  },
  {
    id: 'EMA Crossover',
    label: 'EMA Cross',
    sketch: () => (
      <Sketch>
        <Guide d="M2,8 C18,8 42,28 58,28" w={1.5} dash="4 2" o={0.6} />
        <path d="M2,28 C18,28 42,8 58,8" stroke-width="1.5" />
        <Dot x={30} y={18} r={2.5} />
      </Sketch>
    ),
  },
  {
    id: 'RSI Divergence',
    label: 'Divergence',
    sketch: () => (
      <Sketch>
        <Line points="2,8 14,18 26,6 38,20 50,12 58,24" />
        <Guide d="M2,28 14,26 26,28 38,24 50,26 58,20" o={0.6} />
        <Guide d="M2 8L50 12" w={0.8} dash="2 3" o={0.3} />
        <Guide d="M2 28L50 26" w={0.8} dash="2 3" o={0.3} />
      </Sketch>
    ),
  },
  {
    id: 'VWAP Bounce',
    label: 'VWAP',
    sketch: () => (
      <Sketch>
        <Guide d="M2 22L58 18" dash="4 2" o={0.5} />
        <Line points="2,12 10,14 18,18 24,22 28,26 34,22 40,16 46,10 54,7 58,5" />
        <Dot x={28} y={26} />
      </Sketch>
    ),
  },
  {
    id: 'News Play',
    label: 'News Play',
    sketch: () => (
      <Sketch>
        <path d="M2 26H20" stroke-width="1.5" />
        <Guide d="M22 26V10" dash="2 2" o={0.5} />
        <Line points="22,10 30,11 38,12 46,14 56,14" />
        <Dot x={21} y={18} r={3} o={0.35} />
        <path d="M21 14V22" stroke-width="1" opacity="0.7" />
      </Sketch>
    ),
  },
  {
    id: 'Scalp',
    label: 'Scalp',
    sketch: () => (
      <Sketch>
        <Line points="2,20 5,13 9,20 13,13 17,20 21,13 25,20 29,13 33,20 37,13 41,20 45,13 49,20 53,13 57,20" />
        <Guide d="M2 13H58" w={0.8} dash="2 3" o={0.3} />
        <Guide d="M2 20H58" w={0.8} dash="2 3" o={0.3} />
      </Sketch>
    ),
  },
  {
    id: 'Swing',
    label: 'Swing',
    sketch: () => (
      <Sketch>
        <Line points="2,30 8,28 14,22 20,14 26,8 32,10 38,18 44,24 50,28 56,30" />
        <Dot x={2} y={30} o={0.7} />
        <Dot x={26} y={8} o={0.7} />
        <Guide d="M2 30H26" w={0.8} dash="2 3" o={0.25} />
        <Guide d="M26 8V30" w={0.8} dash="2 3" o={0.25} />
      </Sketch>
    ),
  },
  {
    id: 'Three Drives',
    label: 'Three Drives',
    sketch: () => (
      <Sketch>
        <Line points="2,32 12,22 17,27 28,15 33,20 44,7 58,26" />
        <Guide d="M12 22L44 7" w={0.8} dash="2 3" o={0.4} />
        <Dot x={12} y={22} r={1.6} />
        <Dot x={28} y={15} r={1.6} />
        <Dot x={44} y={7} r={1.8} />
      </Sketch>
    ),
  },
];

const KNOWN = new Set(STRATEGY_CARDS.map((c) => c.id));

/** The cards, and Other for a strategy of one's own words. */
export function StrategyPicker(props: { value: string; onChange(v: string): void }) {
  const [other, setOther] = createSignal(props.value !== '' && !KNOWN.has(props.value));
  return (
    <div class="strategy">
      <div class="strategy-head">
        <span class="strategy-title">Strategy</span>
        <Show
          when={other()}
          fallback={
            <button class="strategy-other" onClick={() => (setOther(true), props.onChange(''))}>
              <Icon name="pencil" size={11} /> Other
            </button>
          }
        >
          <input class="dinput strategy-own" placeholder="Describe your strategy…" maxLength={120} value={props.value} onInput={(e) => props.onChange(e.currentTarget.value)} ref={(el) => queueMicrotask(() => el.focus())} />
          <button class="strategy-other" aria-label="Back to the cards" onClick={() => (setOther(false), props.onChange(''))}>
            <Icon name="close" size={11} />
          </button>
        </Show>
      </div>
      <div class="strategy-cards">
        <For each={STRATEGY_CARDS}>
          {(c) => (
            <button
              classList={{ 'strategy-card': true, on: !other() && props.value === c.id }}
              onClick={() => {
                setOther(false);
                props.onChange(props.value === c.id ? '' : c.id);
              }}
            >
              {c.sketch()}
              <span>{c.label}</span>
            </button>
          )}
        </For>
      </div>
    </div>
  );
}
