/**
 * Which exchange addresses this browser can reach from the network it is on.
 *
 * Some networks block the exchanges' names — a phone on an Indonesian carrier
 * most of all — and not always by turning a call away: a socket or a request
 * can hang until the phone gives up on it a minute or two later. Whatever
 * talks to an exchange asks here first, so what one part of the page learned
 * the slow way the rest go straight past, to the tradoor server's relay. The
 * page and the chart engine's worker tell each other over a BroadcastChannel,
 * and the page keeps the list past a reload, which on a phone is often.
 *
 * An entry lasts five minutes, so moving from that carrier to a wifi tries the
 * exchanges afresh.
 */
const BLOCKED_MS = 5 * 60_000;
const STORE = 'tradoor:unreachable';

type Note = { blocked: [string, number][] } | { reached: string } | { ask: true } | { wake: true };

const blocked = new Map<string, number>();
const wakers = new Set<() => void>();

const channel = typeof BroadcastChannel === 'undefined' ? null : new BroadcastChannel('tradoor-reach');
const tell = (note: Note): void => channel?.postMessage(note);

/** The page's storage; a worker has none and asks the page instead. */
function storage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

function merge(entries: [string, number][]): void {
  const since = Date.now() - BLOCKED_MS;
  for (const [key, at] of entries) if (at > since && at > (blocked.get(key) ?? 0)) blocked.set(key, at);
}

function save(): void {
  const since = Date.now() - BLOCKED_MS;
  try {
    storage()?.setItem(STORE, JSON.stringify([...blocked].filter(([, at]) => at > since)));
  } catch {
    // Private mode or a full store: the list lives for this page only.
  }
}

export const isBlocked = (key: string, now = Date.now()): boolean => (blocked.get(key) ?? 0) > now - BLOCKED_MS;

/** An address that failed to answer: a whole host when it never connected, or one URL. */
export function block(key: string): void {
  const at = Date.now();
  blocked.set(key, at);
  save();
  tell({ blocked: [[key, at]] });
}

/** It answered after all, whatever it did before. */
export function reached(key: string): void {
  if (!blocked.delete(key)) return;
  save();
  tell({ reached: key });
}

/** Called when the page comes back into view, or the network changes under it. */
export function onWake(wake: () => void): () => void {
  wakers.add(wake);
  return () => wakers.delete(wake);
}

function wakeAll(): void {
  for (const wake of wakers) wake();
}

/** The page is back: here, and in the worker and any other tab. */
export function wake(): void {
  wakeAll();
  tell({ wake: true });
}

if (channel) {
  channel.onmessage = (e: MessageEvent<Note>) => {
    const note = e.data;
    if ('blocked' in note) {
      merge(note.blocked);
      save();
    } else if ('reached' in note) {
      if (blocked.delete(note.reached)) save();
    } else if ('ask' in note) {
      if (storage()) tell({ blocked: [...blocked] });
    } else {
      wakeAll();
    }
  };
}

try {
  merge(JSON.parse(storage()?.getItem(STORE) ?? '[]') as [string, number][]);
} catch {
  // A list this page cannot read is one it starts without.
}
// The worker starts empty: whatever the page already knows, it wants now.
if (!storage()) tell({ ask: true });

// A phone unlocked, a tab brought back, a wifi joined: sockets left from before
// may be open and dead, and are better tested at once than in half a minute.
// Named loosely: this file also runs in the worker, and under the tests.
const page = (globalThis as { document?: { visibilityState: string; addEventListener(type: string, listener: () => void): void } }).document;
if (page) {
  page.addEventListener('visibilitychange', () => {
    if (page.visibilityState === 'visible') wake();
  });
  addEventListener('online', wake);
}
