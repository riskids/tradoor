/**
 * Price alerts: the list and log from the server, the actions on them, and the
 * watcher that fires them.
 *
 * The browser watches prices — it holds the exchanges' streams already — so an
 * alert fires the moment a chart sees it. One tab watches (the one holding the
 * `tradoor-alerts` lock); the others follow its changes over a BroadcastChannel,
 * so two tabs never fire one alert twice. The server watches too, for the alerts
 * nobody has open, and refuses a second trigger of one that already stopped —
 * which is why every tab reads the list again now and then, watching or not.
 */
import { createEffect, createRoot, createSignal } from 'solid-js';
import { createStore, produce } from 'solid-js/store';
import { api, catalog, errorText, type Alert, type AlertCondition, type AlertLog, type AlertStyle, type Catalog } from './api';
import { alertTitle, fires, rose } from './alert-rules';
import { watchDay } from './day-feed';
import { formatPrice, priceDecimals } from './format';
import { toast } from './toasts';
import { activate, addPanel, newPanel, tabsOf } from '../workspace/model';
import { doc, setPanelMarket, targetChart, update } from '../workspace/store';

interface State {
  alerts: Alert[];
  log: AlertLog[];
  loaded: boolean;
  error: string | null;
}

const [state, setState] = createStore<State>({ alerts: [], log: [], loaded: false, error: null });
export const alertState = state;

/** How often a tab reads the list again, for what fired elsewhere. */
const REFRESH_MS = 30_000;

const SEEN_KEY = 'tradoor:alerts-seen';
const readSeen = (): number => {
  try {
    return Number(localStorage.getItem(SEEN_KEY)) || 0;
  } catch {
    return 0;
  }
};
const [seen, setSeen] = createSignal(readSeen());

/** Log lines not looked at yet, for the badge. */
export const unread = (): number => state.log.filter((l) => l.time > seen()).length;

export function markLogSeen(): void {
  const newest = state.log.reduce((t, l) => Math.max(t, l.time), seen());
  setSeen(newest);
  try {
    localStorage.setItem(SEEN_KEY, String(newest));
  } catch {
    // A private window keeps it for the session only.
  }
}

const [products, setProducts] = createSignal<Catalog | null>(null);
/** Decimals a market's price is written with: the exchange's tick, else the price's own. */
export const digitsFor = (market: string, price: number): number => products()?.byMarket.get(market)?.priceDecimals ?? priceDecimals(price);
export const titleOf = (a: { market: string; condition: AlertCondition; price: number }): string => alertTitle(a, digitsFor(a.market, a.price));

const channel = typeof BroadcastChannel === 'function' ? new BroadcastChannel('tradoor-alerts') : null;
/** Tells the other tabs the list changed. */
const changed = () => channel?.postMessage('changed');

export async function loadAlerts(): Promise<void> {
  try {
    const { alerts, log } = await api.alerts();
    setState({ alerts, log, loaded: true, error: null });
  } catch (e) {
    setState({ loaded: true, error: errorText(e) });
  }
}

function upsert(alert: Alert): void {
  setState(
    produce((s) => {
      const i = s.alerts.findIndex((a) => a.id === alert.id);
      if (i === -1) s.alerts.unshift(alert);
      else s.alerts[i] = alert;
    }),
  );
}

// ------------------------------------------------------------------ actions

export async function createAlert(market: string, condition: AlertCondition, price: number, message: string | null = null, atPrice?: number): Promise<Alert> {
  askNotifications();
  const { alert } = await api.createAlert({ market, condition, price, message, atPrice });
  ensurePanel();
  upsert(alert);
  changed();
  return alert;
}

export async function editAlert(id: number, patch: { price?: number; condition?: AlertCondition; message?: string | null; atPrice?: number }): Promise<void> {
  const { alert } = await api.updateAlert(id, patch);
  upsert(alert);
  changed();
}

/** How its line is drawn; the alert itself is left as it was. */
export async function styleAlert(id: number, style: AlertStyle): Promise<void> {
  const { alert } = await api.updateAlert(id, { style });
  upsert(alert);
  changed();
}

export async function restartAlert(id: number): Promise<void> {
  const { alert } = await api.updateAlert(id, { active: true });
  upsert(alert);
  changed();
}

export async function stopAlert(id: number): Promise<void> {
  const { alert } = await api.updateAlert(id, { active: false });
  upsert(alert);
  changed();
}

export async function deleteAlert(id: number): Promise<void> {
  const before = state.alerts;
  setState('alerts', (list) => list.filter((a) => a.id !== id));
  try {
    await api.deleteAlert(id);
    changed();
  } catch (e) {
    setState('alerts', before);
    throw e;
  }
}

export async function clearAlerts(which: 'stopped' | 'active' | 'all'): Promise<void> {
  await api.clearAlerts(which);
  setState('alerts', (list) => list.filter((a) => (which === 'all' ? false : which === 'stopped' ? a.active : !a.active)));
  changed();
}

export async function restartAllAlerts(): Promise<void> {
  await api.restartAlerts();
  await loadAlerts();
  changed();
}

export async function clearAlertLog(): Promise<void> {
  await api.clearAlertLog();
  setState('log', []);
  changed();
}

/**
 * A layout made before alerts existed has nowhere to list them: the first
 * alert adds the panel, under the watchlist where there is one.
 */
function ensurePanel(): void {
  const panels = Object.values(doc.panels);
  if (panels.some((p) => p.type === 'alerts')) return;
  const beside = panels.find((p) => p.type === 'watchlist') ?? targetChart();
  update((d) => addPanel(d, newPanel('alerts'), beside ? (tabsOf(d.root, beside.id)?.id ?? null) : null, beside?.type === 'watchlist' ? 'bottom' : 'right'));
}

/** Brings the alerts panel to the front, adding it where the layout has none. */
export function showAlertsPanel(): void {
  ensurePanel();
  update((d) => {
    const panel = Object.values(d.panels).find((p) => p.type === 'alerts');
    const tabs = panel && tabsOf(d.root, panel.id);
    if (panel && tabs) activate(d, tabs.id, panel.id);
  });
}

/** Shows a market on the chart in use. */
export function openMarket(market: string): void {
  const chart = targetChart();
  if (chart) setPanelMarket(chart.id, market);
}

// ------------------------------------------------------------------ firing

let asked = false;
/** Asked once, on a click that makes an alert: the browser allows asking only then. */
function askNotifications(): void {
  if (asked || typeof Notification === 'undefined' || Notification.permission !== 'default') return;
  asked = true;
  void Notification.requestPermission().catch(() => {});
}

let audio: AudioContext | null = null;
/** Two short tones, as TradingView's default alert sound. */
function chime(): void {
  try {
    audio ??= new AudioContext();
    const ctx = audio;
    for (const [at, freq] of [[0, 880], [0.18, 1175]] as const) {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.frequency.value = freq;
      gain.gain.setValueAtTime(0.0001, ctx.currentTime + at);
      gain.gain.exponentialRampToValueAtTime(0.15, ctx.currentTime + at + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, ctx.currentTime + at + 0.16);
      osc.connect(gain).connect(ctx.destination);
      osc.start(ctx.currentTime + at);
      osc.stop(ctx.currentTime + at + 0.17);
    }
  } catch {
    // No sound where the browser has none to give.
  }
}

function announce(a: Alert, value: number, from: number | null): void {
  const title = titleOf(a);
  // Without the price it was set from, the price it came from says the way.
  const way = rose(a.condition, a.price, a.startPrice ?? from);
  const body = `${a.message ? `${a.message} · ` : ''}price ${formatPrice(value, digitsFor(a.market, value))}`;
  toast({ tone: 'alert', title, body, way, action: () => openMarket(a.market) }, 30_000);
  chime();
  // A notice from the system too when the page is out of sight.
  if (typeof Notification !== 'undefined' && Notification.permission === 'granted' && document.hidden) {
    try {
      const n = new Notification(title, { body, tag: `tradoor-alert-${a.id}` });
      n.onclick = () => {
        window.focus();
        openMarket(a.market);
        n.close();
      };
    } catch {
      // Some browsers allow notifications only from a service worker.
    }
  }
}

const last = new Map<string, number>();
const firing = new Set<number>();

function onPrice(market: string, cur: number): void {
  const prev = last.get(market) ?? null;
  last.set(market, cur);
  for (const a of state.alerts) {
    if (!a.active || a.market !== market || firing.has(a.id) || !fires(a.condition, a.price, prev, cur)) continue;
    firing.add(a.id);
    upsert({ ...a, active: false, triggeredAt: Date.now(), triggerPrice: cur });
    announce(a, cur, prev);
    api.triggerAlert(a.id, cur, prev).then(
      ({ alert, log }) => {
        upsert(alert);
        if (log) setState('log', (list) => [log, ...list]);
        changed();
      },
      () => {},
    ).finally(() => firing.delete(a.id));
  }
}

/** One stream per market with an active alert, while this tab is the one watching. */
function watch(): void {
  createRoot(() => {
    const subs = new Map<string, () => void>();
    createEffect(() => {
      const want = new Set(state.alerts.filter((a) => a.active).map((a) => a.market));
      for (const [m, off] of subs) {
        if (!want.has(m)) {
          off();
          subs.delete(m);
          last.delete(m);
        }
      }
      for (const m of want) if (!subs.has(m)) subs.set(m, watchDay(m, (day) => onPrice(m, day.close)));
    });
  });
}

let started = false;

export function startAlerts(): void {
  if (started) return;
  started = true;
  catalog().then(setProducts, () => {});
  void loadAlerts();
  // Another tab fired, made or deleted one: read the list again.
  if (channel) channel.onmessage = () => void loadAlerts();
  if (navigator.locks) {
    // Held until the tab closes; the next tab in line then takes over watching.
    void navigator.locks.request('tradoor-alerts', () => {
      watch();
      return new Promise<void>(() => {});
    });
  } else {
    watch();
  }
  // Every tab catches up now and then: the server fires the alerts no tab is
  // watching, and a message from another tab can be missed.
  setInterval(() => void loadAlerts(), REFRESH_MS);
}
