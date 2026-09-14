/** Page-wide UI state that is not part of the saved workspace. */
import { createSignal } from 'solid-js';

export type Overlay =
  | { kind: 'symbol'; panelId: string | null; initial: string }
  | { kind: 'indicators'; panelId: string }
  | { kind: 'palette' }
  | { kind: 'settings' }
  | { kind: 'help' }
  | { kind: 'note'; save(label: string): void };

export const [overlay, setOverlay] = createSignal<Overlay | null>(null);
export const closeOverlay = (): void => void setOverlay(null);

/** What the keyboard and the command palette can ask of a mounted chart. */
export interface ChartController {
  key(key: string): Promise<boolean>;
  resetView(): void;
  showRange(days: number | null): void;
}

export const chartControllers = new Map<string, ChartController>();

/** The panel whose tab is being dragged, while it is. */
export const [dragging, setDragging] = createSignal<string | null>(null);

/** A bare-letter shortcut must never fire while someone is typing. */
export function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.tagName === 'SELECT' || target.isContentEditable;
}
