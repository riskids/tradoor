/** Short notices in the corner of the page: an alert that fired, an error. */
import { createSignal } from 'solid-js';

export interface Toast {
  id: number;
  title: string;
  body?: string;
  tone: 'alert' | 'info' | 'warn';
  /** Clicking the notice does this, then dismisses it. */
  action?: () => void;
}

const [toasts, setToasts] = createSignal<Toast[]>([]);
export { toasts };

let next = 1;

export function toast(t: Omit<Toast, 'id'>, ms = 12_000): void {
  const id = next++;
  // The five newest are enough; older ones would only cover the chart.
  setToasts((list) => [...list, { ...t, id }].slice(-5));
  setTimeout(() => dismiss(id), ms);
}

export function dismiss(id: number): void {
  setToasts((list) => list.filter((t) => t.id !== id));
}
