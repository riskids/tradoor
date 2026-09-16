/** Messages between the page and the engine worker. */
import type { SocketState } from './streams';

export type LiveStatus = 'off' | SocketState;

export interface Legend {
  index: number | null;
  time: number;
  ohlc: string;
  change: string;
  up: boolean;
  volume: string;
  /** Formatted value per study line, keyed `studyId:lineKey`. */
  values: Record<string, string>;
}

export interface StudyLine {
  id: string;
  label: string;
  color: string;
}

export interface StudyMeta {
  id: string;
  label: string;
  group: string;
  hidden: boolean;
  lines: StudyLine[];
  stats: { label: string; value: string }[];
}

export interface StudyParam {
  name: string;
  default: number;
  min: number;
  max: number;
}

export interface StudyDefinition {
  kind: string;
  name: string;
  short: string;
  group: string;
  about: string;
  params: StudyParam[];
}

export interface DrawingPoint {
  time: number;
  price: number;
}

export interface DrawingShape {
  id: number | null;
  kind: string;
  points: DrawingPoint[];
  label: string | null;
}

export type ChartEvent =
  | { type: 'drawingCreated'; kind: string; points: DrawingPoint[] }
  | { type: 'selection'; id: number | null }
  | { type: 'delete'; id: number }
  | { type: 'drawingChanged'; id: number; points: DrawingPoint[] }
  | { type: 'editRequested'; id: number }
  | { type: 'toolFinished' }
  | { type: 'needHistory'; before: number }
  | { type: 'paneWeights'; value: string }
  | { type: 'autoScale'; value: boolean };

export interface ChartStatus {
  loading: boolean;
  error: string | null;
  bars: number;
  /** Stored partitions, or the exchange's REST API for a market nobody archived. */
  source: 'archive' | 'exchange' | null;
  live: LiveStatus;
  /** The oldest bar there is has been loaded. */
  exhausted: boolean;
}

export interface TpoRow {
  index: number;
  /** Indices into the session's periods, chronological. */
  periods: Uint32Array;
}

export interface TpoSession {
  start: number;
  end: number;
  periods: number;
  high: number;
  low: number;
  poc: number;
  vah: number;
  val: number;
  singlePrints: number[];
  ib: [number, number] | null;
  rows: TpoRow[];
}

export interface TpoData {
  session: string;
  from: string;
  rowSize: number;
  gridLow: number;
  reason: string | null;
  sessions: TpoSession[];
  precision: number;
}

export interface ProfileLevels {
  session: string;
  from: string;
  basis: string;
  vah: number | null;
  val: number | null;
  poc: number | null;
  covered: number;
  total: number;
}

export interface IndicatorValues {
  time: number;
  close: number;
  ema12: number | null;
  ema25: number | null;
  ema200: number | null;
  rsi14: number | null;
  stochK: number | null;
  stochD: number | null;
  profile: ProfileLevels | null;
  precision: number;
}

export type ProfileBasis = 'volume' | 'tpo';

/** Request/response calls, answered by `reply`. */
export interface Calls {
  studyCatalog: { args: Record<string, never>; result: StudyDefinition[] };
  key: { args: { id: string; key: string }; result: boolean };
  tpo: { args: { market: string; timeframe: string; sessions: number }; result: TpoData | null };
  values: { args: { market: string; timeframe: string; basis: ProfileBasis }; result: IndicatorValues | null };
}

export type CallName = keyof Calls;

export type PointerKind = 'move' | 'down' | 'up' | 'leave' | 'dblclick';

export type ToWorker =
  | { t: 'create'; id: string; gl: OffscreenCanvas; text: OffscreenCanvas; width: number; height: number; dpr: number }
  | { t: 'destroy'; id: string }
  | { t: 'resize'; id: string; width: number; height: number; dpr: number }
  | { t: 'market'; id: string; market: string; timeframe: string }
  | { t: 'studies'; id: string; value: string }
  | { t: 'hidden'; id: string; value: string }
  | { t: 'log'; id: string; value: boolean }
  | { t: 'paneWeights'; id: string; value: string }
  | { t: 'tool'; id: string; value: string }
  | { t: 'drawings'; id: string; list: DrawingShape[] }
  | { t: 'selected'; id: string; value: number | null }
  | { t: 'resetView'; id: string }
  | { t: 'autoScale'; id: string }
  | { t: 'range'; id: string; days: number | null }
  | { t: 'pointer'; id: string; kind: PointerKind; x: number; y: number; button: number }
  | { t: 'wheel'; id: string; x: number; y: number; dx: number; dy: number }
  | { t: 'theme'; key: string; value: string }
  /** Development only: fake liquidations, so the ping design can be judged. */
  | { t: 'demoLiquidations'; id: string; perMinute: number }
  | { t: 'call'; req: number; name: CallName; args: unknown };

export type FromWorker =
  | { t: 'ready' }
  | { t: 'fatal'; message: string }
  | { t: 'cursor'; id: string; value: string }
  | { t: 'legend'; id: string; value: Legend }
  | { t: 'studies'; id: string; value: StudyMeta[] }
  | { t: 'event'; id: string; event: ChartEvent }
  | { t: 'status'; id: string; value: ChartStatus }
  | { t: 'price'; id: string; price: number; precision: number }
  | { t: 'reply'; req: number; value?: unknown; error?: string };
