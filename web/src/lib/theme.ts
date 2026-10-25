/** Colours a workspace can override. Chart keys go to the engine; `ui.*` keys
 *  set the page's CSS tokens. Defaults mirror tradoor-render's Theme and styles.css. */

export interface ThemeKey {
  name: string;
  label: string;
  value: string;
}

export const CHART_THEME: readonly ThemeKey[] = [
  { name: 'background', label: 'Background', value: '#131722' },
  { name: 'text', label: 'Axis text', value: '#787b86' },
  { name: 'border', label: 'Grid and borders', value: '#1e222d' },
  { name: 'bull', label: 'Up candles', value: '#26a69a' },
  { name: 'bear', label: 'Down candles', value: '#ef5350' },
  { name: 'ribbonFast', label: 'Ribbon, bullish', value: '#2962ff' },
  { name: 'ribbonSlow', label: 'Ribbon, bearish', value: '#ef5350' },
  { name: 'ema200', label: 'EMA 200', value: '#ff9800' },
  { name: 'crosshair', label: 'Crosshair', value: '#787b86' },
  { name: 'labelBackground', label: 'Axis labels', value: '#2a2e39' },
  { name: 'drawing', label: 'Drawings', value: '#2962ff' },
  { name: 'drawingSelected', label: 'Selected drawing', value: '#5b8dff' },
  { name: 'study0', label: 'Study line 1', value: '#7fb2ff' },
  { name: 'study1', label: 'Study line 2', value: '#c58fff' },
  { name: 'study2', label: 'Study line 3', value: '#ffd166' },
  { name: 'study3', label: 'Study line 4', value: '#4ecdc4' },
  { name: 'study4', label: 'Study line 5', value: '#ff8fab' },
  { name: 'study5', label: 'Study line 6', value: '#9ae66e' },
];

export const FONT_PX = { name: 'fontPx', value: '11' };

export const UI_THEME: readonly (ThemeKey & { css: string })[] = [
  { name: 'ui.primary', css: '--primary', label: 'Accent', value: '#2962ff' },
  { name: 'ui.bg', css: '--bg', label: 'Page', value: '#131722' },
  { name: 'ui.surface', css: '--surface', label: 'Panels', value: '#1e222d' },
  { name: 'ui.border', css: '--border', label: 'Borders', value: '#2a2e39' },
  { name: 'ui.text', css: '--text', label: 'Text', value: '#d1d4dc' },
  { name: 'ui.bull', css: '--bull', label: 'Up', value: '#26a69a' },
  { name: 'ui.bear', css: '--bear', label: 'Down', value: '#ef5350' },
];
