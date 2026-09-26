/**
 * The live workspace. Kept in localStorage so the page opens instantly and
 * offline, and on the server so it follows the reader to another browser.
 * Whichever copy changed last wins; writes carry the server version they were
 * based on, so two tabs cannot silently overwrite each other.
 */
import { batch, createSignal } from 'solid-js';
import { createStore, reconcile, unwrap } from 'solid-js/store';
import { api, ApiError } from '../lib/api';
import { ownerId } from '../lib/owner';
import { readJson, readPref, writePref } from '../lib/prefs';
import { defaultWorkspace, tabsOf, validate, type ChartPanelState, type LinkColor, type Panel, type WorkspaceDoc } from './model';

const LOCAL_KEY = 'workspace';
/** Whose the local copy is, written beside it. */
const LOCAL_OWNER_KEY = 'workspace-owner';
const SERVER_ID = 'default';

/**
 * Whether the copy in this browser is this browser's own to open with. Taking
 * another browser's id means asking for their layout, and theirs is on the
 * server: this one is a stranger's now, and opening with it would not only
 * show the wrong workspace but save it over the one that was asked for, since
 * whichever copy changed last wins. A copy from before the stamp existed has
 * no owner written beside it and is this browser's own.
 */
export const localIsOurs = (stamped: string | null, owner: string): boolean => stamped === null || stamped === owner;

const [doc, setDoc] = createStore<WorkspaceDoc>(
  (localIsOurs(readPref(LOCAL_OWNER_KEY), ownerId()) ? validate(readJson(LOCAL_KEY)) : null) ?? defaultWorkspace(),
);
export { doc };

export type SaveState = 'saved' | 'pending' | 'saving' | 'local' | 'conflict';
const [saveState, setSaveState] = createSignal<SaveState>('saved');
export { saveState };

const [focused, setFocused] = createSignal<string | null>(null);
const [lastChart, setLastChart] = createSignal<string | null>(null);
export const focusedPanelId = focused;

export function focusPanel(id: string): void {
  if (focused() !== id) setFocused(id);
  if (doc.panels[id]?.type === 'chart' && lastChart() !== id) setLastChart(id);
}

export function focusedPanel(): Panel | null {
  const id = focused();
  return (id && doc.panels[id]) || null;
}

/** The chart a watchlist that is not linked should drive: the last one used. */
export function targetChart(): ChartPanelState | null {
  const id = lastChart();
  const last = id ? doc.panels[id] : undefined;
  if (last?.type === 'chart') return last;
  const charts = Object.values(doc.panels).filter((p): p is ChartPanelState => p.type === 'chart');
  // Before one is used, the first on screen: a chart behind another tab has drawn nothing, not even a price.
  return charts.find((p) => tabsOf(doc.root, p.id)?.active === p.id) ?? charts[0] ?? null;
}

export const marketOf = (p: Panel): string => (p.link ? doc.links[p.link].market : p.market);
export const timeframeOf = (p: Panel): string => (p.link ? doc.links[p.link].timeframe : p.timeframe);

// ---------------------------------------------------------------- changes

let serverVersion = 0;
let localTimer: ReturnType<typeof setTimeout> | undefined;
let serverTimer: ReturnType<typeof setTimeout> | undefined;

function saveLocal(): void {
  clearTimeout(localTimer);
  writePref(LOCAL_KEY, JSON.stringify(unwrap(doc)));
  writePref(LOCAL_OWNER_KEY, ownerId());
}

function touched(): void {
  setDoc('savedAt', Date.now());
  clearTimeout(localTimer);
  localTimer = setTimeout(saveLocal, 300);
  clearTimeout(serverTimer);
  if (saveState() !== 'local') setSaveState('pending');
  serverTimer = setTimeout(() => void saveServer(), 1500);
}

/** Write the workspace this instant. It saves itself a second or so after every
 *  change; this only brings that forward, for a reader who reached for Ctrl+S. */
export function saveNow(): void {
  clearTimeout(localTimer);
  saveLocal();
  clearTimeout(serverTimer);
  void saveServer();
}

/** Structural changes: clone, mutate, reconcile. */
export function update(mutate: (draft: WorkspaceDoc) => void): void {
  const draft = structuredClone(unwrap(doc));
  mutate(draft);
  setDoc(reconcile(draft));
  touched();
}

export function patchPanel(id: string, patch: Partial<Omit<ChartPanelState, 'id' | 'type'>> | Record<string, unknown>): void {
  if (!doc.panels[id]) return;
  (setDoc as (...args: unknown[]) => void)('panels', id, patch);
  touched();
}

export function setPanelMarket(id: string, market: string): void {
  const p = doc.panels[id];
  if (!p || marketOf(p) === market) return;
  batch(() => {
    if (p.link) setDoc('links', p.link, 'market', market);
    setDoc('panels', id, 'market', market);
  });
  touched();
}

export function setPanelTimeframe(id: string, timeframe: string): void {
  const p = doc.panels[id];
  if (!p || timeframeOf(p) === timeframe) return;
  batch(() => {
    if (p.link) setDoc('links', p.link, 'timeframe', timeframe);
    setDoc('panels', id, 'timeframe', timeframe);
  });
  touched();
}

/** Joining a group adopts the group's market and timeframe. */
export function setPanelLink(id: string, color: LinkColor | null): void {
  const p = doc.panels[id];
  if (!p) return;
  batch(() => {
    if (color) {
      setDoc('panels', id, 'market', doc.links[color].market);
      setDoc('panels', id, 'timeframe', doc.links[color].timeframe);
    } else if (p.link) {
      setDoc('panels', id, 'market', doc.links[p.link].market);
      setDoc('panels', id, 'timeframe', doc.links[p.link].timeframe);
    }
    setDoc('panels', id, 'link', color);
  });
  touched();
}

export function setLinkMarket(color: LinkColor, market: string): void {
  if (doc.links[color].market === market) return;
  setDoc('links', color, 'market', market);
  touched();
}

export function setThemeValue(key: string, value: string | null): void {
  (setDoc as (...args: unknown[]) => void)('theme', key, value ?? undefined);
  touched();
}

export function resetWorkspace(): void {
  update((d) => Object.assign(d, defaultWorkspace()));
}

// ---------------------------------------------------------------- server

let saving = false;
let saveAgain = false;

function adopt(server: WorkspaceDoc): void {
  setDoc(reconcile(server));
  saveLocal();
  setSaveState('saved');
}

async function saveServer(): Promise<void> {
  if (saving) {
    saveAgain = true;
    return;
  }
  saving = true;
  setSaveState('saving');
  try {
    for (let attempt = 0; attempt < 2; attempt += 1) {
      const res = await api.saveWorkspace(SERVER_ID, { name: doc.name, doc: unwrap(doc), version: serverVersion });
      serverVersion = res.version;
      if (!res.conflict) {
        setSaveState('saved');
        break;
      }
      // Another tab saved first. The newer copy wins; ours is retried on top of its version.
      const server = validate(res.doc);
      if (server && server.savedAt > doc.savedAt) {
        adopt(server);
        break;
      }
      if (attempt === 1) setSaveState('conflict');
    }
  } catch {
    setSaveState('local');
  } finally {
    saving = false;
    if (saveAgain) {
      saveAgain = false;
      void saveServer();
    }
  }
}

export async function syncWithServer(): Promise<void> {
  try {
    const res = await api.workspace(SERVER_ID);
    serverVersion = res.version;
    const server = validate(res.doc);
    if (server && server.savedAt > doc.savedAt) adopt(server);
    else if (!server || server.savedAt < doc.savedAt) void saveServer();
    else setSaveState('saved');
  } catch (e) {
    if (e instanceof ApiError && e.status === 404) {
      serverVersion = 0;
      void saveServer();
    } else {
      setSaveState('local');
    }
  }
}

addEventListener('pagehide', saveLocal);
