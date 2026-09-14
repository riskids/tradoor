/**
 * The shell: a command bar, the workspace, a status line, and the dialogs.
 * Keyboard shortcuts go to the focused panel.
 */
import { createEffect, createSignal, For, onCleanup, onMount, Show } from 'solid-js';
import { CommandPalette, type Command } from '../components/CommandPalette';
import { Help } from '../components/Help';
import { IndicatorPicker } from '../components/IndicatorPicker';
import { NoteInput } from '../components/NoteInput';
import { Settings } from '../components/Settings';
import { SymbolSearch } from '../components/SymbolSearch';
import { onEngineFailure, send } from '../engine/client';
import { api } from '../lib/api';
import { formatClock } from '../lib/format';
import { Icon } from '../lib/icons';
import { CHART_THEME, FONT_PX, UI_THEME } from '../lib/theme';
import { stepTimeframe, TIMEFRAMES } from '../lib/timeframes';
import { Layout, panelTitle } from '../workspace/Layout';
import { addPanel, LINK_COLORS, newPanel, PANEL_TITLE, PANEL_TYPES, removePanel, tabsOf, type DropZone, type PanelType } from '../workspace/model';
import {
  doc,
  focusedPanel,
  focusPanel,
  marketOf,
  patchPanel,
  resetWorkspace,
  saveState,
  setPanelLink,
  setPanelMarket,
  setPanelTimeframe,
  syncWithServer,
  targetChart,
  timeframeOf,
  update,
} from '../workspace/store';
import { chartControllers, closeOverlay, isTypingTarget, overlay, setOverlay } from './ui';

const splitList = (s: string): string[] => s.split(',').filter(Boolean);

function addNear(type: PanelType, zone: DropZone): void {
  const from = focusedPanel() ?? targetChart();
  const panel = newPanel(type, from ? { link: from.link, market: marketOf(from), timeframe: timeframeOf(from) } : undefined);
  update((d) => addPanel(d, panel, from ? (tabsOf(d.root, from.id)?.id ?? null) : null, zone));
  focusPanel(panel.id);
}

/** A picked market lands in the panel that asked: a watchlist gains a row, anything else switches. */
function pickMarket(panelId: string | null, market: string): void {
  const panel = (panelId && doc.panels[panelId]) || targetChart();
  if (!panel) return;
  if (panel.type === 'watchlist') {
    if (!panel.markets.includes(market)) patchPanel(panel.id, { markets: [...panel.markets, market] });
  } else {
    setPanelMarket(panel.id, market);
  }
}

function commands(): Command[] {
  const focus = focusedPanel() ?? targetChart();
  const list: Command[] = [];
  if (focus && focus.type !== 'watchlist') {
    list.push({ group: 'Panel', label: 'Change symbol…', hint: 'a–z', run: () => setOverlay({ kind: 'symbol', panelId: focus.id, initial: '' }) });
    for (const tf of TIMEFRAMES) list.push({ group: 'Timeframe', label: tf.name, hint: tf.label, run: () => setPanelTimeframe(focus.id, tf.value) });
  }
  if (focus?.type === 'chart') {
    const chart = focus;
    list.push(
      { group: 'Chart', label: 'Add indicator…', run: () => setOverlay({ kind: 'indicators', panelId: chart.id }) },
      { group: 'Chart', label: chart.log ? 'Linear price scale' : 'Logarithmic price scale', run: () => patchPanel(chart.id, { log: !chart.log }) },
      { group: 'Chart', label: 'Reset chart view', run: () => chartControllers.get(chart.id)?.resetView() },
      { group: 'Chart', label: 'Show all loaded history', run: () => chartControllers.get(chart.id)?.showRange(null) },
    );
  }
  const zones: [DropZone, string][] = [
    ['center', 'as a tab'],
    ['right', 'split right'],
    ['bottom', 'split down'],
  ];
  for (const type of PANEL_TYPES) for (const [zone, where] of zones) list.push({ group: 'New panel', label: `${PANEL_TITLE[type]} — ${where}`, run: () => addNear(type, zone) });
  if (focus) {
    list.push({ group: 'Link', label: 'Unlink this panel', run: () => setPanelLink(focus.id, null) });
    for (const color of LINK_COLORS) list.push({ group: 'Link', label: `Link this panel to ${color}`, run: () => setPanelLink(focus.id, color) });
    list.push({ group: 'Panel', label: `Close ${panelTitle(focus)}`, run: () => update((d) => removePanel(d, focus.id)) });
  }
  list.push(
    { group: 'App', label: 'Settings and colours…', run: () => setOverlay({ kind: 'settings' }) },
    { group: 'App', label: 'Keyboard shortcuts', hint: '?', run: () => setOverlay({ kind: 'help' }) },
    {
      group: 'App',
      label: 'Reset workspace to default',
      run: () => {
        if (confirm('Replace this workspace with the default layout?')) resetWorkspace();
      },
    },
  );
  return list;
}

function onKeyDown(e: KeyboardEvent): void {
  if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === 'k') {
    e.preventDefault();
    setOverlay(overlay()?.kind === 'palette' ? null : { kind: 'palette' });
    return;
  }
  // Dialogs own the keyboard while open; modifiers belong to the browser.
  if (overlay() || e.metaKey || e.ctrlKey || e.altKey || isTypingTarget(e.target)) return;
  if (e.key === '?') {
    e.preventDefault();
    setOverlay({ kind: 'help' });
    return;
  }
  if (e.key === '/') {
    e.preventDefault();
    setOverlay({ kind: 'palette' });
    return;
  }
  const panel = focusedPanel() ?? targetChart();
  if (!panel) return;
  if (panel.type === 'chart' && (e.key === 'Escape' || e.key === 'Enter' || e.key === 'Delete' || e.key === 'Backspace')) {
    if (e.key === 'Delete' || e.key === 'Backspace') e.preventDefault();
    void chartControllers.get(panel.id)?.key(e.key);
    return;
  }
  if (panel.type !== 'watchlist' && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
    e.preventDefault();
    setPanelTimeframe(panel.id, stepTimeframe(timeframeOf(panel), e.key === 'ArrowLeft' ? -1 : 1));
    return;
  }
  // Letters only: typing a ticker opens the search with that letter already in it.
  if (/^[a-z]$/i.test(e.key)) {
    e.preventDefault();
    setOverlay({ kind: 'symbol', panelId: panel.id, initial: e.key.toUpperCase() });
  }
}

function applyTheme(): void {
  for (const t of CHART_THEME) createEffect(() => send({ t: 'theme', key: t.name, value: doc.theme[t.name] ?? t.value }));
  createEffect(() => send({ t: 'theme', key: FONT_PX.name, value: doc.theme[FONT_PX.name] ?? FONT_PX.value }));
  for (const t of UI_THEME) {
    createEffect(() => {
      const value = doc.theme[t.name];
      if (value) document.documentElement.style.setProperty(t.css, value);
      else document.documentElement.style.removeProperty(t.css);
    });
  }
}

function Health() {
  const [state, setState] = createSignal<'checking' | 'ok' | 'db' | 'down'>('checking');
  const check = async () => {
    try {
      setState((await api.health()).ok ? 'ok' : 'db');
    } catch {
      setState('down');
    }
  };
  onMount(() => {
    void check();
    const timer = setInterval(() => void check(), 60_000);
    onCleanup(() => clearInterval(timer));
  });
  const title = () =>
    ({
      checking: 'Checking the tradoor server…',
      ok: 'Server and database reachable: drawings and layouts are saved.',
      db: 'The server answers but its database does not: drawings cannot be saved.',
      down: 'The tradoor server is not reachable. Charts still load from the exchanges; drawings and layout sync are paused.',
    })[state()];
  return (
    <span class="item" title={title()}>
      <span classList={{ bull: state() === 'ok', warn: state() === 'db', bear: state() === 'down', dim: state() === 'checking' }}>●</span>
      {state() === 'down' ? 'OFFLINE' : state() === 'db' ? 'NO DB' : 'SERVER'}
    </span>
  );
}

function Clock() {
  const [now, setNow] = createSignal(Date.now());
  const timer = setInterval(() => setNow(Date.now()), 1000);
  onCleanup(() => clearInterval(timer));
  return (
    <span class="item num" title="Candles are stored and drawn in UTC">
      <span class="dim">UTC</span>
      {formatClock(now())}
    </span>
  );
}

const SAVE_TEXT = {
  saved: 'layout saved',
  pending: 'layout changed',
  saving: 'saving layout…',
  local: 'layout saved in this browser only',
  conflict: 'another tab changed this layout',
} as const;

function StatusBar() {
  return (
    <footer class="statusbar">
      <span class="caps">{focusedPanel() ? panelTitle(focusedPanel()!) : 'no panel focused'}</span>
      <For
        each={[
          ['a–z', 'symbol'],
          ['← →', 'timeframe'],
          ['/', 'commands'],
          ['?', 'help'],
        ]}
      >
        {([key, label]) => (
          <span class="hint-key">
            <kbd>{key}</kbd>
            {label}
          </span>
        )}
      </For>
      <span class="spacer" />
      <span classList={{ warn: saveState() === 'local' || saveState() === 'conflict' }}>{SAVE_TEXT[saveState()]}</span>
    </footer>
  );
}

function Overlays() {
  return (
    <Show when={overlay()} keyed>
      {(o) => {
        switch (o.kind) {
          case 'symbol': {
            const panel = o.panelId ? doc.panels[o.panelId] : undefined;
            return (
              <SymbolSearch
                initial={o.initial}
                title={panel?.type === 'watchlist' ? 'Add to watchlist' : 'Symbol'}
                current={panel && panel.type !== 'watchlist' ? marketOf(panel) : undefined}
                onClose={closeOverlay}
                onSelect={(market) => {
                  closeOverlay();
                  pickMarket(o.panelId, market);
                }}
              />
            );
          }
          case 'indicators': {
            const panel = doc.panels[o.panelId];
            if (panel?.type !== 'chart') return null;
            return (
              <IndicatorPicker
                active={splitList(panel.studies)}
                onClose={closeOverlay}
                onAdd={(study) => {
                  closeOverlay();
                  const list = splitList(panel.studies);
                  if (!list.includes(study)) patchPanel(panel.id, { studies: [...list, study].join(',') });
                }}
              />
            );
          }
          case 'palette':
            return <CommandPalette commands={commands()} onClose={closeOverlay} />;
          case 'settings':
            return <Settings onClose={closeOverlay} />;
          case 'help':
            return <Help onClose={closeOverlay} />;
          case 'note':
            return <NoteInput onClose={closeOverlay} onSave={o.save} />;
        }
      }}
    </Show>
  );
}

export function App() {
  const [engineError, setEngineError] = createSignal<string | null>(null);
  applyTheme();
  onMount(() => {
    const chart = targetChart();
    if (chart) focusPanel(chart.id);
    void syncWithServer();
    addEventListener('keydown', onKeyDown);
    const off = onEngineFailure(setEngineError);
    onCleanup(() => {
      removeEventListener('keydown', onKeyDown);
      off();
    });
  });

  return (
    <div class="shell">
      <header class="topbar">
        <div class="brand">TRADOOR</div>
        <button class="item" title="Commands (/ or Ctrl K)" onClick={() => setOverlay({ kind: 'palette' })}>
          <Icon name="command" size={12} /> Commands <kbd>/</kbd>
        </button>
        <button class="item" title="Open a chart beside the focused panel" onClick={() => addNear('chart', 'right')}>
          <Icon name="plus" size={12} /> Chart
        </button>
        <span class="item dim">{doc.name}</span>
        <span class="spacer" />
        <Health />
        <Clock />
        <button class="item" title="Settings and colours" onClick={() => setOverlay({ kind: 'settings' })}>
          <Icon name="settings" size={13} />
        </button>
        <button class="item" title="Keyboard shortcuts (?)" onClick={() => setOverlay({ kind: 'help' })}>
          ?
        </button>
      </header>
      <main style={{ display: 'flex', 'flex-direction': 'column', 'min-height': '0', overflow: 'hidden' }}>
        <Show when={engineError()}>{(message) => <div class="banner">▲ Charts cannot be drawn: {message()}</div>}</Show>
        <Layout />
      </main>
      <StatusBar />
      <Overlays />
    </div>
  );
}
