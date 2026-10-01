/**
 * How an alert's line is drawn: its colour and opacity, thickness and dash,
 * as a drawing's are set. The chart shows each change as it is made; Ok keeps
 * it, and Cancel puts the line back as it was.
 */
import { createSignal, For, onMount } from 'solid-js';
import type { AlertStyle } from '../lib/api';
import { ColorField } from './ColorField';

/** The line every alert has until given another: the theme's alert green, dashed, a pixel wide. */
const DEFAULT = { color: '#22ab94', dash: 'dashed', width: 1 } as const;
const WIDTHS = [1, 1.5, 2, 3, 4];

/** `onApply` keeps the style and `onCancel` puts the line back; either closes it. */
export function AlertStyleSettings(props: { title: string; style: AlertStyle | undefined; onPreview(style: AlertStyle): void; onApply(style: AlertStyle): void; onCancel(): void }) {
  const [style, setStyle] = createSignal<AlertStyle>({ ...props.style });
  const set = (next: AlertStyle) => {
    setStyle(next);
    props.onPreview(next);
  };
  let panel!: HTMLDivElement;
  onMount(() => queueMicrotask(() => panel.focus()));

  const ok = () => props.onApply(style());

  return (
    <div
      ref={panel}
      class="dsettings al-style"
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
        <span class="title">Alert line</span>
        <span class="spacer" />
        <button class="x" data-tip="Cancel (esc)" onClick={() => props.onCancel()}>
          ✕
        </button>
      </header>
      <div class="dbody">
        <p class="dnote">{props.title}</p>
        <div class="drow">
          <span class="dlabel">Colour</span>
          <div class="dctl">
            <ColorField value={style().color ?? DEFAULT.color} title="Colour and opacity" onChange={(color) => set({ ...style(), color })} />
          </div>
        </div>
        <div class="drow">
          <span class="dlabel">Line</span>
          <div class="dctl">
            <select class="dinput narrow" data-tip="Thickness" value={String(style().width ?? DEFAULT.width)} onChange={(e) => set({ ...style(), width: Number(e.currentTarget.value) })}>
              <For each={WIDTHS}>{(w) => <option value={w}>{w}px</option>}</For>
            </select>
            <select class="dinput narrow" data-tip="Line style" value={style().dash ?? DEFAULT.dash} onChange={(e) => set({ ...style(), dash: e.currentTarget.value as AlertStyle['dash'] })}>
              <option value="solid">───</option>
              <option value="dashed">- - -</option>
              <option value="dotted">·····</option>
            </select>
          </div>
        </div>
      </div>
      <footer>
        <button class="dbtn" data-tip="The chart's own: green, dashed, a pixel" onClick={() => set({})}>
          Defaults
        </button>
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
