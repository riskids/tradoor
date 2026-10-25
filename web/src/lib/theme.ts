/** Colours a workspace can override. Chart keys go to the engine; `ui.*` keys
 *  set the page's CSS tokens. Defaults mirror tradoor-render's Theme and styles.css. */

/** Which tab of Chart Settings a key is edited on, TradingView-style. */
export type ThemeGroup = 'candles' | 'canvas' | 'scales' | 'studies' | 'drawings';

export interface ThemeKey {
  name: string;
  label: string;
  value: string;
}

/** A `CHART_THEME` entry: which Chart Settings tab it belongs on, TradingView-style. */
export interface ChartThemeKey extends ThemeKey {
  group: ThemeGroup;
}

export const CHART_THEME: readonly ChartThemeKey[] = [
  // TradingView's default dark theme: a black canvas, white up candles, blue down.
  { name: 'background', label: 'Background', value: '#0f0f0f', group: 'canvas' },
  { name: 'text', label: 'Axis text', value: '#b2b5be', group: 'scales' },
  { name: 'border', label: 'Grid and borders', value: '#2a2e39', group: 'canvas' },
  { name: 'grid', label: 'Grid lines', value: '#1c1c1c', group: 'canvas' },
  { name: 'bull', label: 'Up candles', value: '#ffffff', group: 'candles' },
  { name: 'bear', label: 'Down candles', value: '#2196f3', group: 'candles' },
  { name: 'wickBull', label: 'Wick up', value: '#ffffff', group: 'candles' },
  { name: 'wickBear', label: 'Wick down', value: '#2196f3', group: 'candles' },
  { name: 'borderBull', label: 'Border up', value: '#ffffff', group: 'candles' },
  { name: 'borderBear', label: 'Border down', value: '#2196f3', group: 'candles' },
  { name: 'volumeBull', label: 'Volume up', value: '#ffffff', group: 'candles' },
  { name: 'volumeBear', label: 'Volume down', value: '#2196f3', group: 'candles' },
  { name: 'ribbonFast', label: 'Ribbon, bullish', value: '#2962ff', group: 'studies' },
  { name: 'ribbonSlow', label: 'Ribbon, bearish', value: '#ef5350', group: 'studies' },
  { name: 'ema200', label: 'EMA 200', value: '#ff9800', group: 'studies' },
  { name: 'crosshair', label: 'Crosshair', value: '#9598a1', group: 'canvas' },
  { name: 'labelBackground', label: 'Axis labels', value: '#ffffff', group: 'scales' },
  { name: 'labelText', label: 'Axis label text', value: '#131722', group: 'scales' },
  { name: 'drawing', label: 'Drawings', value: '#2962ff', group: 'drawings' },
  { name: 'drawingSelected', label: 'Selected drawing', value: '#5b8dff', group: 'drawings' },
  { name: 'study0', label: 'Study line 1', value: '#7fb2ff', group: 'studies' },
  { name: 'study1', label: 'Study line 2', value: '#c58fff', group: 'studies' },
  { name: 'study2', label: 'Study line 3', value: '#ffd166', group: 'studies' },
  { name: 'study3', label: 'Study line 4', value: '#4ecdc4', group: 'studies' },
  { name: 'study4', label: 'Study line 5', value: '#ff8fab', group: 'studies' },
  { name: 'study5', label: 'Study line 6', value: '#9ae66e', group: 'studies' },
];

export const FONT_PX = { name: 'fontPx', value: '11' };

export const UI_THEME: readonly (ThemeKey & { css: string })[] = [
  { name: 'ui.primary', css: '--primary', label: 'Accent', value: '#2962ff' },
  { name: 'ui.bg', css: '--bg', label: 'Page', value: '#0f0f0f' },
  { name: 'ui.surface', css: '--surface', label: 'Toolbars and panels', value: '#0f0f0f' },
  { name: 'ui.border', css: '--border', label: 'Borders', value: '#2a2a2a' },
  { name: 'ui.text', css: '--text', label: 'Text', value: '#d1d4dc' },
  { name: 'ui.bull', css: '--bull', label: 'Up', value: '#26a69a' },
  { name: 'ui.bear', css: '--bear', label: 'Down', value: '#ef5350' },
];
