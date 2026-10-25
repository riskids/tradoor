/**
 * A study's own dialog, as TradingView gives every indicator: its numbers
 * under Inputs, each line's colour, width and visibility under Style. A
 * study's parameters are its identity, so Ok swaps it for one with the new
 * ones when they changed; a line's style is kept live as it is set.
 */
import { createResource, createSignal, For, onMount, Show } from 'solid-js';
import { studyCatalog } from '../engine/client';
import type { StudyDefinition, StudyMeta } from '../engine/protocol';
import { errorText } from '../lib/api';
import { ColorField } from './ColorField';

const WIDTHS = [1, 2, 3, 4];

/** `studyStyles` keyed by `{studyId}/{lineKey}`, valued `{color},{width},{hidden}`. */
function styleOf(styles: Record<string, string>, key: string): { color: string; width: number; hidden: boolean } {
  const [color, width, hidden] = (styles[key] ?? '').split(',');
  return { color: color || '', width: Number(width) || 1, hidden: hidden === '1' };
}

function withStyle(styles: Record<string, string>, key: string, next: { color: string; width: number; hidden: boolean }): Record<string, string> {
  return { ...styles, [key]: `${next.color},${next.width},${next.hidden ? 1 : 0}` };
}

export function StudySettings(props: {
  study: StudyMeta;
  styles: Record<string, string>;
  onPreview(styles: Record<string, string>): void;
  onApply(newId: string, styles: Record<string, string>): void;
  onCancel(): void;
}) {
  const kind = () => props.study.id.split(':')[0]!;
  const [defs] = createResource(() => studyCatalog());
  const def = (): StudyDefinition | undefined => (defs.state === 'ready' ? defs()!.find((d) => d.kind === kind()) : undefined);
  const [values, setValues] = createSignal<number[]>(props.study.id.split(':').slice(1).map(Number));
  const [tab, setTab] = createSignal<'inputs' | 'style'>('inputs');
  const [styles, setStyles] = createSignal(props.styles);

  const clamp = (d: StudyDefinition, i: number): number => Math.round(Math.min(d.params[i]!.max, Math.max(d.params[i]!.min, values()[i] ?? d.params[i]!.default)));
  const newId = (): string => {
    const d = def();
    return d ? [d.kind, ...d.params.map((_, i) => clamp(d, i))].join(':') : props.study.id;
  };

  const setStyle = (lineId: string, patch: Partial<{ color: string; width: number; hidden: boolean }>) => {
    const key = `${props.study.id}/${lineId.split(':').pop()}`;
    const next = withStyle(styles(), key, { ...styleOf(styles(), key), ...patch });
    setStyles(next);
    props.onPreview(next);
  };

  let panel!: HTMLDivElement;
  onMount(() => queueMicrotask(() => panel.focus()));

  const ok = () => props.onApply(newId(), styles());

  return (
    <div
      ref={panel}
      class="dsettings study"
      tabIndex={-1}
      onPointerDown={(e) => e.stopPropagation()}
      onWheel={(e) => e.stopPropagation()}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === 'Escape') props.onCancel();
        else if (e.key === 'Enter') ok();
      }}
    >
      <header>
        <span class="title">{props.study.label}</span>
        <span class="spacer" />
        <button class="x" data-tip="Cancel (esc)" onClick={() => props.onCancel()}>
          ✕
        </button>
      </header>
      <nav class="dtabs">
        <button classList={{ on: tab() === 'inputs' }} onClick={() => setTab('inputs')}>
          Inputs
        </button>
        <button classList={{ on: tab() === 'style' }} onClick={() => setTab('style')}>
          Style
        </button>
      </nav>
      <div class="dbody">
        <Show when={tab() === 'inputs'}>
          <Show when={def()} fallback={<p class="dnote">{defs.loading ? 'Loading…' : defs.state === 'errored' ? errorText(defs.error) : 'This study has no inputs.'}</p>}>
            {(d) => (
              <For each={d().params} fallback={<p class="dnote">This study has no inputs.</p>}>
                {(p, i) => (
                  <div class="drow">
                    <label class="dlabel" for={`study-input-${i()}`}>
                      {p.name}
                    </label>
                    <div class="dctl">
                      <input
                        id={`study-input-${i()}`}
                        class="dinput num"
                        type="number"
                        min={p.min}
                        max={p.max}
                        value={clamp(d(), i())}
                        onChange={(e) => {
                          const next = [...values()];
                          next[i()] = Number(e.currentTarget.value);
                          setValues(next);
                        }}
                      />
                    </div>
                  </div>
                )}
              </For>
            )}
          </Show>
        </Show>
        <Show when={tab() === 'style'}>
          <For each={props.study.lines} fallback={<p class="dnote">This study draws no line of its own.</p>}>
            {(line) => {
              const lineKey = line.id.split(':').pop()!;
              const key = `${props.study.id}/${lineKey}`;
              const s = () => styleOf(styles(), key);
              return (
                <div class="drow">
                  <input type="checkbox" aria-label={`Show ${line.label}`} checked={!s().hidden} onChange={(e) => setStyle(line.id, { hidden: !e.currentTarget.checked })} />
                  <span class="dlabel">{line.label}</span>
                  <div class="dctl">
                    <ColorField value={s().color || line.color} title={`${line.label} colour`} onChange={(color) => setStyle(line.id, { color })} />
                    <select class="dinput narrow" aria-label={`${line.label} width`} value={String(s().width)} onChange={(e) => setStyle(line.id, { width: Number(e.currentTarget.value) })}>
                      <For each={WIDTHS}>{(w) => <option value={w}>{w}px</option>}</For>
                    </select>
                  </div>
                </div>
              );
            }}
          </For>
        </Show>
      </div>
      <footer>
        <span class="spacer" />
        <button class="dbtn" onClick={() => props.onCancel()}>
          Cancel
        </button>
        <button class="dbtn primary" onClick={ok}>
          Ok
        </button>
      </footer>
    </div>
  );
}
