/**
 * The TPO settings dialog's layout: Exocharts' tabs, field names and hints.
 * Loaded with the dialog; the defaults themselves live in tpo-settings, which
 * the chart needs from the start.
 */
import { TPO_DEFAULTS } from './tpo-settings';

interface Base {
  key: string;
  label: string;
  hint?: string;
}

export type TpoField =
  | (Base & { kind: 'bool'; default: boolean })
  | (Base & { kind: 'number'; default: number; min: number; max: number; step?: number })
  | (Base & { kind: 'color'; default: string })
  | (Base & { kind: 'choice'; default: string; options: readonly { value: string; label: string }[] });

export interface TpoSection {
  title: string;
  fields: readonly TpoField[];
}

// Defaults are read from TPO_DEFAULTS, so the dialog cannot disagree with the chart.
const bool = (key: string, label: string, hint?: string): TpoField => ({ key, label, kind: 'bool', default: TPO_DEFAULTS[key] === '1', hint });
const color = (key: string, label: string): TpoField => ({ key, label, kind: 'color', default: TPO_DEFAULTS[key] ?? '#000000' });
const number = (key: string, label: string, min: number, max: number, step?: number, hint?: string): TpoField => ({ key, label, kind: 'number', default: Number(TPO_DEFAULTS[key]), min, max, step, hint });
const choice = (key: string, label: string, options: readonly { value: string; label: string }[]): TpoField => ({ key, label, kind: 'choice', default: TPO_DEFAULTS[key] ?? '', options });

function marker(prefix: string, show: string, name: string): TpoField[] {
  return [
    bool(show, name),
    bool(`${prefix}Text`, `${name} range text`),
    number(`${prefix}Min`, `${name} minimum TPO`, 1, 100, 1, 'Rows in sequence before it is shown'),
    color(`${prefix}Color`, `${name} color`),
    bool(`${prefix}Naked`, `Naked ${name.toLowerCase()}`, 'Extends right until price trades back into it'),
  ];
}

export const TPO_SECTIONS: readonly TpoSection[] = [
  {
    title: 'Main',
    fields: [
      bool('letters', 'Show TPO letters', 'Letters where rows are tall enough; blocks otherwise'),
      number('minLetterHeight', 'Min. bar height to show letters', 1, 100),
      bool('expand', 'Expand clusters', 'A bracket as wide as its candle, instead of fitted to the split'),
      bool('sessionNames', 'Show session names'),
      bool('openMarker', 'Profile open arrow'),
      bool('closeMarker', 'Profile close arrow'),
      number('rowTicks', 'TPO row size (ticks × N)', 0, 1_000_000, 1, '0 picks a size from the loaded range'),
    ],
  },
  {
    title: 'Colors',
    fields: [
      choice('colorMode', 'TPO profile color mode', [
          { value: 'mono', label: 'TPO Mono color' },
          { value: 'heatmap', label: 'TPO HeatMap' },
          { value: 'candle', label: 'TPO Candle Up/Down' },
        ]),
      color('vaColor', 'TPO Mono color'),
      bool('outsideVa', 'Outside VA color'),
      color('outsideVaColor', 'Outside VA color'),
      color('heatLow', 'TPO HeatMap color 1'),
      color('heatHigh', 'TPO HeatMap color 2'),
      choice('fading', 'Cluster fading', [
          { value: 'off', label: 'Off' },
          { value: 'time', label: 'Time' },
          { value: 'volume', label: 'Volume' },
        ]),
      number('fadingMin', 'Cluster fading min', 0, 1, 0.05),
      number('fadingMax', 'Cluster fading max', 0, 1, 0.05),
    ],
  },
  {
    title: 'POC, VA',
    fields: [
      number('vaPct', 'Value area %', 1, 100),
      bool('vaLines', 'Show VA lines'),
      bool('vaExtend', 'Extend VA lines', 'To the next profile'),
      bool('vaBackground', 'Show VA background'),
      color('vaBackgroundColor', 'VA background color'),
      bool('nakedVa', 'Naked VA', 'Extends right until price trades back to it'),
      bool('vaRangeText', 'Show VA range text'),
      bool('pocRay', 'Show POC ray'),
      color('pocColor', 'POC color'),
      bool('nakedPoc', 'Naked POC ray', 'Extends right until price trades back to it'),
      bool('pocPrice', 'Show POC price'),
      bool('volumeVa', 'Use volume for VA/POC calculation'),
    ],
  },
  {
    title: 'IB, Stats',
    fields: [
      bool('ib', 'Initial balance'),
      color('ibColor', 'Initial balance color'),
      number('ibCandles', 'No. of first candles for IB', 1, 100),
      bool('ibLines', 'IB lines'),
      bool('ibExtend', 'Extend IB lines'),
      bool('ibBackground', 'Show IB background'),
      bool('nakedIb', 'Naked IB'),
      bool('ibRangeText', 'Show IB range text'),
      choice('priceStats', 'Price statistics', [
          { value: 'off', label: 'Off' },
          { value: 'tpo', label: 'TPO' },
          { value: 'volume', label: 'TPO (Volume)' },
        ]),
    ],
  },
  {
    title: 'Single prints, Tails',
    fields: [...marker('singlePrint', 'singlePrints', 'Single prints'), ...marker('sellTail', 'sellTail', 'Sell tail'), ...marker('buyTail', 'buyTail', 'Buy tail')],
  },
];

