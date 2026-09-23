/**
 * The terminal layout: splits with draggable gutters, tab groups, and docking
 * by dragging a tab onto another group's centre or edge.
 */
import { createSignal, For, Match, onCleanup, Show, Switch, untrack, type JSX } from 'solid-js';
import { dragging, setDragging } from '../app/ui';
import { useDismiss } from '../lib/dismiss';
import { Icon } from '../lib/icons';
import { symbolOf } from '../lib/markets';
import { tfLabel } from '../lib/timeframes';
import { ChartPanel } from '../panels/ChartPanel';
import { LiquidationsPanel } from '../panels/LiquidationsPanel';
import { ValuesPanel } from '../panels/ValuesPanel';
import { WatchlistPanel } from '../panels/WatchlistPanel';
import { activate, addPanel, LINK_COLORS, movePanel, newPanel, PANEL_TITLE, PANEL_TYPES, removePanel, setSizes, type DropZone, type LayoutNode, type Panel, type PanelType, type SplitNode, type TabsNode } from './model';
import { doc, focusedPanelId, focusPanel, marketOf, setPanelLink, targetChart, timeframeOf, update } from './store';

/** Smallest a cell may be dragged to, in pixels. */
const MIN_CELL = 90;

export function panelTitle(p: Panel): string {
  if (p.type === 'watchlist') return 'Watchlist';
  const label = `${symbolOf(marketOf(p))} ${tfLabel(timeframeOf(p))}`;
  if (p.type === 'chart') return p.tpo ? `TPO · ${label}` : label;
  return `${PANEL_TITLE[p.type]} · ${label}`;
}

/** Phones and narrow windows: one column the page scrolls, not splits. */
const MOBILE_QUERY = '(max-width: 760px)';

function useMedia(query: string): () => boolean {
  const media = matchMedia(query);
  const [matches, setMatches] = createSignal(media.matches);
  const change = () => setMatches(media.matches);
  media.addEventListener('change', change);
  onCleanup(() => media.removeEventListener('change', change));
  return matches;
}

export function Layout() {
  const mobile = useMedia(MOBILE_QUERY);
  return (
    <div class="workspace" classList={{ mobile: mobile() }}>
      <Show when={mobile()} fallback={<NodeView node={doc.root} />}>
        <MobileStack />
      </Show>
    </div>
  );
}

/**
 * The same workspace laid out for a phone: the chart in use first, as tall as
 * the screen allows with the next panel's header peeking below it, then every
 * other panel in the order the layout holds them. Nothing about the saved
 * layout changes — turn the phone or widen the window and the splits are back.
 */
function MobileStack() {
  // Fixed on the way in: touching a chart makes it the one in use, and a panel
  // that jumped to the top under the finger would be unusable.
  const lead = untrack(() => targetChart()?.id ?? null);
  const order = () => {
    const ids: string[] = [];
    const walk = (node: LayoutNode): void => {
      if (node.kind === 'split') node.children.forEach(walk);
      else ids.push(...node.panels);
    };
    walk(doc.root);
    return lead && ids.includes(lead) ? [lead, ...ids.filter((id) => id !== lead)] : ids;
  };
  return (
    <For each={order()}>
      {(id) => (
        <Show when={doc.panels[id]}>
          {(panel) => (
            <section class={`mcell ${panel().type}`} classList={{ lead: id === lead }} onPointerDown={() => focusPanel(id)}>
              <div class="mhead">
                <LinkButton panel={panel()} />
                <span>{panelTitle(panel())}</span>
              </div>
              <div class="panelbody">
                <PanelView panel={panel()} />
              </div>
            </section>
          )}
        </Show>
      )}
    </For>
  );
}

function NodeView(props: { node: LayoutNode }) {
  return (
    <Switch>
      <Match when={props.node.kind === 'split' && (props.node as SplitNode)}>{(node) => <SplitView node={node()} />}</Match>
      <Match when={props.node.kind === 'tabs' && (props.node as TabsNode)}>{(node) => <TabsView node={node()} />}</Match>
    </Switch>
  );
}

function SplitView(props: { node: SplitNode }) {
  let el!: HTMLDivElement;
  /** Sizes while a gutter is being dragged; committed once, on release. */
  const [live, setLive] = createSignal<number[] | null>(null);
  const [active, setActive] = createSignal(-1);
  const sizes = () => live() ?? props.node.sizes;

  const startDrag = (index: number, e: PointerEvent) => {
    e.preventDefault();
    const gutter = e.currentTarget as HTMLElement;
    gutter.setPointerCapture(e.pointerId);
    const row = props.node.dir === 'row';
    const rect = el.getBoundingClientRect();
    const total = row ? rect.width : rect.height;
    const start = row ? e.clientX : e.clientY;
    const initial = [...props.node.sizes];
    const pair = initial[index]! + initial[index + 1]!;
    const min = Math.min(pair / 2, MIN_CELL / total);
    setActive(index);
    const move = (ev: PointerEvent) => {
      const delta = ((row ? ev.clientX : ev.clientY) - start) / total;
      const a = Math.max(min, Math.min(pair - min, initial[index]! + delta));
      const next = [...initial];
      next[index] = a;
      next[index + 1] = pair - a;
      setLive(next);
    };
    const end = () => {
      gutter.removeEventListener('pointermove', move);
      gutter.removeEventListener('pointerup', end);
      gutter.removeEventListener('pointercancel', end);
      const final = live();
      setLive(null);
      setActive(-1);
      if (final) update((d) => setSizes(d, props.node.id, final));
    };
    gutter.addEventListener('pointermove', move);
    gutter.addEventListener('pointerup', end);
    gutter.addEventListener('pointercancel', end);
  };

  return (
    <div ref={el} class={`split ${props.node.dir}`}>
      <For each={props.node.children}>
        {(child, i) => (
          <>
            <Show when={i() > 0}>
              <div class="gutter" classList={{ active: active() === i() - 1 }} onPointerDown={(e) => startDrag(i() - 1, e)} />
            </Show>
            <div class="cell" style={{ flex: `${sizes()[i()] ?? 1} 1 0` }}>
              <NodeView node={child} />
            </div>
          </>
        )}
      </For>
    </div>
  );
}

const HINT: Record<DropZone, JSX.CSSProperties> = {
  center: { inset: '0' },
  left: { left: '0', top: '0', bottom: '0', width: '50%' },
  right: { right: '0', top: '0', bottom: '0', width: '50%' },
  top: { left: '0', right: '0', top: '0', height: '50%' },
  bottom: { left: '0', right: '0', bottom: '0', height: '50%' },
};

function zoneAt(e: DragEvent, el: HTMLElement): DropZone {
  const r = el.getBoundingClientRect();
  const x = (e.clientX - r.left) / r.width;
  const y = (e.clientY - r.top) / r.height;
  const edge = Math.min(x, 1 - x, y, 1 - y);
  if (edge > 0.22) return 'center';
  if (edge === x) return 'left';
  if (edge === 1 - x) return 'right';
  return edge === y ? 'top' : 'bottom';
}

function TabsView(props: { node: TabsNode }) {
  const [zone, setZone] = createSignal<DropZone | null>(null);
  const [adding, setAdding] = createSignal(false);
  useDismiss(adding, () => setAdding(false));
  const active = () => doc.panels[props.node.active];
  const focused = () => props.node.panels.includes(focusedPanelId() ?? '');

  const drop = (where: DropZone) => {
    const id = dragging();
    setZone(null);
    setDragging(null);
    if (id) update((d) => movePanel(d, id, props.node.id, where));
  };

  const addHere = (type: PanelType, where: DropZone) => {
    setAdding(false);
    const from = active();
    const panel = newPanel(type, from ? { link: from.link, market: marketOf(from), timeframe: timeframeOf(from) } : undefined);
    update((d) => addPanel(d, panel, props.node.id, where));
    focusPanel(panel.id);
  };

  return (
    <div class="tabs" classList={{ focused: focused() }} onPointerDown={() => focusPanel(props.node.active)}>
      <div
        class="tabstrip"
        onDragOver={(e) => {
          if (!dragging()) return;
          e.preventDefault();
          setZone(null);
        }}
        onDrop={(e) => {
          e.preventDefault();
          drop('center');
        }}
      >
        <div class="tablist">
          <For each={props.node.panels}>{(id) => <Tab id={id} tabs={props.node} />}</For>
        </div>
        <div class="tabtools">
          <button class="iconbtn" data-tip="Add a panel to this group" onPointerDown={(e) => e.stopPropagation()} onClick={() => setAdding((v) => !v)}>
            <Icon name="plus" size={12} />
          </button>
          <button class="iconbtn" data-tip="Split right" onClick={() => addHere(active()?.type ?? 'chart', 'right')}>
            <Icon name="splitRight" size={12} />
          </button>
          <button class="iconbtn" data-tip="Split down" onClick={() => addHere(active()?.type ?? 'chart', 'bottom')}>
            <Icon name="splitDown" size={12} />
          </button>
          <Show when={adding()}>
            <div class="menu" style={{ right: '0', top: '21px' }} onPointerDown={(e) => e.stopPropagation()}>
              <For each={PANEL_TYPES}>
                {(type) => (
                  <button onClick={() => addHere(type, 'center')}>
                    {PANEL_TITLE[type]}
                    <span class="dim">new tab</span>
                  </button>
                )}
              </For>
            </div>
          </Show>
        </div>
      </div>
      <div class="panelbody">
        <Show when={active()} keyed>
          {(panel) => <PanelView panel={panel} />}
        </Show>
        <Show when={dragging()}>
          <div
            class="dropcover"
            onDragOver={(e) => {
              e.preventDefault();
              setZone(zoneAt(e, e.currentTarget));
            }}
            onDragLeave={() => setZone(null)}
            onDrop={(e) => {
              e.preventDefault();
              drop(zoneAt(e, e.currentTarget));
            }}
          >
            <Show when={zone()}>{(z) => <div class="drophint" style={HINT[z()]} />}</Show>
          </div>
        </Show>
      </div>
    </div>
  );
}

function Tab(props: { id: string; tabs: TabsNode }) {
  const panel = () => doc.panels[props.id];
  return (
    <Show when={panel()}>
      {(p) => (
        <div
          class="tab"
          classList={{ active: props.tabs.active === props.id }}
          draggable={true}
          data-tip={`${panelTitle(p())} — drag onto another group, or its edge, to dock it there`}
          onDragStart={(e) => {
            e.dataTransfer?.setData('text/plain', props.id);
            if (e.dataTransfer) e.dataTransfer.effectAllowed = 'move';
            // Changing the page during dragstart cancels the drag in Chromium.
            requestAnimationFrame(() => setDragging(props.id));
          }}
          onDragEnd={() => setDragging(null)}
          onClick={() => {
            if (props.tabs.active !== props.id) update((d) => activate(d, props.tabs.id, props.id));
            focusPanel(props.id);
          }}
          onAuxClick={(e) => {
            if (e.button === 1) update((d) => removePanel(d, props.id));
          }}
        >
          <LinkButton panel={p()} />
          <span class="label">{panelTitle(p())}</span>
          <button
            class="close"
            data-tip="Close panel"
            onClick={(e) => {
              e.stopPropagation();
              update((d) => removePanel(d, props.id));
            }}
          >
            <Icon name="close" size={10} stroke={2.2} />
          </button>
        </div>
      )}
    </Show>
  );
}

/** Click to cycle through the link groups: panels in one group share a market. */
function LinkButton(props: { panel: Panel }) {
  const next = () => {
    const i = props.panel.link ? LINK_COLORS.indexOf(props.panel.link) : -1;
    return LINK_COLORS[i + 1] ?? null;
  };
  return (
    <button
      class="linkbtn"
      classList={{ linked: props.panel.link !== null }}
      style={props.panel.link ? { color: `var(--link-${props.panel.link})` } : undefined}
      data-tip={props.panel.link ? `Linked to the ${props.panel.link} group — click for the next group` : 'Not linked — click to join a group that shares one market'}
      onClick={(e) => {
        e.stopPropagation();
        setPanelLink(props.panel.id, next());
      }}
    >
      <Icon name="link" size={11} stroke={2.2} />
    </button>
  );
}

function PanelView(props: { panel: Panel }) {
  const p = props.panel;
  switch (p.type) {
    case 'chart':
      return <ChartPanel panel={p} />;
    case 'watchlist':
      return <WatchlistPanel panel={p} />;
    case 'values':
      return <ValuesPanel panel={p} />;
    case 'liquidations':
      return <LiquidationsPanel panel={p} />;
  }
}
