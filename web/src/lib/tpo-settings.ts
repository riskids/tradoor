/**
 * TPO settings as a chart keeps them: only what differs from Exocharts'
 * defaults, which are listed here and are the engine's defaults too. The
 * dialog's layout — tabs, names, hints — is in tpo-schema, loaded with it.
 */

export type TpoSettings = Record<string, string>;

const flag = (on: boolean) => (on ? '1' : '0');

function marker(prefix: string, show: string, colour: string): Record<string, string> {
  return { [show]: '0', [`${prefix}Text`]: '0', [`${prefix}Min`]: '2', [`${prefix}Color`]: colour, [`${prefix}Naked`]: '0' };
}

export const TPO_DEFAULTS: Readonly<Record<string, string>> = {
  // Main
  letters: flag(false),
  minLetterHeight: '12',
  expand: flag(false),
  sessionNames: flag(false),
  openMarker: flag(true),
  closeMarker: flag(false),
  rowTicks: '0',
  // Colors
  colorMode: 'mono',
  vaColor: '#d9dcd9',
  outsideVa: flag(false),
  outsideVaColor: '#4a7fd8',
  heatLow: '#d32f2f',
  heatHigh: '#ffd54f',
  fading: 'time',
  fadingMin: '0.3',
  fadingMax: '1',
  // POC, VA
  vaPct: '70',
  vaLines: flag(true),
  vaExtend: flag(true),
  vaBackground: flag(false),
  vaBackgroundColor: '#d9dcd9',
  nakedVa: flag(false),
  vaRangeText: flag(true),
  pocRay: flag(false),
  pocColor: '#f07a2a',
  nakedPoc: flag(false),
  pocPrice: flag(false),
  volumeVa: flag(false),
  // IB, Stats
  ib: flag(false),
  ibColor: '#5ab6ff',
  ibCandles: '1',
  ibLines: flag(false),
  ibExtend: flag(false),
  ibBackground: flag(false),
  nakedIb: flag(false),
  ibRangeText: flag(false),
  priceStats: 'off',
  // Single prints, Tails
  ...marker('singlePrint', 'singlePrints', '#ffd166'),
  ...marker('sellTail', 'sellTail', '#ff4d5f'),
  ...marker('buyTail', 'buyTail', '#3ddc97'),
};

/** The value in force: the chart's own, or the default. */
export const tpoValue = (settings: TpoSettings | undefined, key: string): string => settings?.[key] ?? TPO_DEFAULTS[key] ?? '';

/** `settings` with `key` set, and dropped again once it equals the default. */
export function withTpoValue(settings: TpoSettings | undefined, key: string, value: string): TpoSettings {
  const next = { ...settings };
  if (value === TPO_DEFAULTS[key]) delete next[key];
  else next[key] = value;
  return next;
}

/** What the engine is sent: known keys only, `key=value;key=value`. */
export function serializeTpo(settings: TpoSettings | undefined): string {
  return Object.entries(settings ?? {})
    .filter(([key, value]) => key in TPO_DEFAULTS && !/[;=]/.test(value))
    .map(([key, value]) => `${key}=${value}`)
    .join(';');
}
