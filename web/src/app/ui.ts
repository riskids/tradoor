/** Page-wide UI state that is not part of the saved workspace. */
import { createSignal } from 'solid-js';
import type { MagnetMode } from '../engine/protocol';
import { readPref, writePref } from '../lib/prefs';

export type Overlay =
  | { kind: 'symbol'; panelId: string | null; initial: string; /** Where in a watchlist to add it. */ at?: number }
  | { kind: 'indicators'; panelId: string }
  | { kind: 'tpoSettings'; panelId: string }
  | { kind: 'palette' }
  | { kind: 'settings' }
  | { kind: 'help' }
  | { kind: 'note'; initial?: string; save(label: string): void };

export const [overlay, setOverlay] = createSignal<Overlay | null>(null);
export const closeOverlay = (): void => void setOverlay(null);

/** What the keyboard and the command palette can ask of a mounted chart. */
export interface ChartController {
  key(key: string): Promise<boolean>;
  /** Arm a drawing tool, or disarm it when it is the one armed. */
  tool(kind: string): void;
  resetView(): void;
  showRange(days: number | null): void;
  /** Undo or redo the last drawing change; false when there was none. */
  undo(): boolean;
  redo(): boolean;
  /** Copy a picture of the chart, or save the file instead. */
  capture(save: boolean): void;
}

export const chartControllers = new Map<string, ChartController>();

/** Alt + the key arms a drawing tool on the chart in use, as on TradingView.
 *  By `KeyboardEvent.code`: on a Mac, Alt changes the letter `key` reports. */
export const TOOL_KEYS: Record<string, string> = {
  KeyT: 'trendline',
  KeyH: 'horizontal',
  KeyJ: 'ray',
  KeyR: 'rect',
  KeyC: 'channel',
  KeyF: 'fib',
  KeyN: 'text',
  KeyM: 'measure',
  KeyL: 'long',
  KeyS: 'short',
  KeyB: 'brush',
  KeyP: 'path',
};
/** Alt + this key turns the magnet through off, weak and strong. */
export const MAGNET_KEY = 'KeyG';
/** Alt + this key copies a picture of the chart; with shift, saves the file. */
export const CAPTURE_KEY = 'KeyK';

/** The drawing magnet, one setting for every chart, remembered by this browser. */
const MAGNETS: readonly MagnetMode[] = ['off', 'weak', 'strong'];
const storedMagnet = readPref('magnet');
export const [magnet, setMagnetSignal] = createSignal<MagnetMode>(MAGNETS.includes(storedMagnet as MagnetMode) ? (storedMagnet as MagnetMode) : 'off');
export function setMagnet(mode: MagnetMode): void {
  setMagnetSignal(mode);
  writePref('magnet', mode);
}
/** Off, weak, strong, off again. */
export const cycleMagnet = (): void => setMagnet(MAGNETS[(MAGNETS.indexOf(magnet()) + 1) % MAGNETS.length]!);

/** The panel whose tab is being dragged, while it is. */
export const [dragging, setDragging] = createSignal<string | null>(null);

/** A bare-letter shortcut must never fire while someone is typing. */
export function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.tagName === 'SELECT' || target.isContentEditable;
}
