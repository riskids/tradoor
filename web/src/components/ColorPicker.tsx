/**
 * TradingView's small colour picker: sixteen presets, a native custom colour,
 * and an opacity slider — opened from a button showing the colour picked.
 * Used for theme and study colours, which take alpha (`#rrggbbaa`) as opacity.
 */
import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js';
import { Portal } from 'solid-js/web';
import { joinColor, splitColor } from './ColorField';

const PRESETS = [
  '#ffffff', '#b2b5be', '#787b86', '#2196f3', '#2962ff', '#26a69a', '#4caf50', '#ef5350',
  '#f23645', '#ff9800', '#ffeb3b', '#e040fb', '#9c27b0', '#00bcd4', '#131722', '#000000',
];

/** Only one of these — or a `ColorField`'s palette — is open at a time. */
let closeOpenPicker: (() => void) | null = null;

export function ColorPicker(props: { value: string; fallback?: string; onChange(v: string): void; title?: string }) {
  const [open, setOpenState] = createSignal<{ left: number; top: number } | null>(null);
  const close = () => setOpenState(null);
  let button!: HTMLButtonElement;
  const toggle = () => {
    let next: { left: number; top: number } | null = null;
    if (!open()) {
      const r = button.getBoundingClientRect();
      next = { left: Math.max(4, Math.min(r.left, innerWidth - 216)), top: r.bottom + 150 > innerHeight ? Math.max(4, r.top - 150) : r.bottom + 4 };
    }
    if (next && closeOpenPicker !== close) closeOpenPicker?.();
    closeOpenPicker = next ? close : null;
    setOpenState(next);
  };
  let pop: HTMLDivElement | undefined;
  // Closed by a press outside it or Escape, as ColorField's palette is.
  createEffect(() => {
    if (!open()) return;
    const onDown = (e: PointerEvent) => {
      const at = e.target as Node;
      if (!pop?.contains(at) && !button.contains(at)) close();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.stopPropagation();
      close();
      button.focus();
    };
    addEventListener('pointerdown', onDown, true);
    addEventListener('keydown', onKey, true);
    onCleanup(() => {
      removeEventListener('pointerdown', onDown, true);
      removeEventListener('keydown', onKey, true);
    });
  });
  onCleanup(() => {
    if (closeOpenPicker === close) closeOpenPicker = null;
  });
  const parts = () => splitColor(props.value, props.fallback);
  return (
    <span class="dcolor">
      <button
        ref={button}
        type="button"
        class="swatchbtn"
        aria-label={props.title ?? 'Colour'}
        aria-expanded={open() !== null}
        data-tip={props.title ?? 'Colour'}
        onPointerDown={(e) => e.stopPropagation()}
        onClick={toggle}
        style={{ '--c': props.value || props.fallback || 'transparent' }}
      />
      <Show when={open()}>
        <Portal>
          <div ref={pop} class="palette small" role="dialog" aria-label={props.title ?? 'Colour'} style={{ left: `${open()!.left}px`, top: `${open()!.top}px` }} onPointerDown={(e) => e.stopPropagation()}>
            <div class="grid">
              <For each={PRESETS}>
                {(c) => (
                  <button type="button" classList={{ cur: parts().hex.toLowerCase() === c }} style={{ background: c }} data-tip={c} aria-label={c} onClick={() => props.onChange(joinColor(c, parts().opacity))} />
                )}
              </For>
            </div>
            <label class="custom">
              <input type="color" value={parts().hex} aria-label="Custom colour" onInput={(e) => props.onChange(joinColor(e.currentTarget.value, parts().opacity))} />
              Custom
            </label>
            <label class="opacity">
              Opacity
              <input type="range" min="0" max="100" value={parts().opacity} aria-label="Opacity" onInput={(e) => props.onChange(joinColor(parts().hex, Number(e.currentTarget.value)))} />
              <span class="num">{parts().opacity}%</span>
            </label>
          </div>
        </Portal>
      </Show>
    </span>
  );
}
