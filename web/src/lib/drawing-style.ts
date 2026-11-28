/**
 * How a drawing looks and behaves, as the flat `style` object it is saved with.
 * The engine reads the same keys (see `DrawingStyle::set` in the render crate);
 * anything left out falls back to the theme and to what suits the kind — a
 * fib's and a channel's levels included. What only the settings dialog needs
 * lives in drawing-options, so it loads with the dialog.
 */
import { readJson } from './prefs';

export type Style = Record<string, string | number | boolean>;

/** Tools that are another kind drawn a set way: a ray is a trendline carried on
 *  past its second end. Saved as that kind, so its settings are that kind's. */
export const PRESETS: Record<string, { kind: string; style: Style }> = {
  trendray: { kind: 'trendline', style: { extendRight: true } },
  extended: { kind: 'trendline', style: { extendLeft: true, extendRight: true } },
  infoline: { kind: 'trendline', style: { stats: true, priceLabels: true } },
  angle: { kind: 'trendline', style: { angle: true } },
};

/** The kind the engine draws for a tool. */
export const engineKind = (tool: string): string => PRESETS[tool]?.kind ?? tool;

/** What a fresh drawing of each kind looks like before anyone changes it. */
export function factoryStyle(kind: string): Style {
  const preset = PRESETS[kind];
  if (preset) return { ...factoryStyle(preset.kind), ...preset.style };
  switch (kind) {
    case 'long':
    case 'short':
      // The target in a quiet grey, the stop a faint red: the price is what stands out.
      return { account: 1000, risk: 1, riskUnit: '%', lot: 1, leverage: 1, profitLine: '#d1d4dc', stopLine: '#f2364566', profitFill: '#d1d4dc1a', stopFill: '#f236451a' };
    case 'channel':
      return { color: '#2962ff', width: 1, fillOn: true, fill: '#2962ff1f' };
    case 'fib':
      return { width: 1, fillOn: true, fill: '#2962ff14', extendRight: false };
    case 'rect':
      return { color: '#9c27b0', width: 1, fillOn: true, fill: '#9c27b033' };
    case 'vrvp':
      return { color: '#2962ff', width: 1, fillOn: true, priceLabels: true, rows: 24 };
    case 'trendline':
    case 'ray':
    case 'horizontal':
    case 'vertical':
    case 'cross':
      return { color: '#2962ff', width: 1.5 };
    case 'text':
      return { textColor: '#2962ff', textSize: 14 };
    default:
      return {};
  }
}

// ---------------------------------------------------------------- templates

export const TEMPLATE_KEY = 'drawingTemplates';

export function templates(): Record<string, Style> {
  const raw = readJson(TEMPLATE_KEY);
  return raw && typeof raw === 'object' ? (raw as Record<string, Style>) : {};
}

/** The style a new drawing of `kind` starts with: the saved default, else the factory one. */
export function defaultStyle(kind: string): Style {
  return { ...factoryStyle(kind), ...(templates()[kind] ?? {}) };
}

// ---------------------------------------------------------------- visibility

export const hiddenUnits = (style: Style): string[] => (typeof style.hideOn === 'string' && style.hideOn ? style.hideOn.split(',') : []);

export const visibleOn = (style: Style, timeframe: string): boolean => !hiddenUnits(style).includes(timeframe.slice(-1));
