/** The strip over the chart while a drawing is selected: its colour and width
 *  at hand, its settings, and delete. */
import { For, Show } from 'solid-js';
import type { Drawing } from '../lib/api';
import { KIND_NAMES } from '../lib/drawing-options';
import { defaultStyle, type Style } from '../lib/drawing-style';
import { Icon } from '../lib/icons';
import { ColorField } from './ColorField';

export function DrawingToolbar(props: { drawing: Drawing; onRestyle(patch: Style): void; onSettings(): void; onDelete(): void }) {
  const d = () => props.drawing;
  return (
    <div class="dtoolbar" onPointerDown={(e) => e.stopPropagation()} onDblClick={(e) => e.stopPropagation()} onContextMenu={(e) => e.stopPropagation()}>
      <span class="name">{KIND_NAMES[d().kind] ?? d().kind}</span>
      <Show when={!['long', 'short', 'text', 'measure'].includes(d().kind)}>
        <ColorField value={String(d().style.color ?? defaultStyle(d().kind).color ?? '#2962ff')} onChange={(v) => props.onRestyle({ color: v })} title="Line colour" />
        <select class="dinput narrow" data-tip="Thickness" value={String(d().style.width ?? defaultStyle(d().kind).width ?? 1.5)} onChange={(e) => props.onRestyle({ width: Number(e.currentTarget.value) })}>
          <For each={[1, 1.5, 2, 3, 4]}>{(w) => <option value={w}>{w}px</option>}</For>
        </select>
      </Show>
      <button data-tip="Settings (double-click the drawing)" disabled={d().id < 0} onClick={() => props.onSettings()}>
        <Icon name="settings" size={13} />
      </button>
      <button class="del" data-tip="Delete (Delete)" onClick={() => props.onDelete()}>
        <Icon name="trash" size={13} />
      </button>
    </div>
  );
}
