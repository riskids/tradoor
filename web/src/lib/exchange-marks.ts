/**
 * The live exchanges' marks as plain path data, so both the DOM logos and the
 * chart engine draw the same artwork. The engine's text layer fills these with
 * Path2D on its 2D canvas; nothing here imports Solid, so the worker can use it.
 */
import type { ExchangeId } from './exchanges';

/** One filled path, in the mark's own square box. */
export interface MarkPath {
  d: string;
  fill: string;
  /** The group's `translate(tx ty) scale(s)`, applied in that order. */
  t?: [tx: number, ty: number, s: number];
}

export interface ExchangeMark {
  /** Side of the square the paths are drawn in. */
  box: number;
  paths: MarkPath[];
}

/** The black disc every mark sits on, as a path rather than a <circle>. */
const DISC = { d: 'M0 16a16 16 0 1 0 32 0a16 16 0 1 0-32 0', fill: '#000' };

export const MARKS: Partial<Record<ExchangeId, ExchangeMark>> = {
  binance: {
    box: 32,
    paths: [
      DISC,
      {
        t: [-5, -5, 1.75],
        fill: '#F0B90B',
        d: 'M7.6159 12L5.81135 13.8045L4.00226 12L5.80681 10.1955L7.6159 12ZM12.0023 7.61364L15.0977 10.7091L16.9023 8.90455L12.0023 4L7.09772 8.90455L8.90226 10.7091L12.0023 7.61364ZM18.1932 10.1955L16.3886 12L18.1932 13.8045L19.9977 12L18.1932 10.1955ZM12.0023 16.3864L8.90681 13.2909L7.10226 15.0955L12.0023 20L16.9023 15.0955L15.0977 13.2909L12.0023 16.3864ZM12.0023 13.8045L13.8068 12L12.0023 10.1955L10.1932 12L12.0023 13.8045Z',
      },
    ],
  },
  bybit: {
    box: 32,
    paths: [
      DISC,
      { t: [-5, -5, 1.75], fill: '#F6A500', d: 'M15.4033 13.445V9.33372H16.23V13.445H15.4033Z' },
      {
        t: [-5, -5, 1.75],
        fill: '#FFF',
        d: 'M5.77161 14.6663H4V10.555H5.70075C6.5275 10.555 7.00819 11.0049 7.00819 11.71C7.00819 12.1659 6.69875 12.4612 6.48498 12.5592C6.74009 12.675 7.06725 12.9348 7.06725 13.4828C7.06725 14.2505 6.52632 14.6663 5.77161 14.6663ZM5.63461 11.2707H4.82557V12.2179H5.63461C5.98538 12.2179 6.18144 12.0278 6.18144 11.7443C6.18144 11.462 5.98538 11.2707 5.63461 11.2707ZM5.68775 12.9395H4.82557V13.9505H5.68775C6.06215 13.9505 6.2405 13.7202 6.2405 13.4427C6.2405 13.1651 6.06215 12.9407 5.68775 12.9407V12.9395Z',
      },
      { t: [-5, -5, 1.75], fill: '#FFF', d: 'M9.58884 12.9797V14.6663H8.76799V12.9797L7.49598 10.555H8.39359L9.18373 12.212L9.96324 10.555H10.8609L9.58884 12.9797Z' },
      {
        t: [-5, -5, 1.75],
        fill: '#FFF',
        d: 'M13.2041 14.6663H11.4325V10.555H13.1332C13.96 10.555 14.4407 11.0049 14.4407 11.71C14.4407 12.1659 14.1312 12.4612 13.9175 12.5592C14.1726 12.675 14.4997 12.9348 14.4997 13.4828C14.4997 14.2505 13.9588 14.6663 13.2041 14.6663ZM13.0671 11.2707H12.2581V12.2179H13.0671C13.4179 12.2179 13.6139 12.0278 13.6139 11.7443C13.6139 11.462 13.4179 11.2707 13.0671 11.2707ZM13.1202 12.9395H12.2581V13.9505H13.1202C13.4946 13.9505 13.673 13.7202 13.673 13.4427C13.673 13.1651 13.4946 12.9407 13.1202 12.9407V12.9395Z',
      },
      { t: [-5, -5, 1.75], fill: '#FFF', d: 'M18.8933 11.2707V14.6663H18.0678V11.2707H16.9611V10.555H20V11.2707H18.8933Z' },
    ],
  },
};

/** The order the engine numbers marks in: a liquidation names its exchange by index. */
export const MARK_ORDER = Object.keys(MARKS) as ExchangeId[];

/** The engine's number for the exchange behind `binance-perp` and the like. */
export const markIndex = (venue: string): number => Math.max(0, MARK_ORDER.indexOf(venue.split('-')[0] as ExchangeId));

/**
 * The mark as the engine takes it: one path a line, `tx|ty|scale|fill|d`.
 * A plain string keeps the worker boundary free of JSON and the engine free
 * of a parser it would only use here.
 */
export const markSpec = (mark: ExchangeMark): string =>
  mark.paths.map((p) => `${p.t?.[0] ?? 0}|${p.t?.[1] ?? 0}|${p.t?.[2] ?? 1}|${p.fill}|${p.d}`).join('\n');
