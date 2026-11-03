/**
 * The live workspace. Kept in localStorage so the page opens instantly and
 * offline, and on the server so it follows the reader to another browser.
 * Whichever copy changed last wins; writes carry the server version they were
 * based on, so two tabs cannot silently overwrite each other.
 */
import { batch, createSignal } from 'solid-js';
import { createStore, reconcile, unwrap } from 'solid-js/store';
import { api, ApiError, errorText, type LayoutMeta } from '../lib/api';
import { activeLayoutId, DEFAULT_LAYOUT, layoutIdFor, layoutSummary, localKeyFor, SESSION_KEY } from '../lib/layout';
import { identity } from '../lib/owner';
import { readJson, readPref, writePref } from '../lib/prefs';
import { toast } from '../lib/toasts';
import { defaultWorkspace, tabsOf, validate, type ChartPanelState, type LinkColor, type Panel, type WorkspaceDoc } from './model';
import { TIMEFRAMES } from '../lib/timeframes';

export type { LayoutMeta };
export { activeLayoutId };

// ponytail: key bumped once so browsers holding the old three-row layout open the TradingView-like default.
// One local copy per named layout (lib/layout.ts); the default one keeps the original key.
const SERVER_ID = activeLayoutId();
const LOCAL_KEY = localKeyFor(SERVER_ID);
/** Whose the local copy is, written beside it. */
const LOCAL_OWNER_KEY = 'workspace-owner';

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
  (localIsOurs(readPref(LOCAL_OWNER_KEY), identity()) ? validate(readJson(LOCAL_KEY)) : null) ?? defaultWorkspace(),
);
export { doc };

/**
 * Charts outside the layout — the training view's — kept beside the workspace
 * rather than in it, which drops any panel its tree does not place. The panel
 * setters below reach them by id all the same; whoever holds one saves it.
 */
const [detached, setDetached] = createStore<Record<string, ChartPanelState>>({});
export { detached };

export function holdDetached(panel: ChartPanelState): void {
  setDetached(panel.id, panel);
}

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
/** Set once this page is on its way to another layout, or this one is deleted: nothing may be written for it any more. */
let leaving = false;
let localTimer: ReturnType<typeof setTimeout> | undefined;
let serverTimer: ReturnType<typeof setTimeout> | undefined;

function saveLocal(): void {
  clearTimeout(localTimer);
  if (leaving) return;
  writePref(LOCAL_KEY, JSON.stringify(unwrap(doc)));
  writePref(LOCAL_OWNER_KEY, identity());
}

function touched(): void {
  setDoc('savedAt', Date.now());
  clearTimeout(localTimer);
  localTimer = setTimeout(saveLocal, 300);
  clearTimeout(serverTimer);
  if (saveState() !== 'local') setSaveState('pending');
  // With autosave off the change stays in this browser, marked unsaved, until Ctrl+S.
  if (autosave()) serverTimer = setTimeout(() => void saveServer(), 1500);
}

/** Whether changes reach the server by themselves; remembered by this browser. */
const [autosave, setAutosaveSignal] = createSignal(readPref('autosave') !== 'false');
export { autosave };
export function setAutosave(on: boolean): void {
  setAutosaveSignal(on);
  writePref('autosave', String(on));
  if (on && saveState() === 'pending') void saveServer();
}

/** The chart in use as a layout is listed by: `BTCUSDT.P, 240`. */
function summary(): string {
  const p = focusedPanel();
  const chart = p?.type === 'chart' ? p : targetChart();
  return chart ? layoutSummary(marketOf(chart), timeframeOf(chart)) : '';
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
  if (!doc.panels[id]) {
    if (detached[id]) (setDetached as (...args: unknown[]) => void)(id, patch);
    return;
  }
  (setDoc as (...args: unknown[]) => void)('panels', id, patch);
  touched();
}

export function setPanelMarket(id: string, market: string): void {
  const p = doc.panels[id];
  if (!p && detached[id]) {
    if (detached[id].market !== market) setDetached(id, 'market', market);
    return;
  }
  if (!p || marketOf(p) === market) return;
  batch(() => {
    if (p.link) setDoc('links', p.link, 'market', market);
    setDoc('panels', id, 'market', market);
  });
  touched();
}

export function setPanelTimeframe(id: string, timeframe: string): void {
  const p = doc.panels[id];
  if (!p && detached[id]) {
    if (detached[id].timeframe !== timeframe) setDetached(id, 'timeframe', timeframe);
    return;
  }
  if (!p || timeframeOf(p) === timeframe) return;
  batch(() => {
    if (p.link) setDoc('links', p.link, 'timeframe', timeframe);
    setDoc('panels', id, 'timeframe', timeframe);
  });
  touched();
}

/** A timeframe into the charts' bar of buttons, or back to their menu alone. */
export function toggleFavoriteTimeframe(timeframe: string): void {
  update((d) => {
    const on = d.timeframes.includes(timeframe);
    d.timeframes = TIMEFRAMES.map((t) => t.value).filter((tf) => (tf === timeframe ? !on : d.timeframes.includes(tf)));
  });
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
  if (leaving) return;
  if (saving) {
    saveAgain = true;
    return;
  }
  saving = true;
  setSaveState('saving');
  try {
    for (let attempt = 0; attempt < 2; attempt += 1) {
      const res = await api.saveWorkspace(SERVER_ID, { name: doc.name, doc: unwrap(doc), version: serverVersion, summary: summary() });
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
    // Unsaved edits kept in this browser stay unsaved while autosave is off.
    else if (!server || (server.savedAt < doc.savedAt && autosave())) await saveServer();
    else setSaveState(server.savedAt < doc.savedAt ? 'pending' : 'saved');
  } catch (e) {
    if (e instanceof ApiError && e.status === 404) {
      serverVersion = 0;
      await saveServer();
    } else {
      setSaveState('local');
    }
  }
  // Tell the server this layout was opened (it orders the list), then read the list.
  void api.patchWorkspace(SERVER_ID, { opened: true }).catch(() => {}).then(refreshLayouts);
}

// ---------------------------------------------------------------- layouts

const [layouts, setLayouts] = createSignal<LayoutMeta[]>([]);
export { layouts };

export async function refreshLayouts(): Promise<void> {
  try {
    setLayouts(await api.layouts());
  } catch {
    // Offline: the list stays as it was.
  }
}

/** Run a layout action: its failure is said in a notice, and the list is read again either way. */
async function act(what: string, run: () => Promise<void>): Promise<void> {
  try {
    await run();
  } catch (e) {
    toast({ tone: 'warn', title: `Could not ${what}`, body: errorText(e) }, 8_000);
  }
  await refreshLayouts();
}

/** Leave this page for another layout. */
function go(id: string): void {
  saveLocal();
  leaving = true;
  clearTimeout(serverTimer);
  try {
    sessionStorage.setItem(SESSION_KEY, id);
  } catch {
    // The address carries it.
  }
  // ponytail: a reload is the simplest way to reset every chart slot and its drawings. Upgrade path: swap the doc in place.
  location.search = `?layout=${encodeURIComponent(id)}`;
}

/** Open a layout in this tab, or in a new window. */
export function openLayout(id: string, newWindow = false): Promise<void> {
  return act('open the layout', async () => {
    await api.patchWorkspace(id, { opened: true }).catch(() => {});
    if (newWindow) {
      (globalThis as unknown as { open(url: string, target: string): void }).open(`${location.pathname}?layout=${encodeURIComponent(id)}`, '_blank');
      return;
    }
    if (id === SERVER_ID) return;
    // What was just changed goes up before this page lets go of it.
    if (autosave() && saveState() === 'pending') await saveServer();
    go(id);
  });
}

async function putNew(id: string, from: WorkspaceDoc): Promise<void> {
  const res = await api.saveWorkspace(id, { name: from.name, doc: { ...from, savedAt: Date.now() }, version: 0 });
  if (res.conflict) throw new Error('a layout with that id already exists');
}

const clean = (name: string): string => name.trim().slice(0, 120) || 'Untitled';

export function createLayout(name: string): Promise<void> {
  const id = layoutIdFor(name);
  return act('create the layout', async () => {
    await putNew(id, { ...defaultWorkspace(), name: clean(name) });
    await openLayout(id);
  });
}

/** A copy of this layout as it is now, under a new name. */
export function copyLayout(name: string): Promise<void> {
  const id = layoutIdFor(name);
  return act('copy the layout', async () => {
    await putNew(id, { ...structuredClone(unwrap(doc)), name: clean(name) });
    await openLayout(id);
  });
}

export function renameLayout(name: string): Promise<void> {
  return act('rename the layout', async () => {
    update((d) => void (d.name = clean(name)));
    await api.patchWorkspace(SERVER_ID, { name: clean(name) });
  });
}

export function deleteLayout(id: string): Promise<void> {
  return act('delete the layout', async () => {
    await api.deleteWorkspace(id);
    try {
      localStorage.removeItem(`tradoor:${localKeyFor(id)}`);
    } catch {
      // Blocked: nothing was kept.
    }
    if (id !== SERVER_ID) return;
    // This page must not write the deleted layout back on its way out.
    leaving = true;
    clearTimeout(serverTimer);
    const next = (await api.layouts().catch(() => [] as LayoutMeta[])).find((l) => l.id !== id)?.id ?? DEFAULT_LAYOUT;
    if (next === id) location.reload(); // the last one: a reload makes a fresh default
    else go(next);
  });
}

export function setStarred(id: string, on: boolean): Promise<void> {
  return act('star the layout', () => api.patchWorkspace(id, { starred: on }));
}

addEventListener('pagehide', saveLocal);
