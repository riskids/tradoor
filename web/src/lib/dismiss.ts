import { createEffect, onCleanup, type Accessor } from 'solid-js';

/** Close a popup on the next pointerdown outside it, or on Escape. The popup
 *  stops propagation of its own pointerdowns. */
export function useDismiss(open: Accessor<unknown>, close: () => void): void {
  createEffect(() => {
    if (!open()) return;
    const onDown = () => close();
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      e.stopPropagation();
      close();
    };
    // Armed after the click that opened it has finished propagating.
    const timer = setTimeout(() => {
      addEventListener('pointerdown', onDown);
      addEventListener('keydown', onKey, true);
    });
    onCleanup(() => {
      clearTimeout(timer);
      removeEventListener('pointerdown', onDown);
      removeEventListener('keydown', onKey, true);
    });
  });
}
