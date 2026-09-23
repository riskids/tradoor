/** Studies the engine computes, with their parameters editable before adding.
 *  A study's identity is its parameters, so a different length is a different study. */
import { createMemo, createResource, createSignal, For, onMount, Show } from 'solid-js';
import { createStore } from 'solid-js/store';
import { studyCatalog } from '../engine/client';
import type { StudyDefinition } from '../engine/protocol';
import { errorText } from '../lib/api';
import { Icon } from '../lib/icons';

export function IndicatorPicker(props: { active: string[]; onAdd(id: string): void; onClose(): void }) {
  const [query, setQuery] = createSignal('');
  const [params, setParams] = createStore<Record<string, number[]>>({});
  const [defs] = createResource(() => studyCatalog());
  let input!: HTMLInputElement;
  onMount(() => input.focus());

  const matches = createMemo(() => {
    const all = defs.state === 'ready' ? defs()! : [];
    const q = query().trim().toLowerCase();
    if (!q) return all;
    return all.filter((d) => d.name.toLowerCase().includes(q) || d.short.toLowerCase().includes(q) || d.kind.includes(q));
  });

  const values = (d: StudyDefinition): number[] => d.params.map((p, i) => Math.round(Math.min(p.max, Math.max(p.min, params[d.kind]?.[i] ?? p.default))));
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
                    {/* Open interest is set in its own dialog, from the legend, once added. */}
                    <For each={d.kind === 'oi' ? [] : d.params}>
                      {(p, i) => (
                        <label class="field" title={p.name}>
                          {p.name}
                          <input
                            class="box num"
                            type="number"
                            min={p.min}
                            max={p.max}
                            value={values(d)[i()]}
                            onInput={(e) => {
                              const next = [...values(d)];
                              next[i()] = Number(e.currentTarget.value);
                              setParams(d.kind, next);
                            }}
                          />
                        </label>
                      )}
                    </For>
                    <button class="btn" disabled={added()} title={added() ? 'Already on the chart' : `Add ${idOf(d)}`} onClick={() => props.onAdd(idOf(d))}>
                      <Icon name="plus" size={11} />
                      {added() ? 'Added' : 'Add'}
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
