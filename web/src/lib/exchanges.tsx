/**
 * Exchanges and their marks. Binance and Bybit are live; the rest are listed so
 * their logos and names are ready when their venues, streams and history
 * loaders are added — flip `live` then, and they appear in search and filters.
 * Marks are the official ones, on a black disc, from rsalianto's exchange icons.
 */
import type { Component, JSX } from 'solid-js';
import { Dynamic, For } from 'solid-js/web';
import { MARKS, type ExchangeMark } from './exchange-marks';
import { parseMarket, type Venue } from './markets';

export type ExchangeId = 'binance' | 'bybit' | 'okx' | 'bitget' | 'hyperliquid' | 'kraken' | 'lighter' | 'gate' | 'htx' | 'bitmex' | 'bitfinex' | 'aster' | 'deribit';

interface LogoProps {
  size?: number;
}

export interface ExchangeDef {
  id: ExchangeId;
  name: string;
  /** Venues this app can chart, stream and store. */
  live: boolean;
  Logo: Component<LogoProps>;
}

function Mark(props: LogoProps & { viewBox: string; children: JSX.Element }) {
  const size = () => props.size ?? 14;
  return (
    <svg class="xlogo" width={size()} height={size()} viewBox={props.viewBox} aria-hidden="true">
      {props.children}
    </svg>
  );
}

/** A mark whose paths the chart engine draws too — one source of artwork. */
function Paths(props: LogoProps & { mark: ExchangeMark }) {
  return (
    <Mark size={props.size} viewBox={`0 0 ${props.mark.box} ${props.mark.box}`}>
      <For each={props.mark.paths}>{(path) => <path d={path.d} fill={path.fill} transform={path.t ? `translate(${path.t[0]} ${path.t[1]}) scale(${path.t[2]})` : undefined} />}</For>
    </Mark>
  );
}

/** Every mark is path data the engine draws too — one source of artwork. */
const logo = (id: ExchangeId): Component<LogoProps> => (p) => <Paths size={p.size} mark={MARKS[id]} />;

export const EXCHANGES: Record<ExchangeId, ExchangeDef> = {
  binance: { id: 'binance', name: 'Binance', live: true, Logo: logo('binance') },
  bybit: { id: 'bybit', name: 'Bybit', live: true, Logo: logo('bybit') },
  okx: { id: 'okx', name: 'OKX', live: false, Logo: logo('okx') },
  bitget: { id: 'bitget', name: 'Bitget', live: false, Logo: logo('bitget') },
  hyperliquid: { id: 'hyperliquid', name: 'Hyperliquid', live: false, Logo: logo('hyperliquid') },
  kraken: { id: 'kraken', name: 'Kraken', live: false, Logo: logo('kraken') },
  lighter: { id: 'lighter', name: 'Lighter', live: false, Logo: logo('lighter') },
  gate: { id: 'gate', name: 'Gate', live: false, Logo: logo('gate') },
  htx: { id: 'htx', name: 'HTX', live: false, Logo: logo('htx') },
  bitmex: { id: 'bitmex', name: 'BitMEX', live: false, Logo: logo('bitmex') },
  bitfinex: { id: 'bitfinex', name: 'Bitfinex', live: false, Logo: logo('bitfinex') },
  aster: { id: 'aster', name: 'Aster', live: false, Logo: logo('aster') },
  deribit: { id: 'deribit', name: 'Deribit', live: false, Logo: logo('deribit') },
};

export const ALL_EXCHANGES: readonly ExchangeDef[] = Object.values(EXCHANGES);
export const LIVE_EXCHANGES: readonly ExchangeDef[] = ALL_EXCHANGES.filter((e) => e.live);

export const exchangeOfVenue = (venue: Venue | string): ExchangeDef => EXCHANGES[venue.split('-')[0] as ExchangeId] ?? EXCHANGES.binance;

export function ExchangeLogo(props: { exchange: ExchangeDef; size?: number }) {
  return <Dynamic component={props.exchange.Logo} size={props.size} />;
}

/** Logo, optionally the exchange's name, and PERP or SPOT. */
export function VenueTag(props: { market: string; name?: boolean; size?: number }) {
  const ref = () => parseMarket(props.market);
  const exchange = () => exchangeOfVenue(ref()?.venue ?? 'binance');
  return (
    <span class="venue" title={ref() ? `${exchange().name} ${ref()!.venue.endsWith('perp') ? 'perpetual' : 'spot'}` : undefined}>
      <ExchangeLogo exchange={exchange()} size={props.size} />
      {props.name ? <span class="venue-name">{exchange().name}</span> : null}
      <span class="kind">{ref()?.venue.endsWith('perp') ? 'PERP' : 'SPOT'}</span>
    </span>
  );
}
