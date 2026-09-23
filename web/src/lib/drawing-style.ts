/**
 * How a drawing looks and behaves, as the flat `style` object it is saved with.
 * The engine reads the same keys (see `DrawingStyle::set` in the render crate);
 * anything left out falls back to the theme and to what suits the kind.
 */
import { readJson, writePref } from './prefs';

export type Style = Record<string, string | number | boolean>;

export interface Level {
  value: number;
  on: boolean;
  color: string;
  dash: Dash;
}

export type Dash = 'solid' | 'dashed' | 'dotted';

export const KIND_NAMES: Record<string, string> = {
  path: 'Path',
  brush: 'Brush',
  trendline: 'Trendline',
  horizontal: 'Horizontal line',
  ray: 'Horizontal ray',
  rect: 'Rectangle',
  fib: 'Fib retracement',
  text: 'Text',
  measure: 'Price range',
  long: 'Long position',
  short: 'Short position',
  channel: 'Parallel channel',
};

export const isPosition = (kind: string): boolean => kind === 'long' || kind === 'short';

/** A palette in the spirit of TradingView's: greys, then hues in three strengths. */
export const PALETTE = [
  '#ffffff', '#d1d4dc', '#b2b5be', '#9598a1', '#787b86', '#5d606b', '#434651', '#2a2e39', '#131722', '#000000',
  '#f23645', '#ff9800', '#ffeb3b', '#4caf50', '#089981', '#00bcd4', '#2962ff', '#673ab7', '#9c27b0', '#e91e63',
  '#fccbcd', '#ffe0b2', '#fff9c4', '#c8e6c9', '#ace5dc', '#b2ebf2', '#bbd9fb', '#d1c4e9', '#e1bee7', '#f8bbd0',
  '#f7525f', '#ffb74d', '#fff176', '#81c784', '#42bda8', '#4dd0e1', '#5b9cf6', '#9575cd', '#ba68c8', '#f06292',
  '#b22833', '#f57c00', '#fbc02d', '#388e3c', '#056656', '#0097a7', '#1848cc', '#512da8', '#7b1fa2', '#c2185b',
];

const CHANNEL_LEVELS: Level[] = [
  { value: -0.25, on: false, color: '#2962ff', dash: 'solid' },
  { value: 0, on: true, color: '', dash: 'solid' },
  { value: 0.25, on: false, color: '#2962ff', dash: 'solid' },
  { value: 0.5, on: true, color: '#ff6d00', dash: 'dashed' },
  { value: 0.75, on: false, color: '#2962ff', dash: 'solid' },
  { value: 1, on: true, color: '', dash: 'solid' },
  { value: 1.25, on: false, color: '#2962ff', dash: 'solid' },
];

const FIB_LEVELS: Level[] = [
  { value: 0, on: true, color: '#787b86', dash: 'solid' },
  { value: 0.236, on: true, color: '#f23645', dash: 'solid' },
  { value: 0.382, on: true, color: '#ff9800', dash: 'solid' },
  { value: 0.5, on: true, color: '#4caf50', dash: 'solid' },
  { value: 0.618, on: true, color: '#089981', dash: 'solid' },
  { value: 0.786, on: true, color: '#00bcd4', dash: 'solid' },
  { value: 1, on: true, color: '#787b86', dash: 'solid' },
  { value: 1.618, on: false, color: '#2962ff', dash: 'solid' },
  { value: 2.618, on: false, color: '#f23645', dash: 'solid' },
  { value: 3.618, on: false, color: '#9c27b0', dash: 'solid' },
  { value: 4.236, on: false, color: '#e91e63', dash: 'solid' },
];

export const serializeLevels = (levels: Level[]): string => levels.map((l) => `${l.value},${l.on ? 1 : 0},${l.color},${l.dash}`).join('|');

export function parseLevels(raw: unknown, kind: string): Level[] {
  if (typeof raw !== 'string' || raw === '') return (kind === 'channel' ? CHANNEL_LEVELS : FIB_LEVELS).map((l) => ({ ...l }));
  return raw.split('|').flatMap((item) => {
    const [v, on, color, dash] = item.split(',');
    const value = Number(v);
    if (!Number.isFinite(value)) return [];
    return [{ value, on: on !== '0', color: color ?? '', dash: dash === 'dashed' || dash === 'dotted' ? dash : 'solid' }];
  });
}

/** What a fresh drawing of each kind looks like before anyone changes it. */
export function factoryStyle(kind: string): Style {
  switch (kind) {
    case 'long':
    case 'short':
      return { account: 1000, risk: 1, riskUnit: '%', lot: 1, leverage: 1, profitLine: '#089981', stopLine: '#f23645', profitFill: '#08998133', stopFill: '#f2364533' };
    case 'channel':
      return { color: '#2962ff', width: 1, fillOn: true, fill: '#2962ff1f', levels: serializeLevels(CHANNEL_LEVELS) };
    case 'fib':
      return { width: 1, fillOn: true, fill: '#2962ff14', levels: serializeLevels(FIB_LEVELS), extendRight: false };
    case 'rect':
      return { color: '#9c27b0', width: 1, fillOn: true, fill: '#9c27b033' };
    case 'trendline':
    case 'ray':
    case 'horizontal':
      return { color: '#2962ff', width: 1.5 };
    case 'text':
      return { textColor: '#2962ff', textSize: 14 };
    default:
      return {};
  }
}

// ---------------------------------------------------------------- templates

const TEMPLATE_KEY = 'drawingTemplates';

function templates(): Record<string, Style> {
  const raw = readJson(TEMPLATE_KEY);
  return raw && typeof raw === 'object' ? (raw as Record<string, Style>) : {};
}

/** The style a new drawing of `kind` starts with: the saved default, else the factory one. */
export function defaultStyle(kind: string): Style {
  return { ...factoryStyle(kind), ...(templates()[kind] ?? {}) };
}

export function saveDefaultStyle(kind: string, style: Style): void {
  const all = templates();
  all[kind] = style;
  writePref(TEMPLATE_KEY, JSON.stringify(all));
}

export function resetDefaultStyle(kind: string): void {
  const all = templates();
  delete all[kind];
  writePref(TEMPLATE_KEY, JSON.stringify(all));
}

// ---------------------------------------------------------------- visibility

/** Timeframe families a drawing can be hidden on, by the timeframe's unit letter. */
export const VISIBILITY_UNITS: { unit: string; label: string }[] = [
  { unit: 'm', label: 'Minutes' },
  { unit: 'h', label: 'Hours' },
  { unit: 'd', label: 'Days' },
  { unit: 'w', label: 'Weeks' },
  { unit: 'M', label: 'Months' },
];

const hiddenUnits = (style: Style): string[] => (typeof style.hideOn === 'string' && style.hideOn ? style.hideOn.split(',') : []);

export const visibleOn = (style: Style, timeframe: string): boolean => !hiddenUnits(style).includes(timeframe.slice(-1));

export function withHidden(style: Style, unit: string, hidden: boolean): Style {
  const set = new Set(hiddenUnits(style));
  if (hidden) set.add(unit);
  else set.delete(unit);
  return { ...style, hideOn: [...set].join(',') };
}

// ---------------------------------------------------------------- colours

/** `#rrggbb` and an opacity 0–100 out of any `#rgb`, `#rrggbb` or `#rrggbbaa`. */
export function splitColor(color: unknown, fallback = '#2962ff'): { hex: string; opacity: number } {
  let c = typeof color === 'string' && color.startsWith('#') ? color.slice(1) : fallback.slice(1);
  if (c.length === 3) c = [...c].map((ch) => ch + ch).join('');
  const hex = `#${c.slice(0, 6).padEnd(6, '0')}`;
  const opacity = c.length >= 8 ? Math.round((parseInt(c.slice(6, 8), 16) / 255) * 100) : 100;
  return { hex, opacity: Number.isFinite(opacity) ? opacity : 100 };
}

export function joinColor(hex: string, opacity: number): string {
  if (opacity >= 100) return hex;
  const a = Math.round((Math.max(0, opacity) / 100) * 255)
    .toString(16)
    .padStart(2, '0');
  return `${hex}${a}`;
}
