/** Studies the engine computes, added with their defaults; inputs are set from
 *  the legend's gear afterwards. A study's identity is its parameters. */
import { createMemo, createResource, createSignal, For, onMount, Show } from 'solid-js';
import { studyCatalog } from '../engine/client';
import type { StudyDefinition } from '../engine/protocol';
import { errorText } from '../lib/api';
import { Icon } from '../lib/icons';

export function IndicatorPicker(props: { active: string[]; onAdd(id: string): void; onClose(): void }) {
  const [query, setQuery] = createSignal('');
  const [defs] = createResource(() => studyCatalog());
  let input!: HTMLInputElement;
  onMount(() => input.focus());

  const matches = createMemo(() => {
    const all = defs.state === 'ready' ? defs()! : [];
    const q = query().trim().toLowerCase();
    if (!q) return all;
    return all.filter((d) => d.name.toLowerCase().includes(q) || d.short.toLowerCase().includes(q) || d.kind.includes(q));
  });

  const values = (d: StudyDefinition): number[] => d.params.map((p) => p.default);
  const idOf = (d: StudyDefinition): string => [d.kind, ...values(d)].join(':');

  return (
    <div class="backdrop" onPointerDown={() => props.onClose()}>
      <div
        class="dialog medium"
        onPointerDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key !== 'Escape') return;
          e.stopPropagation();
          props.onClose();
        }}
      >
        <header>
          <span class="title caps">Indicators</span>
          <Icon name="search" size={12} />
          <input ref={input} class="q" value={query()} onInput={(e) => setQuery(e.currentTarget.value)} placeholder="Search indicators" />
          <button class="x" onClick={() => props.onClose()}>
            esc ✕
          </button>
        </header>
        <div class="list">
          <Show when={defs.state !== 'errored'} fallback={<div class="empty warn">▲ {errorText(defs.error)}</div>}>
            <For each={matches()} fallback={<div class="empty">{defs.loading ? 'Loading…' : `Nothing matches “${query()}”.`}</div>}>
              {(d) => {
                const added = () => props.active.includes(idOf(d));
                return (
                  <div class="study">
                    <span>{d.name}</span>
                    <span class="dim">{d.short}</span>
                    <span class="dim">{d.group === 'price' ? 'on price' : 'own pane'}</span>
                    <span class="spacer" />
                    {/* Inputs are set from the legend's gear once added, as on TradingView. */}
                    <button class="btn" disabled={added()} data-tip={added() ? 'Already on the chart' : `Add ${idOf(d)}`} onClick={() => props.onAdd(idOf(d))}>
                      <Icon name="plus" size={11} />
                      <span>{added() ? 'Added' : 'Add'}</span>
                    </button>
                    <span class="about">{d.about}</span>
                  </div>
                );
              }}
            </For>
          </Show>
        </div>
        <footer>
          <span>Computed by the engine that draws the chart. Remove one from the chart's legend.</span>
        </footer>
      </div>
    </div>
  );
}
