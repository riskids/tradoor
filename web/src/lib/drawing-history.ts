/**
 * Undo and redo for one chart's drawings. A step names its drawing through a
 * shared ref rather than an id: a drawing brought back by undo is saved again
 * and gets a new id from the server, and every step about it must follow.
 */
import type { DrawingPoint } from '../engine/protocol';
import type { Drawing } from './api';
import type { Style } from './drawing-style';

export interface DrawingRef {
  id: number;
}

export interface DrawingChange {
  points?: DrawingPoint[];
  label?: string | null;
  style?: Style;
}

export type Step =
  | { kind: 'create'; ref: DrawingRef; drawing: Drawing }
  | { kind: 'delete'; ref: DrawingRef; drawing: Drawing }
  | { kind: 'change'; ref: DrawingRef; before: DrawingChange; after: DrawingChange }
  | { kind: 'clear'; items: { ref: DrawingRef; drawing: Drawing }[] };

const LIMIT = 100;

export class DrawingHistory {
  private done: Step[] = [];
  private undone: Step[] = [];
  private refs = new Map<number, DrawingRef>();

  /** `changed`: told whenever what can be undone or redone may have changed, for the buttons. */
  constructor(private changed: () => void = () => {}) {}

  /** The one ref for a drawing id, made on first use. */
  ref(id: number): DrawingRef {
    let r = this.refs.get(id);
    if (!r) {
      r = { id };
      this.refs.set(id, r);
    }
    return r;
  }

  /** A drawing now answers to another id: saved, or saved again. */
  rekey(ref: DrawingRef, id: number): void {
    if (this.refs.get(ref.id) === ref) this.refs.delete(ref.id);
    ref.id = id;
    this.refs.set(id, ref);
  }

  /** A new action: it can be undone, and whatever was undone can no longer be redone. */
  record(step: Step): void {
    this.done.push(step);
    if (this.done.length > LIMIT) this.done.shift();
    this.undone = [];
    this.changed();
  }

  /** Drop every step about a drawing that never made it to the server. */
  forget(ref: DrawingRef): void {
    const about = (s: Step) => (s.kind === 'clear' ? s.items.some((i) => i.ref === ref) : s.ref === ref);
    this.done = this.done.filter((s) => !about(s));
    this.undone = this.undone.filter((s) => !about(s));
    this.changed();
  }

  takeUndo(): Step | undefined {
    const step = this.done.pop();
    if (step) this.undone.push(step);
    this.changed();
    return step;
  }

  takeRedo(): Step | undefined {
    const step = this.undone.pop();
    if (step) this.done.push(step);
    this.changed();
    return step;
  }

  get canUndo(): boolean {
    return this.done.length > 0;
  }

  get canRedo(): boolean {
    return this.undone.length > 0;
  }

  reset(): void {
    this.done = [];
    this.undone = [];
    this.refs.clear();
    this.changed();
  }
}

/** What `change` will overwrite on `d`, so it can be put back. */
export function before(d: Drawing, change: DrawingChange): DrawingChange {
  const out: DrawingChange = {};
  if (change.points) out.points = d.points;
  if ('label' in change) out.label = d.label;
  if (change.style) out.style = d.style;
  return out;
}
