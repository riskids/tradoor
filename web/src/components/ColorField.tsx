/** A colour swatch that opens a palette, a custom colour and an opacity slider.
 *  Kept apart from the drawing dialog so the chart's toolbar can use it without
 *  loading the dialog. */
import { createEffect, createSignal, For, onCleanup, Show } from 'solid-js';
import { Portal } from 'solid-js/web';

/** A palette in the spirit of TradingView's: greys, then hues in three strengths. */
export const PALETTE = [
  '#ffffff', '#d1d4dc', '#b2b5be', '#9598a1', '#787b86', '#5d606b', '#434651', '#2a2e39', '#131722', '#000000',
  '#f23645', '#ff9800', '#ffeb3b', '#4caf50', '#089981', '#00bcd4', '#2962ff', '#673ab7', '#9c27b0', '#e91e63',
  '#fccbcd', '#ffe0b2', '#fff9c4', '#c8e6c9', '#ace5dc', '#b2ebf2', '#bbd9fb', '#d1c4e9', '#e1bee7', '#f8bbd0',
  '#f7525f', '#ffb74d', '#fff176', '#81c784', '#42bda8', '#4dd0e1', '#5b9cf6', '#9575cd', '#ba68c8', '#f06292',
  '#b22833', '#f57c00', '#fbc02d', '#388e3c', '#056656', '#0097a7', '#1848cc', '#512da8', '#7b1fa2', '#c2185b',
];

/** `#rrggbb` and an opacity 0–100 out of any `#rgb`, `#rrggbb` or `#rrggbbaa`. */
export function splitColor(color: unknown, fallback = '#2962ff'): { hex: string; opacity: number } {
  let c = typeof color === 'string' && color.startsWith('#') ? color.slice(1) : fallback.slice(1);
  if (c.length === 3) c = [...c].map((ch) => ch + ch).join('');
  const hex = `#${c.slice(0, 6).padEnd(6, '0')}`;
  const opacity = c.length >= 8 ? Math.round((parseInt(c.slice(6, 8), 16) / 255) * 100) : 100;
  return { hex, opacity: Number.isFinite(opacity) ? opacity : 100 };
}

export function joinColor(hex: string, opacity: number): string {
  if (opacity >= 100) return hex;
  const a = Math.round((Math.max(0, opacity) / 100) * 255)
    .toString(16)
    .padStart(2, '0');
  return `${hex}${a}`;
}

/** Only one palette is open at a time: opening one closes the last. */
let closeOpenPalette: (() => void) | null = null;

/** A colour swatch that opens TradingView's palette, a custom colour, and opacity. */
export function ColorField(props: { value: string; onChange(v: string): void; title?: string }) {
  // Placed on the page's body, not in the dialog or the chart: a scroll box
  // would clip it, and a panel's container query would make `fixed` mean the panel.
  const [open, setOpenState] = createSignal<{ left: number; top: number } | null>(null);
  const close = () => setOpenState(null);
  let button!: HTMLButtonElement;
  const toggle = () => {
    let next: { left: number; top: number } | null = null;
    if (!open()) {
      const r = button.getBoundingClientRect();
      next = { left: Math.max(4, Math.min(r.left, innerWidth - 240)), top: r.bottom + 222 > innerHeight ? Math.max(4, r.top - 222) : r.bottom + 4 };
    }
    if (next && closeOpenPalette !== close) closeOpenPalette?.();
    closeOpenPalette = next ? close : null;
    setOpenState(next);
  };
  let palette: HTMLDivElement | undefined;
  // Closed by a press anywhere outside it, heard on the way down: a dialog the
  // field sits in keeps its own presses from the chart, and the palette stayed
  // open over the dialog's buttons. Escape closes it and hands the focus back
  // to the swatch, so a second Escape reaches the dialog.
  createEffect(() => {
    if (!open()) return;
    const onDown = (e: PointerEvent) => {
      const at = e.target as Node;
      if (!palette?.contains(at) && !button.contains(at)) close();
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
    if (closeOpenPalette === close) closeOpenPalette = null;
  });
  const parts = () => splitColor(props.value);
  return (
    <span class="dcolor">
      <button
        ref={button}
        class="swatchbtn"
        data-tip={props.title ?? 'Colour'}
        onPointerDown={(e) => e.stopPropagation()}
        onClick={toggle}
        style={{ '--c': props.value || 'transparent' }}
      />
      <Show when={open()}>
        <Portal>
          <div ref={palette} class="palette" style={{ left: `${open()!.left}px`, top: `${open()!.top}px` }} onPointerDown={(e) => e.stopPropagation()}>
            <div class="grid">
              <For each={PALETTE}>
                {(c) => <button classList={{ cur: parts().hex.toLowerCase() === c }} style={{ background: c }} data-tip={c} onClick={() => props.onChange(joinColor(c, parts().opacity))} />}
              </For>
            </div>
            <label class="custom">
              <input type="color" value={parts().hex} onInput={(e) => props.onChange(joinColor(e.currentTarget.value, parts().opacity))} />
              Custom
            </label>
            <label class="opacity">
              Opacity
              <input type="range" min="0" max="100" value={parts().opacity} onInput={(e) => props.onChange(joinColor(parts().hex, Number(e.currentTarget.value)))} />
              <span class="num">{parts().opacity}%</span>
            </label>
          </div>
        </Portal>
      </Show>
    </span>
  );
}

