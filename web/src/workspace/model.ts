/**
 * The workspace: a tree of splits and tab groups, the panels in it, the link
 * groups that make panels follow one market, and theme overrides. Stored whole
 * as one JSON document, locally and on the server.
 *
 * Every operation here mutates a draft; the store clones, applies and
 * reconciles, so the tree functions stay plain and testable.
 */
import { DEFAULT_MARKET, isMarket } from '../lib/markets';
import { isTimeframe } from '../lib/timeframes';

export type PanelType = 'chart' | 'watchlist' | 'tpo' | 'values';
export type LinkColor = 'green' | 'blue' | 'red' | 'yellow';

export const LINK_COLORS: readonly LinkColor[] = ['green', 'blue', 'red', 'yellow'];
export const PANEL_TYPES: readonly PanelType[] = ['chart', 'watchlist', 'tpo', 'values'];
export const PANEL_TITLE: Record<PanelType, string> = { chart: 'Chart', watchlist: 'Watchlist', tpo: 'TPO', values: 'Values' };

interface PanelBase {
  id: string;
  /** Linked panels read and write their group's market and timeframe. */
  link: LinkColor | null;
  market: string;
  timeframe: string;
}

export interface ChartPanelState extends PanelBase {
  type: 'chart';
  /** `rsi:14,ema:50` — the spelling the engine parses. */
  studies: string;
  /** Hidden layer ids: ribbon, ema200, volume, or a study id. */
  hidden: string;
  log: boolean;
  /** `price:3,pct:1`. */
  paneWeights: string;
}

export interface WatchlistPanelState extends PanelBase {
  type: 'watchlist';
  markets: string[];
}

export interface TpoPanelState extends PanelBase {
  type: 'tpo';
  sessions: number;
}

export interface ValuesPanelState extends PanelBase {
  type: 'values';
  basis: 'volume' | 'tpo';
}

export type Panel = ChartPanelState | WatchlistPanelState | TpoPanelState | ValuesPanelState;

export interface SplitNode {
  kind: 'split';
  id: string;
  dir: 'row' | 'column';
  /** Fractions of the split, summing to one. */
  sizes: number[];
  children: LayoutNode[];
}

export interface TabsNode {
  kind: 'tabs';
  id: string;
  panels: string[];
  active: string;
}

export type LayoutNode = SplitNode | TabsNode;

export interface LinkState {
  market: string;
  timeframe: string;
}

export interface WorkspaceDoc {
  v: 1;
  name: string;
  /** When this copy last changed, so a browser and the server can tell which is newer. */
  savedAt: number;
  root: LayoutNode;
  panels: Record<string, Panel>;
  links: Record<LinkColor, LinkState>;
  /** Chart theme keys, and `ui.*` for the page's own colours. */
  theme: Record<string, string>;
}

export type DropZone = 'center' | 'left' | 'right' | 'top' | 'bottom';

export const DEFAULT_STUDIES = 'rsi:14,stochrsi:14:14:3:3';
export const DEFAULT_TIMEFRAME = '1h';
const DEFAULT_WATCHLIST = ['binance-perp:BTCUSDT', 'binance-perp:ETHUSDT', 'binance-perp:SOLUSDT', 'binance-spot:BTCUSDT', 'bybit-perp:BTCUSDT'];

export const uid = (prefix: string): string => `${prefix}-${Math.random().toString(36).slice(2, 9)}`;

export function newPanel(type: PanelType, from?: Partial<Pick<PanelBase, 'link' | 'market' | 'timeframe'>>): Panel {
  const base = { id: uid(type), link: from?.link ?? null, market: from?.market ?? DEFAULT_MARKET, timeframe: from?.timeframe ?? DEFAULT_TIMEFRAME };
  switch (type) {
    case 'chart':
      return { ...base, type, studies: DEFAULT_STUDIES, hidden: '', log: false, paneWeights: '' };
    case 'watchlist':
      return { ...base, type, markets: [...DEFAULT_WATCHLIST] };
    case 'tpo':
      return { ...base, type, sessions: 12 };
    case 'values':
      return { ...base, type, basis: 'volume' };
  }
}

const tabsFor = (panel: Panel): TabsNode => ({ kind: 'tabs', id: uid('tabs'), panels: [panel.id], active: panel.id });

export function defaultWorkspace(): WorkspaceDoc {
  const group = { link: 'green' as const, market: DEFAULT_MARKET, timeframe: DEFAULT_TIMEFRAME };
  const chart = newPanel('chart', group);
  const tpo = newPanel('tpo', group);
  const values = newPanel('values', group);
  const watchlist = newPanel('watchlist', group);
  const links = {} as Record<LinkColor, LinkState>;
  for (const c of LINK_COLORS) links[c] = { market: DEFAULT_MARKET, timeframe: DEFAULT_TIMEFRAME };
  return {
    v: 1,
    name: 'Default',
    savedAt: 0,
    root: {
      kind: 'split',
      id: uid('split'),
      dir: 'row',
      sizes: [0.8, 0.2],
      children: [
        {
          kind: 'split',
          id: uid('split'),
          dir: 'column',
          sizes: [0.7, 0.3],
          children: [tabsFor(chart), { kind: 'split', id: uid('split'), dir: 'row', sizes: [0.62, 0.38], children: [tabsFor(tpo), tabsFor(values)] }],
        },
        tabsFor(watchlist),
      ],
    },
    panels: { [chart.id]: chart, [tpo.id]: tpo, [values.id]: values, [watchlist.id]: watchlist },
    links,
    theme: {},
  };
}

// ---------------------------------------------------------------- reading stored documents

const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const str = (v: unknown, fallback: string): string => (typeof v === 'string' ? v : fallback);
const isLink = (v: unknown): v is LinkColor => LINK_COLORS.includes(v as LinkColor);

function readPanel(id: string, raw: unknown): Panel | null {
  if (!isObj(raw) || !PANEL_TYPES.includes(raw.type as PanelType)) return null;
  const p = newPanel(raw.type as PanelType, {
    link: isLink(raw.link) ? raw.link : null,
    market: isMarket(raw.market) ? raw.market : DEFAULT_MARKET,
    timeframe: isTimeframe(raw.timeframe) ? raw.timeframe : DEFAULT_TIMEFRAME,
  });
  p.id = id;
  if (p.type === 'chart') {
    p.studies = str(raw.studies, p.studies);
    p.hidden = str(raw.hidden, '');
    p.log = raw.log === true;
    p.paneWeights = str(raw.paneWeights, '');
  } else if (p.type === 'watchlist') {
    if (Array.isArray(raw.markets)) p.markets = [...new Set(raw.markets.filter(isMarket))].slice(0, 200);
  } else if (p.type === 'tpo') {
    const n = Number(raw.sessions);
    if (Number.isInteger(n) && n >= 1 && n <= 60) p.sessions = n;
  } else if (raw.basis === 'tpo') {
    p.basis = 'tpo';
  }
  return p;
}

function readNode(raw: unknown, panels: Record<string, Panel>, placed: Set<string>): LayoutNode | null {
  if (!isObj(raw) || typeof raw.id !== 'string') return null;
  if (raw.kind === 'tabs') {
    const ids = Array.isArray(raw.panels) ? raw.panels.filter((p): p is string => typeof p === 'string' && p in panels && !placed.has(p)) : [];
    for (const id of ids) placed.add(id);
    if (ids.length === 0) return null;
    return { kind: 'tabs', id: raw.id, panels: ids, active: typeof raw.active === 'string' && ids.includes(raw.active) ? raw.active : ids[0]! };
  }
  if (raw.kind !== 'split' || !Array.isArray(raw.children)) return null;
  const rawSizes = Array.isArray(raw.sizes) ? raw.sizes : [];
  const children: LayoutNode[] = [];
  const sizes: number[] = [];
  raw.children.forEach((child, i) => {
    const node = readNode(child, panels, placed);
    if (!node) return;
    const size = Number(rawSizes[i]);
    children.push(node);
    sizes.push(Number.isFinite(size) && size > 0 ? size : 1);
  });
  return normalize({ kind: 'split', id: raw.id, dir: raw.dir === 'column' ? 'column' : 'row', sizes, children });
}

/** A stored document made safe to render, or null when it cannot be. */
export function validate(raw: unknown): WorkspaceDoc | null {
  if (!isObj(raw) || raw.v !== 1 || !isObj(raw.panels)) return null;
  const panels: Record<string, Panel> = {};
  for (const [id, p] of Object.entries(raw.panels)) {
    const panel = readPanel(id, p);
    if (panel) panels[id] = panel;
  }
  const placed = new Set<string>();
  const root = readNode(raw.root, panels, placed);
  if (!root) return null;
  for (const id of Object.keys(panels)) if (!placed.has(id)) delete panels[id];
  const links = {} as Record<LinkColor, LinkState>;
  for (const c of LINK_COLORS) {
    const l = isObj(raw.links) ? raw.links[c] : null;
    links[c] = {
      market: isObj(l) && isMarket(l.market) ? l.market : DEFAULT_MARKET,
      timeframe: isObj(l) && isTimeframe(l.timeframe) ? l.timeframe : DEFAULT_TIMEFRAME,
    };
  }
  const theme: Record<string, string> = {};
  if (isObj(raw.theme)) for (const [k, v] of Object.entries(raw.theme)) if (typeof v === 'string' && v.length <= 40) theme[k] = v;
  const savedAt = Number(raw.savedAt);
  return { v: 1, name: str(raw.name, '').slice(0, 120) || 'Default', savedAt: Number.isFinite(savedAt) ? savedAt : 0, root, panels, links, theme };
}

// ---------------------------------------------------------------- the tree

/** Drop empty tab groups, collapse one-child splits, flatten same-direction ones. */
export function normalize(node: LayoutNode): LayoutNode | null {
  if (node.kind === 'tabs') {
    if (node.panels.length === 0) return null;
    if (!node.panels.includes(node.active)) node.active = node.panels[0]!;
    return node;
  }
  const children: LayoutNode[] = [];
  const sizes: number[] = [];
  node.children.forEach((child, i) => {
    const n = normalize(child);
    if (!n) return;
    const size = node.sizes[i] ?? 1;
    if (n.kind === 'split' && n.dir === node.dir) {
      const total = n.sizes.reduce((a, b) => a + b, 0) || 1;
      n.children.forEach((c, j) => {
        children.push(c);
        sizes.push((size * (n.sizes[j] ?? 1)) / total);
      });
    } else {
      children.push(n);
      sizes.push(size);
    }
  });
  if (children.length === 0) return null;
  if (children.length === 1) return children[0]!;
  const total = sizes.reduce((a, b) => a + b, 0) || 1;
  node.children = children;
  node.sizes = sizes.map((s) => s / total);
  return node;
}

export function findNode(node: LayoutNode, id: string): LayoutNode | null {
  if (node.id === id) return node;
  if (node.kind === 'split') {
    for (const child of node.children) {
      const hit = findNode(child, id);
      if (hit) return hit;
    }
  }
  return null;
}

function findParent(node: LayoutNode, id: string): SplitNode | null {
  if (node.kind !== 'split') return null;
  for (const child of node.children) {
    if (child.id === id) return node;
    const hit = findParent(child, id);
    if (hit) return hit;
  }
  return null;
}

export function tabsOf(node: LayoutNode, panelId: string): TabsNode | null {
  if (node.kind === 'tabs') return node.panels.includes(panelId) ? node : null;
  for (const child of node.children) {
    const hit = tabsOf(child, panelId);
    if (hit) return hit;
  }
  return null;
}

export function firstTabs(node: LayoutNode): TabsNode {
  return node.kind === 'tabs' ? node : firstTabs(node.children[0]!);
}

function insert(doc: WorkspaceDoc, panelId: string, targetId: string | null, zone: DropZone): void {
  const found = targetId ? findNode(doc.root, targetId) : null;
  const target = found?.kind === 'tabs' ? found : firstTabs(doc.root);
  if (zone === 'center') {
    target.panels.push(panelId);
    target.active = panelId;
    return;
  }
  const fresh: TabsNode = { kind: 'tabs', id: uid('tabs'), panels: [panelId], active: panelId };
  const dir = zone === 'left' || zone === 'right' ? 'row' : 'column';
  const before = zone === 'left' || zone === 'top';
  const parent = findParent(doc.root, target.id);
  if (parent && parent.dir === dir) {
    const i = parent.children.indexOf(target);
    const half = (parent.sizes[i] ?? 1) / 2;
    parent.sizes[i] = half;
    parent.children.splice(before ? i : i + 1, 0, fresh);
    parent.sizes.splice(before ? i : i + 1, 0, half);
    return;
  }
  const split: SplitNode = { kind: 'split', id: uid('split'), dir, sizes: [0.5, 0.5], children: before ? [fresh, target] : [target, fresh] };
  if (parent) parent.children[parent.children.indexOf(target)] = split;
  else doc.root = split;
}

export function addPanel(doc: WorkspaceDoc, panel: Panel, targetId: string | null, zone: DropZone): void {
  doc.panels[panel.id] = panel;
  insert(doc, panel.id, targetId, zone);
}

export function removePanel(doc: WorkspaceDoc, panelId: string): void {
  const tabs = tabsOf(doc.root, panelId);
  if (!tabs) return;
  tabs.panels = tabs.panels.filter((p) => p !== panelId);
  delete doc.panels[panelId];
  const root = normalize(doc.root);
  if (root) {
    doc.root = root;
    return;
  }
  // The last panel went: an empty workspace has nothing to click, so start a chart.
  const chart = newPanel('chart');
  doc.panels[chart.id] = chart;
  doc.root = tabsFor(chart);
}

export function movePanel(doc: WorkspaceDoc, panelId: string, targetId: string, zone: DropZone): void {
  const source = tabsOf(doc.root, panelId);
  const target = findNode(doc.root, targetId);
  if (!source || target?.kind !== 'tabs') return;
  if (source === target && (zone === 'center' || source.panels.length === 1)) {
    source.active = panelId;
    return;
  }
  source.panels = source.panels.filter((p) => p !== panelId);
  const root = normalize(doc.root);
  if (!root) return;
  doc.root = root;
  insert(doc, panelId, targetId, zone);
}

export function activate(doc: WorkspaceDoc, tabsId: string, panelId: string): void {
  const node = findNode(doc.root, tabsId);
  if (node?.kind === 'tabs' && node.panels.includes(panelId)) node.active = panelId;
}

export function setSizes(doc: WorkspaceDoc, splitId: string, sizes: number[]): void {
  const node = findNode(doc.root, splitId);
  if (node?.kind !== 'split' || sizes.length !== node.children.length) return;
  const total = sizes.reduce((a, b) => a + b, 0) || 1;
  node.sizes = sizes.map((s) => s / total);
}
