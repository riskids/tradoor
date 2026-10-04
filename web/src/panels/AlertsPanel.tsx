/**
 * Price alerts as TradingView lists them: every alert with its state — Active,
 * or Stopped once it has triggered — and a log of what fired. An alert is made
 * from the chart's price axis (the clock by the crosshair's price) or here.
 */
import { createEffect, createMemo, createSignal, For, onCleanup, Show } from 'solid-js';
import { setOverlay } from '../app/ui';
import { lastPrice } from '../engine/client';
import { conditionLabel, CONDITIONS, rose } from '../lib/alert-rules';
import {
  alertState,
  clearAlertLog,
  clearAlerts,
  createAlert,
  deleteAlert,
  digitsFor,
  editAlert,
  markLogSeen,
  openMarket,
  restartAlert,
  restartAllAlerts,
  titleOf,
  unread,
} from '../lib/alerts';
import { api, errorText, type Alert, type AlertCondition } from '../lib/api';
import { useDismiss } from '../lib/dismiss';
import { ExchangeLogo, exchangeOfVenue } from '../lib/exchanges';
import { formatPrice } from '../lib/format';
import { Icon } from '../lib/icons';
import { parseMarket, symbolOf } from '../lib/markets';
import { formatAlertTime } from '../lib/timezone';
import { toast } from '../lib/toasts';
import type { AlertSort, AlertsPanelState } from '../workspace/model';
import { marketOf, patchPanel, targetChart } from '../workspace/store';

const SORTS: { value: AlertSort; label: string }[] = [
  { value: 'newest', label: 'Newest first' },
  { value: 'symbol', label: 'By symbol' },
  { value: 'state', label: 'Active first' },
];

/** The alert being made or edited in the form. */
interface Form {
  id: number | null;
  market: string;
  condition: AlertCondition;
  price: string;
  message: string;
}

const failed = (what: string) => (e: unknown) => toast({ tone: 'warn', title: what, body: errorText(e) });

function MarketTag(props: { market: string }) {
  return (
    <span class="al-market">
      <ExchangeLogo exchange={exchangeOfVenue(parseMarket(props.market)?.venue ?? 'binance')} size={12} />
      {symbolOf(props.market)}
    </span>
  );
}

/** Which way the price came to the level: green up, red down, nothing unknown. */
function Way(props: { condition: AlertCondition; level: number; startPrice: number | null }) {
  const way = () => rose(props.condition, props.level, props.startPrice);
  return (
    <Show when={way() !== null}>
      <span class={way() ? 'bull' : 'bear'}>{way() ? '▲' : '▼'} </span>
    </Show>
  );
}

export function AlertsPanel(props: { panel: AlertsPanelState }) {
  const id = props.panel.id;
  const [query, setQuery] = createSignal('');
  const [searching, setSearching] = createSignal(false);
  const [menu, setMenu] = createSignal(false);
  const [form, setForm] = createSignal<Form | null>(null);
  useDismiss(menu, () => setMenu(false));

  const tab = () => props.panel.tab;
  const setTab = (t: 'alerts' | 'log') => {
    patchPanel(id, { tab: t });
    if (t === 'log') markLogSeen();
  };
  // A log shown is a log read.
  createEffect(() => {
    if (tab() === 'log' && alertState.log.length) markLogSeen();
  });

  const matches = (market: string, text: string) => {
    const q = query().trim().toUpperCase();
    return !q || market.toUpperCase().includes(q) || text.toUpperCase().includes(q);
  };

  const alerts = createMemo(() => {
    const list = alertState.alerts.filter((a) => matches(a.market, a.message ?? ''));
    const sort = props.panel.sort;
    return [...list].sort((a, b) => {
      if (sort === 'symbol') return symbolOf(a.market).localeCompare(symbolOf(b.market)) || b.id - a.id;
      if (sort === 'state') return Number(b.active) - Number(a.active) || b.id - a.id;
      return b.id - a.id;
    });
  });
  const log = createMemo(() => alertState.log.filter((l) => matches(l.market, l.message ?? '')));
  const stopped = () => alertState.alerts.filter((a) => !a.active).length;

  // The bot is set up on this server but this browser never linked a chat, so
  // everything that fires goes nowhere: the panel says so rather than keeping
  // the quiet to itself.
  const [telegramMissing, setTelegramMissing] = createSignal(false);
  const readTelegram = async () => {
    try {
      const state = await api.telegram();
      setTelegramMissing(state.enabled && state.chats.length === 0);
    } catch {
      setTelegramMissing(false);
    }
  };
  void readTelegram();
  const telegramTimer = setInterval(() => void readTelegram(), 60_000);
  onCleanup(() => clearInterval(telegramTimer));

  /** A new alert on the chart in use, at its last price. */
  const newAlert = () => {
    const chart = targetChart();
    if (!chart) return;
    const market = marketOf(chart);
    const p = lastPrice(chart.id);
    const price = p && p.market === market ? formatPrice(p.price, digitsFor(market, p.price)).replaceAll(',', '') : '';
    setForm({ id: null, market, condition: 'crossing', price, message: '' });
  };
  const edit = (a: Alert) => setForm({ id: a.id, market: a.market, condition: a.condition, price: String(a.price), message: a.message ?? '' });

  /** What the market costs now, which says which way a level is reached. */
  const marketPrice = (market: string): number | undefined => {
    const chart = targetChart();
    const p = chart && lastPrice(chart.id);
    return p && p.market === market && Number.isFinite(p.price) ? p.price : undefined;
  };

  const save = async () => {
    const f = form();
    if (!f) return;
    const price = Number(f.price.replaceAll(',', ''));
    if (!(price > 0)) {
      toast({ tone: 'warn', title: 'Enter a price above zero' });
      return;
    }
    const atPrice = marketPrice(f.market);
    try {
      if (f.id === null) await createAlert(f.market, f.condition, price, f.message.trim() || null, atPrice);
      else await editAlert(f.id, { condition: f.condition, price, message: f.message.trim() || null, atPrice });
      setForm(null);
    } catch (e) {
      failed(f.id === null ? 'Alert not created' : 'Alert not saved')(e);
    }
  };

  const run = (f: () => Promise<void>, what: string) => () => {
    setMenu(false);
    f().catch(failed(what));
  };
  const triggered = (n: number) => `${n} triggered alert${n === 1 ? '' : 's'}`;
  const entries = (n: number) => `${n} log ${n === 1 ? 'entry' : 'entries'}`;
  const restartTriggered = run(restartAllAlerts, 'Alerts not restarted');
  const deleteTriggered = () => {
    if (confirm(`Delete ${triggered(stopped())}?`)) run(() => clearAlerts('stopped'), 'Alerts not deleted')();
    else setMenu(false);
  };
  const clearLog = () => {
    if (confirm(`Clear ${entries(alertState.log.length)}?`)) run(clearAlertLog, 'Log not cleared')();
    else setMenu(false);
  };

  return (
    <div class="panelcol al">
      <div class="al-tabs">
        <button classList={{ on: tab() === 'alerts' }} onClick={() => setTab('alerts')}>
          Alerts
        </button>
        <button classList={{ on: tab() === 'log' }} onClick={() => setTab('log')}>
          Log
          <Show when={unread() > 0}>
            <span class="al-badge">{unread()}</span>
          </Show>
        </button>
      </div>
      <Show when={telegramMissing()}>
        <button class="al-notice" onClick={() => setOverlay({ kind: 'settings' })}>
          ▲ Alerts are not going to Telegram — this browser has no chat linked. Connect it in Settings.
        </button>
      </Show>
      <div class="toolbar">
        <Show when={tab() === 'alerts'}>
          <button class="iconbtn" data-tip="Create an alert on the chart's market" onClick={newAlert}>
            <Icon name="alarm" size={14} />
          </button>
        </Show>
        <Show when={searching()} fallback={<span class="spacer" />}>
          <input class="al-search" placeholder="Search" value={query()} ref={(el) => setTimeout(() => el.focus())} onInput={(e) => setQuery(e.currentTarget.value)} onKeyDown={(e) => e.key === 'Escape' && (setQuery(''), setSearching(false))} />
        </Show>
        <button class="iconbtn" classList={{ on: searching() }} data-tip="Search" onClick={() => (setSearching(!searching()), setQuery(''))}>
          <Icon name="search" size={13} />
        </button>
        <Show when={tab() === 'alerts'}>
          <button
            class="iconbtn"
            data-tip={`Sort: ${SORTS.find((s) => s.value === props.panel.sort)?.label}`}
            onClick={() => patchPanel(id, { sort: SORTS[(SORTS.findIndex((s) => s.value === props.panel.sort) + 1) % SORTS.length]!.value })}
          >
            <Icon name="sort" size={13} />
          </button>
        </Show>
        <span class="al-more">
          <button class="iconbtn" data-tip="More" onClick={() => setMenu(!menu())}>
            <Icon name="more" size={14} stroke={3} />
          </button>
          <Show when={menu()}>
            <div class="menu" style={{ right: '0', top: '24px' }} onPointerDown={(e) => e.stopPropagation()}>
              <button disabled={!stopped()} onClick={restartTriggered}>
                Restart all triggered
              </button>
              <button disabled={!stopped()} onClick={deleteTriggered}>
                Delete all triggered
              </button>
              <button disabled={!alertState.log.length} onClick={clearLog}>
                Clear log
              </button>
              <hr />
              <button
                disabled={!alertState.alerts.length}
                onClick={() => {
                  if (!confirm(`Delete all ${alertState.alerts.length} alerts?`)) return;
                  run(() => clearAlerts('all'), 'Alerts not deleted')();
                }}
              >
                Delete all alerts
              </button>
            </div>
          </Show>
        </span>
      </div>

      {/* The log's clean-up in sight rather than only under the menu. */}
      <Show when={tab() === 'log' && alertState.log.length > 0}>
        <div class="al-bulk">
          <span>{alertState.log.length === 1 ? '1 entry' : `${alertState.log.length} entries`}</span>
          <button class="del" onClick={clearLog}>
            <Icon name="trash" size={12} />
            <span>Clear log</span>
          </button>
        </div>
      </Show>

      <Show when={form()}>
        {(f) => (
          <form
            class="al-form"
            onSubmit={(e) => {
              e.preventDefault();
              void save();
            }}
          >
            <div class="al-form-title">
              {f().id === null ? 'New alert' : 'Edit alert'} · <MarketTag market={f().market} />
            </div>
            <label>
              Condition
              <select value={f().condition} onChange={(e) => setForm({ ...f(), condition: e.currentTarget.value as AlertCondition })}>
                <For each={CONDITIONS}>{(c) => <option value={c.value}>{c.label}</option>}</For>
              </select>
            </label>
            <label>
              Price
              <input inputmode="decimal" value={f().price} onInput={(e) => setForm({ ...f(), price: e.currentTarget.value })} />
            </label>
            <label>
              Message
              <input placeholder="Optional" maxLength={280} value={f().message} onInput={(e) => setForm({ ...f(), message: e.currentTarget.value })} />
            </label>
            <div class="al-form-actions">
              <button type="button" class="tb" onClick={() => setForm(null)}>
                Cancel
              </button>
              <button type="submit" class="tb on">
                {f().id === null ? 'Create' : 'Save'}
              </button>
            </div>
          </form>
        )}
      </Show>

      <div class="scroll">
        <Show when={tab() === 'alerts'}>
          <For each={alerts()} fallback={<div class="empty">{alertState.error ? `▲ ${alertState.error}` : query() ? 'No alert matches.' : 'No alerts. Hover the chart’s price axis and click the clock, or use the clock above.'}</div>}>
            {(a) => (
              <div class="al-row" classList={{ stopped: !a.active }} onClick={() => openMarket(a.market)}>
                <div class="al-title">
                  <Way condition={a.condition} level={a.price} startPrice={a.startPrice} />
                  {titleOf(a)}
                </div>
                <Show when={a.message}>
                  <div class="al-message">{a.message}</div>
                </Show>
                <div class="al-sub">
                  <MarketTag market={a.market} />
                  <span class="dim">•</span>
                  <Show when={a.active} fallback={<span class="al-state stopped">Stopped — Triggered</span>}>
                    <span class="al-state active">Active</span>
                  </Show>
                  <Show when={!a.active && a.triggeredAt}>
                    <span class="dim">• {formatAlertTime(a.triggeredAt!)}</span>
                  </Show>
                </div>
                <div class="al-actions" onClick={(e) => e.stopPropagation()}>
                  <Show when={!a.active}>
                    <button data-tip="Restart" aria-label="Restart" onClick={() => restartAlert(a.id).catch(failed('Alert not restarted'))}>
                      <Icon name="reset" size={13} />
                    </button>
                  </Show>
                  <button data-tip="Edit" aria-label="Edit" onClick={() => edit(a)}>
                    <Icon name="pencil" size={13} />
                  </button>
                  <button class="del" data-tip="Delete" aria-label="Delete" onClick={() => deleteAlert(a.id).catch(failed('Alert not deleted'))}>
                    <Icon name="trash" size={13} />
                  </button>
                </div>
              </div>
            )}
          </For>
        </Show>
        <Show when={tab() === 'log'}>
          <For each={log()} fallback={<div class="empty">Nothing has triggered yet.</div>}>
            {(l) => (
              <div class="al-row" onClick={() => openMarket(l.market)}>
                <div class="al-title">
                  <Way condition={l.condition} level={l.price} startPrice={l.startPrice} />
                  {titleOf(l)}
                </div>
                <Show when={l.message}>
                  <div class="al-message">{l.message}</div>
                </Show>
                <div class="al-sub">
                  <MarketTag market={l.market} />
                  <span class="dim">•</span>
                  <span>
                    {conditionLabel(l.condition)} at {formatPrice(l.value, digitsFor(l.market, l.value))}
                  </span>
                  <span class="dim">• {formatAlertTime(l.time)}</span>
                </div>
              </div>
            )}
          </For>
        </Show>
      </div>
    </div>
  );
}
